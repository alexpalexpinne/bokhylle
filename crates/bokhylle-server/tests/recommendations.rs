mod common;

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bokhylle_metadata::{MetadataResult, testing::FakeMetadataProvider};
use bokhylle_server::{
    auth::Role,
    library::{import_metadata, queries},
    user_books,
};
use serde_json::{Value, json};
use tower::ServiceExt;

fn metadata(key: &str, subjects: &[&str]) -> MetadataResult {
    MetadataResult {
        provider: "fake".into(), provider_key: key.into(), title: format!("Imaginary {key}"),
        authors: vec![format!("Writer {key}")], subjects: subjects.iter().map(|s| (*s).into()).collect(),
        language: Some("en".into()), languages: vec!["en".into()], cover_id: Some(format!("cover-{key}")),
        description: Some("An imaginary reader follows a distant light across unfamiliar worlds, learning how the stories of a community can change its future and finding a place to call home.".into()),
        ..Default::default()
    }
}

async fn add(app: &common::TestApp, book: &MetadataResult, downloaded: bool) -> i64 {
    let id = import_metadata::upsert_book_from_metadata(&app.state.db, book)
        .await
        .unwrap();
    if downloaded {
        let edition: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO book_files(edition_id,path,format,size,sha256) VALUES(?,?,'epub',1,?)",
        )
        .bind(edition)
        .bind(format!("{}.epub", book.provider_key))
        .bind(&book.provider_key)
        .execute(&app.state.db)
        .await
        .unwrap();
    }
    id
}

async fn reader(app: &common::TestApp) -> (i64, String) {
    let user = app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET preferred_languages = '[\"en\"]' WHERE id = ?")
        .bind(user.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    (user.id, common::login(app, "reader", "password123").await)
}

async fn get(app: &common::TestApp, cookie: &str, uri: &str) -> Value {
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[tokio::test]
async fn updates_do_not_expose_another_profiles_private_titles() {
    let app = common::test_app().await;
    let (_, cookie) = reader(&app).await;
    let owner = app
        .state
        .auth
        .create_user("owner", "password123", Role::User)
        .await
        .unwrap();
    let private = add(&app, &metadata("private", &["Space opera"]), true).await;
    let shared = add(&app, &metadata("shared", &["Space opera"]), true).await;
    sqlx::query("UPDATE books SET sharing_managed = 1 WHERE id = ?")
        .bind(private)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_access(user_id,book_id,sharing,is_owner) VALUES(?,?,'private',1)",
    )
    .bind(owner.id)
    .bind(private)
    .execute(&app.state.db)
    .await
    .unwrap();
    let payload = get(&app, &cookie, "/api/home/updates").await;
    assert!(
        payload["library"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["bookId"] == shared)
    );
    assert!(!payload.to_string().contains("Imaginary private"));
}

#[tokio::test]
async fn taste_filters_noise_before_selection_and_accumulates_likes() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![
        metadata("space-suggestion", &["Space opera"]),
        metadata("fantasy-suggestion", &["Fantasy"]),
    ]));
    let app = common::test_app_with_metadata(provider).await;
    let (user, cookie) = reader(&app).await;
    for key in ["space-one", "space-two"] {
        let id = add(
            &app,
            &metadata(
                key,
                &[
                    "Accessible book",
                    "American fiction",
                    "Book",
                    "Fiction",
                    "Literature",
                    "Space opera",
                ],
            ),
            false,
        )
        .await;
        user_books::set_preference(&app.state.db, user, id, Some("liked"))
            .await
            .unwrap();
    }
    let id = add(&app, &metadata("fantasy-seed", &["Fantasy"]), false).await;
    user_books::set_preference(&app.state.db, user, id, Some("liked"))
        .await
        .unwrap();
    let payload = get(&app, &cookie, "/api/home/spotlight").await;
    assert_eq!(
        payload["items"][0]["reasonLabel"],
        "Matches your interests: Space opera"
    );
    assert!(
        !payload
            .to_string()
            .contains("Because you like Accessible book")
    );
    let author: i64 =
        sqlx::query_scalar("SELECT id FROM authors WHERE name = 'Writer fantasy-seed'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    bokhylle_server::follows::set(&app.state.db, user, author, true)
        .await
        .unwrap();
    // A follow augments taste instead of replacing the authors of liked books.
    let payload = get(&app, &cookie, "/api/home/spotlight").await;
    assert!(
        payload["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["reasonLabel"] == "Matches your interests: Space opera")
    );
}

#[tokio::test]
async fn catalogue_pool_refills_after_ownership_and_only_visible_spotlight_books_leave_the_rail() {
    let books: Vec<_> = (0..50)
        .map(|i| metadata(&format!("pool-{i}"), &["Space opera"]))
        .collect();
    let provider = Arc::new(FakeMetadataProvider::new(books.clone()));
    let app = common::test_app_with_metadata(provider.clone()).await;
    let (user, cookie) = reader(&app).await;
    sqlx::query(
        "INSERT INTO user_subject_interests(user_id,normalized_name) VALUES(?,'space opera')",
    )
    .bind(user)
    .execute(&app.state.db)
    .await
    .unwrap();
    for book in books.iter().take(30) {
        add(&app, book, true).await;
    }
    let payload = get(&app, &cookie, "/api/home/spotlight").await;
    assert_eq!(provider.last_limit(), 50);
    assert_eq!(payload["items"].as_array().unwrap().len(), 5);
    assert_eq!(payload["recommendations"].as_array().unwrap().len(), 18);
    let catalogue: Vec<_> = payload["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(payload["recommendations"].as_array().unwrap())
        .filter_map(|i| i["providerKey"].as_str())
        .collect();
    assert_eq!(
        catalogue.len(),
        20,
        "all twenty unowned candidates remain visible"
    );
    for i in 30..50 {
        assert!(catalogue.contains(&format!("pool-{i}").as_str()));
    }
    let rejected = add(&app, &books[49], false).await;
    user_books::set_preference(&app.state.db, user, rejected, Some("not_for_me"))
        .await
        .unwrap();
    let cached = get(&app, &cookie, "/api/home/spotlight?cachedOnly=true").await;
    assert!(
        !cached.to_string().contains("pool-49"),
        "saved suggestions recheck live feedback"
    );
}

#[tokio::test]
async fn hidden_topics_filter_cached_catalogue_results_and_explicit_search_seeds() {
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![
        metadata("visible", &["Space opera"]),
        metadata("hidden", &["Space opera", "Colonization"]),
    ])))
    .await;
    let (user, cookie) = reader(&app).await;
    sqlx::query(
        "INSERT INTO user_subject_interests(user_id,normalized_name) VALUES(?,'space opera')",
    )
    .bind(user)
    .execute(&app.state.db)
    .await
    .unwrap();
    get(&app, &cookie, "/api/home/spotlight").await;
    queries::set_subject_hidden(&app.state.db, user, "colonization", true)
        .await
        .unwrap();
    let cached = get(&app, &cookie, "/api/home/spotlight?cachedOnly=true").await;
    assert!(cached.to_string().contains("Imaginary visible"));
    assert!(!cached.to_string().contains("Imaginary hidden"));
    queries::set_subject_hidden(&app.state.db, user, "space opera", true)
        .await
        .unwrap();
    let payload = get(&app, &cookie, "/api/home/spotlight").await;
    assert!(payload["items"].as_array().unwrap().is_empty());
    assert!(payload["recommendations"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn spotlight_rechecks_subjects_and_languages_revealed_by_detail_metadata() {
    let hidden = metadata("detail-hidden", &["Space opera"]);
    let mut wrong_language = metadata("detail-wrong-language", &["Space opera"]);
    wrong_language.language = None;
    wrong_language.languages.clear();
    let visible = metadata("detail-visible", &["Space opera"]);
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![
        hidden.clone(),
        wrong_language.clone(),
        visible,
    ])))
    .await;
    let (user, cookie) = reader(&app).await;
    sqlx::query(
        "INSERT INTO user_subject_interests(user_id,normalized_name) VALUES(?,'space opera')",
    )
    .bind(user)
    .execute(&app.state.db)
    .await
    .unwrap();
    queries::set_subject_hidden(&app.state.db, user, "colonization", true)
        .await
        .unwrap();
    bokhylle_server::discovery::store_book(
        &app.state,
        &MetadataResult {
            subjects: vec!["Space opera".into(), "Colonization".into()],
            ..hidden
        },
    )
    .await
    .unwrap();
    bokhylle_server::discovery::store_book(
        &app.state,
        &MetadataResult {
            language: Some("ru".into()),
            languages: vec!["ru".into()],
            ..wrong_language
        },
    )
    .await
    .unwrap();
    let payload = get(&app, &cookie, "/api/home/spotlight").await;
    assert_eq!(payload["items"].as_array().unwrap().len(), 1);
    assert_eq!(payload["items"][0]["providerKey"], "detail-visible");
    assert!(payload["recommendations"].as_array().unwrap().is_empty());
    let cached = get(&app, &cookie, "/api/home/spotlight?cachedOnly=true").await;
    assert_eq!(cached["items"].as_array().unwrap().len(), 1);
    assert_eq!(cached["items"][0]["providerKey"], "detail-visible");
    assert!(cached["recommendations"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn rediscovery_varies_by_day_and_respects_feedback_and_file_languages() {
    let app = common::test_app().await;
    let (user, _) = reader(&app).await;
    let mut ids = Vec::new();
    for i in 0..40 {
        let mut book = metadata(&format!("rediscover-{i}"), &["Space opera"]);
        if i == 2 {
            book.subjects.push("Colonization".into());
        }
        if i == 3 {
            book.language = Some("ru".into());
            book.languages = vec!["ru".into()];
        }
        let id = add(&app, &book, true).await;
        user_books::add(&app.state.db, user, id, "manual")
            .await
            .unwrap();
        ids.push(id);
    }
    user_books::set_preference(&app.state.db, user, ids[1], Some("not_for_me"))
        .await
        .unwrap();
    queries::set_subject_hidden(&app.state.db, user, "colonization", true)
        .await
        .unwrap();
    let first = queries::highlight_books_visible(&app.state.db, 12, Some(user), 42, user)
        .await
        .unwrap();
    let repeated = queries::highlight_books_visible(&app.state.db, 12, Some(user), 42, user)
        .await
        .unwrap();
    let next = queries::highlight_books_visible(&app.state.db, 12, Some(user), 43, user)
        .await
        .unwrap();
    let selected =
        |books: Vec<queries::BookSummary>| books.into_iter().map(|b| b.id).collect::<Vec<_>>();
    assert_eq!(selected(first), selected(repeated));
    assert_ne!(
        selected(next.clone()),
        selected(
            queries::highlight_books_visible(&app.state.db, 12, Some(user), 42, user)
                .await
                .unwrap()
        )
    );
    let eligible = queries::highlight_books_visible(&app.state.db, 50, Some(user), 42, user)
        .await
        .unwrap();
    assert_eq!(eligible.len(), 37);
    assert!(eligible.iter().all(|b| !ids[1..4].contains(&b.id)));
}

#[tokio::test]
async fn subject_rails_check_languages_and_liked_similarity_ignores_generic_tags() {
    let app = common::test_app().await;
    let (user, _) = reader(&app).await;
    let liked = add(&app, &metadata("liked", &["Fantasy", "Fiction"]), false).await;
    user_books::set_preference(&app.state.db, user, liked, Some("liked"))
        .await
        .unwrap();
    for i in 0..6 {
        let mut book = metadata(&format!("fantasy-{i}"), &["Fantasy", "Fiction"]);
        if i >= 3 {
            book.language = Some("ru".into());
            book.languages = vec!["ru".into()];
        }
        add(&app, &book, true).await;
    }
    let rails = queries::home_rails(&app.state.db, user).await.unwrap();
    assert_eq!(rails.len(), 1);
    assert_eq!(rails[0].books.len(), 3);
    assert!(
        rails[0]
            .books
            .iter()
            .all(|b| b.language.as_deref() == Some("en"))
    );
    assert!(rails.iter().all(|r| r.title != "Based on books you liked"));
}

#[tokio::test]
async fn liked_rail_has_priority_and_overlapping_subject_rails_are_suppressed() {
    let app = common::test_app().await;
    let (user, _) = reader(&app).await;
    let seed = add(
        &app,
        &metadata("liked-pair", &["Space opera", "Colonization"]),
        false,
    )
    .await;
    user_books::set_preference(&app.state.db, user, seed, Some("liked"))
        .await
        .unwrap();
    for i in 0..4 {
        add(
            &app,
            &metadata(&format!("pair-{i}"), &["Space opera", "Colonization"]),
            true,
        )
        .await;
    }
    for subject in ["Cooking", "Travel", "Psychology", "History"] {
        sqlx::query("INSERT INTO user_subject_interests(user_id,normalized_name) VALUES(?,?)")
            .bind(user)
            .bind(subject.to_lowercase())
            .execute(&app.state.db)
            .await
            .unwrap();
        for i in 0..3 {
            add(&app, &metadata(&format!("{subject}-{i}"), &[subject]), true).await;
        }
    }
    let rails = queries::home_rails(&app.state.db, user).await.unwrap();
    assert_eq!(rails[0].title, "Based on books you liked");
    assert_eq!(rails[0].books.len(), 4);
    assert_eq!(
        rails.len(),
        4,
        "liked results retain their place beside three varied subjects"
    );
    assert!(
        rails
            .iter()
            .skip(1)
            .all(|r| !matches!(r.subject.as_deref(), Some("space opera" | "colonization")))
    );
}

#[tokio::test]
async fn library_search_promotes_an_older_exact_title_over_newer_partial_matches() {
    let app = common::test_app().await;
    let exact = add(
        &app,
        &MetadataResult {
            title: "Dune".into(),
            ..metadata("exact", &[])
        },
        true,
    )
    .await;
    let other = add(
        &app,
        &MetadataResult {
            title: "Dune Companion".into(),
            ..metadata("partial", &[])
        },
        true,
    )
    .await;
    sqlx::query("UPDATE books SET created_at = 1 WHERE id = ?")
        .bind(exact)
        .execute(&app.state.db)
        .await
        .unwrap();
    let found = queries::search_books(&app.state.db, "Dune", 10, &Default::default())
        .await
        .unwrap();
    assert_eq!(
        found.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![exact, other]
    );
}

#[tokio::test]
async fn followed_author_rail_filters_before_its_quota_and_accepts_available_or_unknown_languages()
{
    let app = common::test_app().await;
    let (user, cookie) = reader(&app).await;
    let owned = metadata("owned", &["Space opera"]);
    add(&app, &owned, true).await;
    let author: i64 = sqlx::query_scalar("SELECT id FROM authors WHERE name = 'Writer owned'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    bokhylle_server::follows::set(&app.state.db, user, author, true)
        .await
        .unwrap();
    let rejected = add(
        &app,
        &MetadataResult {
            authors: owned.authors.clone(),
            ..metadata("rejected", &[])
        },
        false,
    )
    .await;
    user_books::set_preference(&app.state.db, user, rejected, Some("not_for_me"))
        .await
        .unwrap();
    queries::set_subject_hidden(&app.state.db, user, "colonization", true)
        .await
        .unwrap();
    for (i, key, language, languages, subjects) in [
        (6, "owned", Some("en"), json!(["en"]), json!([])),
        (5, "rejected", Some("en"), json!(["en"]), json!([])),
        (
            4,
            "hidden",
            Some("en"),
            json!(["en"]),
            json!(["Colonization"]),
        ),
        (3, "wrong", Some("ru"), json!(["ru"]), json!([])),
        (
            2,
            "multilingual",
            Some("ru"),
            json!(["ru", "en"]),
            json!([]),
        ),
        (1, "unknown", None, json!([]), json!([])),
    ] {
        sqlx::query("INSERT INTO author_discoveries(author_id,provider,provider_key,title,authors,language,languages,subjects,discovered_at) VALUES(?,'fake',?,?,?,?,?,?,?)")
            .bind(author).bind(key).bind(format!("Imaginary {key}")).bind("Writer owned")
            .bind(language).bind(languages.to_string()).bind(subjects.to_string()).bind(i)
            .execute(&app.state.db).await.unwrap();
    }
    let payload = get(&app, &cookie, "/api/home/updates").await;
    let keys: Vec<_> = payload["discoveries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["providerKey"].as_str().unwrap())
        .collect();
    assert_eq!(keys, vec!["multilingual", "unknown"]);
}

async fn write(
    app: &common::TestApp,
    cookie: &str,
    method: &str,
    uri: &str,
    body: Value,
) -> StatusCode {
    app.router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn interest(app: &common::TestApp, user: i64, topic: &str) {
    sqlx::query("INSERT INTO user_subject_interests(user_id,normalized_name) VALUES(?,?)")
        .bind(user)
        .bind(topic)
        .execute(&app.state.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn complete_taste_ranks_the_entire_provider_pool_and_counts_aliases_once() {
    let mut books: Vec<_> = (0..35)
        .map(|i| metadata(&format!("weak-{i}"), &["Space opera"]))
        .collect();
    books.push(metadata(
        "best",
        &[
            "Space opera",
            "Fantasy",
            "Fantasy fiction",
            "Detective and mystery stories",
            "Humour",
        ],
    ));
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(books))).await;
    let (user, cookie) = reader(&app).await;
    for topic in ["space opera", "fantasy", "mystery", "humor"] {
        interest(&app, user, topic).await;
    }
    sqlx::query("UPDATE users SET role='admin' WHERE id=?")
        .bind(user)
        .execute(&app.state.db)
        .await
        .unwrap();
    let payload = get(&app, &cookie, "/api/recommendations?limit=72").await;
    assert_eq!(
        payload["items"][0]["providerKey"], "best",
        "a late provider result matching four interests outranks single-interest matches"
    );
    assert!(payload["items"].as_array().unwrap().len() > 24);
    let scores = get(&app, &cookie, "/api/recommendations/diagnostics").await;
    let best = scores["scores"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["title"] == "Imaginary best")
        .unwrap();
    assert_eq!(
        best["affinity"], 20,
        "two names for fantasy contribute once"
    );
    assert_eq!(best["matchingSubjects"].as_array().unwrap().len(), 4);
    assert!(scores["timingsMs"]["local_queries"].is_number());
    assert!(scores["retrieved"].as_u64().unwrap() >= 36);
    let filtered = get(
        &app,
        &cookie,
        "/api/recommendations?subject=mystery&limit=72",
    )
    .await;
    assert_eq!(filtered["items"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["items"][0]["providerKey"], "best");
}

#[tokio::test]
async fn canonical_subjects_unify_rails_taste_and_exclusions_without_inflating_likes() {
    let app = common::test_app().await;
    let (user, cookie) = reader(&app).await;
    let liked = add(
        &app,
        &metadata("seed", &["Fantasy", "Fantasy fiction"]),
        false,
    )
    .await;
    user_books::set_preference(&app.state.db, user, liked, Some("liked"))
        .await
        .unwrap();
    for (i, topics) in [
        vec!["Fantasy fiction"],
        vec!["Fantasy, English"],
        vec!["Fantasy"],
    ]
    .iter()
    .enumerate()
    {
        add(&app, &metadata(&format!("alias-{i}"), topics), true).await;
    }
    let rails = queries::home_rails(&app.state.db, user).await.unwrap();
    assert_eq!(rails.len(), 1);
    assert_eq!(rails[0].subject.as_deref(), Some("fantasy"));
    assert_eq!(rails[0].books.len(), 3);
    for topic in ["fantasy", "fantasy fiction", "Fantasy, English"] {
        let filters = queries::BookFilters {
            viewer_id: Some(user),
            subject: Some(topic.into()),
            ..Default::default()
        };
        assert_eq!(
            queries::list_books(&app.state.db, "title", 1, 24, &filters)
                .await
                .unwrap()
                .total,
            3,
            "View all for {topic}"
        );
    }
    let facets = queries::book_facets_visible(&app.state.db, None, user)
        .await
        .unwrap();
    assert_eq!(facets.subjects.len(), 1);
    assert_eq!(facets.subjects[0].normalized, "fantasy");
    assert_eq!(facets.subjects[0].count, 3);
    assert_eq!(facets.subjects[0].aliases.len(), 3);
    assert_ne!(
        rails[0].title, "Based on books you liked",
        "two aliases are not two shared topics"
    );
    sqlx::query("UPDATE users SET role='admin' WHERE id=?")
        .bind(user)
        .execute(&app.state.db)
        .await
        .unwrap();
    get(&app, &cookie, "/api/home/spotlight").await;
    let diagnostics = get(&app, &cookie, "/api/recommendations/diagnostics").await;
    let scores = diagnostics["scores"].as_array().unwrap();
    assert!(
        scores
            .iter()
            .filter(|s| s["title"].as_str().unwrap().contains("alias-"))
            .all(|s| s["affinity"] == 5)
    );
    queries::set_subject_hidden(&app.state.db, user, "fantasy fiction", true)
        .await
        .unwrap();
    assert!(
        queries::home_rails(&app.state.db, user)
            .await
            .unwrap()
            .is_empty()
    );
    queries::set_subject_hidden(&app.state.db, user, "fantasy", false)
        .await
        .unwrap();
    assert_eq!(
        queries::home_rails(&app.state.db, user)
            .await
            .unwrap()
            .len(),
        1
    );
    for topic in [
        "Fantasy, English",
        "Fantasy fiction, general",
        "Science fiction stories, in English",
        "Humour",
        "Detective and mystery stories",
        "Space opera, American, general",
    ] {
        let normalized = bokhylle_core::identity::normalize_text(topic);
        sqlx::query(
            "INSERT OR IGNORE INTO user_subject_interests(user_id,normalized_name) VALUES(?,?)",
        )
        .bind(user)
        .bind(&normalized)
        .execute(&app.state.db)
        .await
        .unwrap();
        let concept: String =
            sqlx::query_scalar("SELECT concept FROM subject_concepts WHERE normalized_name=?")
                .bind(&normalized)
                .fetch_one(&app.state.db)
                .await
                .unwrap();
        assert_eq!(
            concept,
            bokhylle_server::library::subjects::concept(&normalized),
            "{topic}"
        );
    }
}

#[tokio::test]
async fn catalogue_feedback_is_reversible_profile_scoped_and_has_no_acquisition_or_shelf_effects() {
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![metadata(
        "feedback",
        &["Space opera"],
    )])))
    .await;
    let (user, cookie) = reader(&app).await;
    interest(&app, user, "space opera").await;
    let other = app
        .state
        .auth
        .create_user("other", "password123", Role::User)
        .await
        .unwrap();
    let other_cookie = common::login(&app, "other", "password123").await;
    let payload = get(&app, &cookie, "/api/recommendations").await;
    let key = payload["items"][0]["recommendationKey"].as_str().unwrap();
    let uri = format!("/api/recommendations/{key}/feedback");
    assert_eq!(
        write(
            &app,
            &other_cookie,
            "POST",
            &uri,
            json!({"action":"not_for_me"})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        write(&app, &cookie, "POST", &uri, json!({"action":"not_for_me"})).await,
        StatusCode::OK
    );
    let rejected = get(&app, &cookie, "/api/profile/rejected").await;
    let id = rejected["items"][0]["bookId"].as_i64().unwrap();
    let shelf: (i64, String) =
        sqlx::query_as("SELECT on_shelf,preference FROM user_books WHERE user_id=? AND book_id=?")
            .bind(user)
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(shelf, (0, "not_for_me".into()));
    let acquisitions: i64 = sqlx::query_scalar("SELECT count(*) FROM acquisitions")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(acquisitions, 0);
    assert!(
        get(&app, &cookie, "/api/recommendations?cachedOnly=true").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        get(&app, &other_cookie, "/api/profile/rejected").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Another reader's restore never mutates this reader's preference.
    assert_eq!(
        write(
            &app,
            &other_cookie,
            "DELETE",
            &format!("/api/profile/rejected/{id}"),
            json!(null)
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        get(&app, &cookie, "/api/profile/rejected").await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        write(
            &app,
            &cookie,
            "DELETE",
            &format!("/api/profile/rejected/{id}"),
            json!(null)
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let restored = get(&app, &cookie, "/api/recommendations?cachedOnly=true").await;
    assert_eq!(restored["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        write(&app, &cookie, "POST", &uri, json!({"action":"like"})).await,
        StatusCode::OK
    );
    let liked: (i64, String) =
        sqlx::query_as("SELECT on_shelf,preference FROM user_books WHERE user_id=? AND book_id=?")
            .bind(user)
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(liked, (0, "liked".into()));
    let other_count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_books WHERE user_id=?")
        .bind(other.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(other_count, 0);
}

#[tokio::test]
async fn impressions_apply_a_temporary_penalty_and_dismissal_expires_without_changing_taste() {
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![
        metadata("first", &["Space opera"]),
        metadata("second", &["Space opera"]),
    ])))
    .await;
    let (user, cookie) = reader(&app).await;
    interest(&app, user, "space opera").await;
    sqlx::query("UPDATE users SET role='admin' WHERE id=?")
        .bind(user)
        .execute(&app.state.db)
        .await
        .unwrap();
    let payload = get(&app, &cookie, "/api/recommendations").await;
    let first = payload["items"][0].clone();
    let key = first["recommendationKey"].as_str().unwrap();
    let seen: Option<i64> = sqlx::query_scalar(
        "SELECT last_seen_at FROM recommendation_candidates WHERE user_id=? AND identity_key=?",
    )
    .bind(user)
    .bind(key)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(seen, None, "offered is not seen");
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            "/api/recommendations/impressions",
            json!({"keys":[key,"unknown"]})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            "/api/recommendations/impressions",
            json!({"keys":[key]})
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let next = get(&app, &cookie, "/api/recommendations?cachedOnly=true").await;
    assert_ne!(next["items"][0]["providerKey"], first["providerKey"]);
    assert_eq!(
        next["items"].as_array().unwrap().len(),
        2,
        "seen books remain eligible"
    );
    let diagnostics = get(&app, &cookie, "/api/recommendations/diagnostics").await;
    let seen = diagnostics["scores"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap();
    assert_eq!(seen["affinity"], 5);
    assert_eq!(seen["score"], 4);
    assert_eq!(seen["recentlySeen"], true);
    let feedback = format!("/api/recommendations/{key}/feedback");
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            &feedback,
            json!({"action":"dismiss"})
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(
        get(&app, &cookie, "/api/recommendations?cachedOnly=true").await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let prefs: i64 = sqlx::query_scalar("SELECT count(*) FROM user_books WHERE user_id=?")
        .bind(user)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(prefs, 0);
    sqlx::query("UPDATE recommendation_candidates SET dismissed_until=unixepoch()-1,last_seen_at=unixepoch()-8*86400 WHERE user_id=?").bind(user).execute(&app.state.db).await.unwrap();
    assert_eq!(
        get(&app, &cookie, "/api/recommendations?cachedOnly=true").await["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let diagnostics = get(&app, &cookie, "/api/recommendations/diagnostics").await;
    assert!(
        diagnostics["scores"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["recentlySeen"] == false && s["affinity"] == s["score"])
    );
    sqlx::query(
        "UPDATE recommendation_candidates SET offered_at=unixepoch()-2*86400 WHERE user_id=?",
    )
    .bind(user)
    .execute(&app.state.db)
    .await
    .unwrap();
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            &feedback,
            json!({"action":"dismiss"})
        )
        .await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn diagnostics_explain_filters_and_require_administration() {
    let mut wrong = metadata("wrong", &["Space opera"]);
    wrong.language = Some("ru".into());
    wrong.languages = vec!["ru".into()];
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![
        metadata("owned", &["Space opera"]),
        metadata("hidden", &["Space opera", "Fantasy fiction"]),
        wrong,
        metadata("good", &["Space opera"]),
    ])))
    .await;
    let (user, cookie) = reader(&app).await;
    interest(&app, user, "space opera").await;
    add(&app, &metadata("owned", &["Space opera"]), true).await;
    queries::set_subject_hidden(&app.state.db, user, "fantasy", true)
        .await
        .unwrap();
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/recommendations/diagnostics")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    sqlx::query("UPDATE users SET role='admin' WHERE id=?")
        .bind(user)
        .execute(&app.state.db)
        .await
        .unwrap();
    get(&app, &cookie, "/api/recommendations").await;
    let diagnostics = get(&app, &cookie, "/api/recommendations/diagnostics").await;
    for reason in ["language", "hidden_subject", "owned_or_feedback"] {
        assert_eq!(diagnostics["filtered"][reason], 1, "{reason}");
    }
    assert_eq!(diagnostics["catalogueCached"], false);
    get(&app, &cookie, "/api/recommendations?cachedOnly=true").await;
    assert_eq!(
        get(&app, &cookie, "/api/recommendations/diagnostics").await["catalogueCached"],
        true
    );
}

#[tokio::test]
async fn series_continuations_require_explicit_completion_and_an_unambiguous_successor() {
    let app = common::test_app().await;
    let (user, cookie) = reader(&app).await;
    let mut ids = Vec::new();
    for number in [1, 2, 4] {
        let id = add(
            &app,
            &metadata(&format!("volume-{number}"), &["Space opera"]),
            true,
        )
        .await;
        sqlx::query("UPDATE books SET series='Imaginary Cycle',series_number=? WHERE id=?")
            .bind(number.to_string())
            .bind(id)
            .execute(&app.state.db)
            .await
            .unwrap();
        ids.push(id);
    }
    assert!(
        get(&app, &cookie, "/api/home/series")
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    sqlx::query("INSERT INTO user_book_completions(user_id,book_id) VALUES(?,?)")
        .bind(user)
        .bind(ids[0])
        .execute(&app.state.db)
        .await
        .unwrap();
    let next = get(&app, &cookie, "/api/home/series").await;
    assert_eq!(next[0]["book"]["id"], ids[1]);
    assert_eq!(next[0]["readable"], true);
    user_books::set_preference(&app.state.db, user, ids[1], Some("not_for_me"))
        .await
        .unwrap();
    assert!(
        get(&app, &cookie, "/api/home/series")
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    user_books::set_preference(&app.state.db, user, ids[1], None)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_book_completions(user_id,book_id) VALUES(?,?)")
        .bind(user)
        .bind(ids[1])
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        get(&app, &cookie, "/api/home/series").await[0]["missingVolume"],
        3
    );
    sqlx::query("UPDATE books SET series_number='Special' WHERE id=?")
        .bind(ids[2])
        .execute(&app.state.db)
        .await
        .unwrap();
    assert!(
        get(&app, &cookie, "/api/home/series")
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Duplicate regular volume numbers cannot select an arbitrary book.
    sqlx::query("DELETE FROM user_book_completions WHERE book_id=?")
        .bind(ids[1])
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE books SET series_number='2',series_sort_order=2 WHERE id=?")
        .bind(ids[2])
        .execute(&app.state.db)
        .await
        .unwrap();
    assert!(
        get(&app, &cookie, "/api/home/series")
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn child_series_and_feedback_remain_shelf_scoped_and_recheck_access() {
    let app = common::test_app().await;
    let (user, cookie) = reader(&app).await;
    sqlx::query("UPDATE users SET profile_type='child' WHERE id=?")
        .bind(user)
        .execute(&app.state.db)
        .await
        .unwrap();
    let one = add(&app, &metadata("child-one", &["Fantasy"]), true).await;
    let two = add(&app, &metadata("private-successor", &["Fantasy"]), true).await;
    for (id, number) in [(one, 1), (two, 2)] {
        sqlx::query(
            "UPDATE books SET series='Child Cycle',series_number=?,series_sort_order=? WHERE id=?",
        )
        .bind(number.to_string())
        .bind(number as f64)
        .bind(id)
        .execute(&app.state.db)
        .await
        .unwrap();
    }
    user_books::add(&app.state.db, user, one, "parent_assigned")
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_book_completions(user_id,book_id) VALUES(?,?)")
        .bind(user)
        .bind(one)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert!(
        get(&app, &cookie, "/api/home/series")
            .await
            .as_array()
            .unwrap()
            .is_empty()
    );
    user_books::add(&app.state.db, user, two, "parent_assigned")
        .await
        .unwrap();
    assert_eq!(
        get(&app, &cookie, "/api/home/series").await[0]["book"]["id"],
        two
    );
    let spotlight = get(&app, &cookie, "/api/home/spotlight").await;
    let item = spotlight["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["bookId"] == two)
        .unwrap();
    let key = item["recommendationKey"].as_str().unwrap();
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            &format!("/api/recommendations/{key}/feedback"),
            json!({"action":"not_for_me"})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    user_books::remove(&app.state.db, user, two).await.unwrap();
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            "/api/recommendations/impressions",
            json!({"keys":[key]})
        )
        .await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn large_household_selection_keeps_results_bounded_and_reports_local_query_cost() {
    let app = common::test_app().await;
    let (user, cookie) = reader(&app).await;
    sqlx::query("UPDATE users SET role='admin' WHERE id=?")
        .bind(user)
        .execute(&app.state.db)
        .await
        .unwrap();
    interest(&app, user, "fantasy").await;
    sqlx::query("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<1000) INSERT INTO books(title,normalized_title,description,language) SELECT 'Fictional Library '||i,'fictional library '||i,'An imaginary reader follows a distant light across unfamiliar worlds, learning how the stories of a community can change its future and finding a place to call home.','en' FROM n").execute(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO editions(book_id,title,language) SELECT id,title,'en' FROM books")
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO book_files(edition_id,path,format,size,sha256) SELECT id,'fixture-'||id||'.epub','epub',1,'fixture-'||id FROM editions").execute(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO subjects(name,normalized_name) VALUES('Fantasy fiction','fantasy fiction'),('Fantasy','fantasy')").execute(&app.state.db).await.unwrap();
    sqlx::query("INSERT INTO book_subjects(book_id,subject_id) SELECT b.id,s.id FROM books b JOIN subjects s ON s.id=(b.id%2)+1").execute(&app.state.db).await.unwrap();
    let spotlight = get(&app, &cookie, "/api/home/spotlight?cachedOnly=true").await;
    assert_eq!(spotlight["items"].as_array().unwrap().len(), 5);
    let rails = queries::home_rails(&app.state.db, user).await.unwrap();
    assert_eq!(rails.len(), 1);
    assert_eq!(rails[0].books.len(), 12);
    let diagnostics = get(&app, &cookie, "/api/recommendations/diagnostics").await;
    eprintln!(
        "1000 fictional household books: {}",
        diagnostics["timingsMs"]
    );
    assert_eq!(diagnostics["eligible"], 40);
}

#[tokio::test]
async fn local_rails_honor_visible_impressions_and_temporary_dismissals() {
    let app = common::test_app().await;
    let (user, cookie) = reader(&app).await;
    interest(&app, user, "fantasy").await;
    let mut ids = Vec::new();
    for i in 0..4 {
        ids.push(add(&app, &metadata(&format!("local-{i}"), &["Fantasy"]), true).await);
    }
    let before = queries::home_rails(&app.state.db, user).await.unwrap();
    let first = before[0].books[0].id;
    let spotlight = get(&app, &cookie, "/api/home/spotlight").await;
    let key = spotlight["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["bookId"] == first)
        .unwrap()["recommendationKey"]
        .as_str()
        .unwrap();
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            "/api/recommendations/impressions",
            json!({"keys":[key]})
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let after = queries::home_rails(&app.state.db, user).await.unwrap();
    assert_eq!(
        after[0].books.last().unwrap().id,
        first,
        "a seen book remains eligible but falls behind equally relevant unseen books"
    );
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            &format!("/api/recommendations/{key}/feedback"),
            json!({"action":"dismiss"})
        )
        .await,
        StatusCode::OK
    );
    let after = queries::home_rails(&app.state.db, user).await.unwrap();
    assert_eq!(after[0].books.len(), 3);
    assert!(after[0].books.iter().all(|b| b.id != first));
    sqlx::query("UPDATE recommendation_candidates SET dismissed_until=unixepoch()-1,last_seen_at=unixepoch()-8*86400 WHERE user_id=?").bind(user).execute(&app.state.db).await.unwrap();
    let restored = queries::home_rails(&app.state.db, user).await.unwrap();
    assert_eq!(restored[0].books[0].id, first);
    assert_eq!(restored[0].books.len(), 4);
}

#[tokio::test]
async fn losing_access_removes_private_taste_titles_and_stale_feedback_authority() {
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![metadata(
        "public-suggestion",
        &["Fantasy"],
    )])))
    .await;
    let (user, cookie) = reader(&app).await;
    let seed = add(&app, &metadata("previously-shared", &["Fantasy"]), true).await;
    user_books::set_preference(&app.state.db, user, seed, Some("liked"))
        .await
        .unwrap();
    for i in 0..3 {
        add(
            &app,
            &metadata(&format!("accessible-fantasy-{i}"), &["Fantasy"]),
            true,
        )
        .await;
    }
    let selection = get(&app, &cookie, "/api/home/spotlight").await;
    let key = selection["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["bookId"] == seed)
        .unwrap()["recommendationKey"]
        .as_str()
        .unwrap();
    let undo = feedback_receipt(&app, &cookie, key, "like").await;
    sqlx::query("UPDATE books SET sharing_managed=1 WHERE id=?")
        .bind(seed)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        write(&app, &cookie, "DELETE", &undo, json!(null)).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        write(
            &app,
            &cookie,
            "POST",
            &format!("/api/recommendations/{key}/feedback"),
            json!({"action":"like"})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert!(
        get(&app, &cookie, "/api/profile/liked").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        get(&app, &cookie, "/api/recommendations").await["items"]
            .as_array()
            .unwrap()
            .is_empty(),
        "inaccessible book metadata does not supply fresh taste"
    );
    assert!(
        get(&app, &cookie, "/api/home/spotlight").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        queries::home_rails(&app.state.db, user)
            .await
            .unwrap()
            .is_empty(),
        "private historical signals do not qualify public local rails"
    );
    user_books::set_preference(&app.state.db, user, seed, Some("not_for_me"))
        .await
        .unwrap();
    assert!(
        get(&app, &cookie, "/api/profile/rejected").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn catalogue_author_matches_are_rechecked_after_detail_enrichment() {
    let mut book = metadata("author-detail", &[]);
    book.authors = vec!["Followed Fictional Author".into()];
    let app =
        common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![book.clone()])))
            .await;
    let (user, cookie) = reader(&app).await;
    let seed = add(
        &app,
        &MetadataResult {
            provider_key: "author-seed".into(),
            title: "Fictional seed".into(),
            ..book.clone()
        },
        false,
    )
    .await;
    let author: i64 = sqlx::query_scalar("SELECT author_id FROM book_authors WHERE book_id=?")
        .bind(seed)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    bokhylle_server::follows::set(&app.state.db, user, author, true)
        .await
        .unwrap();
    bokhylle_server::discovery::store_book(
        &app.state,
        &MetadataResult {
            authors: vec!["Different Fictional Author".into()],
            ..book
        },
    )
    .await
    .unwrap();
    let payload = get(&app, &cookie, "/api/home/spotlight").await;
    assert!(payload["items"].as_array().unwrap().is_empty());
    assert!(
        payload["recommendations"].as_array().unwrap().is_empty(),
        "detail metadata cannot preserve a false followed-author reason"
    );
}

#[tokio::test]
async fn request_rails_combine_distinct_books_across_subject_aliases() {
    let app = common::test_app().await;
    let (user, _) = reader(&app).await;
    for topic in ["space opera", "dragon riders", "time travel"] {
        interest(&app, user, topic).await;
        for i in 0..3 {
            add(&app, &metadata(&format!("{topic}-{i}"), &[topic]), true).await;
        }
    }
    for i in 0..3 {
        add(
            &app,
            &metadata(&format!("fantasy-copy-{i}"), &["Fantasy"]),
            true,
        )
        .await;
    }
    for (i, topic) in ["Fantasy", "Fantasy fiction"].iter().enumerate() {
        let id = add(&app, &metadata(&format!("requested-{i}"), &[*topic]), false).await;
        let acquisition = format!("request-fixture-{i}");
        sqlx::query("INSERT INTO acquisitions(id,book_id,user_id,status) VALUES(?,?,?,'READY')")
            .bind(&acquisition)
            .bind(id)
            .bind(user)
            .execute(&app.state.db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO acquisition_requests(acquisition_id,user_id,deliver_on_ready) VALUES(?,?,0)").bind(&acquisition).bind(user).execute(&app.state.db).await.unwrap();
    }
    let rails = queries::home_rails(&app.state.db, user).await.unwrap();
    assert_eq!(
        rails.len(),
        4,
        "three stronger specific-interest rails leave room for a requested genre"
    );
    assert_eq!(rails[3].subject.as_deref(), Some("fantasy"));
    assert!(rails[3].title.starts_with("Because you requested"));
    assert_eq!(rails[3].books.len(), 3);
}

async fn feedback_receipt(app: &common::TestApp, cookie: &str, key: &str, action: &str) -> String {
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/recommendations/{key}/feedback"))
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"action":action}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    format!(
        "/api/recommendations/{key}/feedback/{}",
        value["undoToken"].as_str().unwrap()
    )
}

#[tokio::test]
async fn feedback_undo_restores_previous_taste_and_dismissal_without_shelf_or_acquisition_effects()
{
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![metadata(
        "undo",
        &["Space opera"],
    )])))
    .await;
    let (user, cookie) = reader(&app).await;
    interest(&app, user, "space opera").await;
    let other = app
        .state
        .auth
        .create_user("other", "password123", Role::User)
        .await
        .unwrap();
    let other_cookie = common::login(&app, "other", "password123").await;
    let payload = get(&app, &cookie, "/api/recommendations").await;
    let key = payload["items"][0]["recommendationKey"].as_str().unwrap();
    let undo = feedback_receipt(&app, &cookie, key, "not_for_me").await;
    let rejected = get(&app, &cookie, "/api/profile/rejected").await;
    let id = rejected["items"][0]["bookId"].as_i64().unwrap();
    assert_eq!(
        write(&app, &other_cookie, "DELETE", &undo, json!(null)).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        write(&app, &cookie, "DELETE", &undo, json!(null)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        user_books::preference(&app.state.db, user, id)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        write(&app, &cookie, "DELETE", &undo, json!(null)).await,
        StatusCode::NOT_FOUND
    );
    // Undo a like restores an existing rejection, rather than merely clearing it.
    user_books::set_preference(&app.state.db, user, id, Some("not_for_me"))
        .await
        .unwrap();
    let undo = feedback_receipt(&app, &cookie, key, "like").await;
    assert_eq!(
        write(&app, &cookie, "DELETE", &undo, json!(null)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        user_books::preference(&app.state.db, user, id)
            .await
            .unwrap()
            .as_deref(),
        Some("not_for_me")
    );
    let previous = bokhylle_server::services::recommendations::epoch() + 3 * 86400;
    sqlx::query(
        "UPDATE recommendation_candidates SET dismissed_until=? WHERE user_id=? AND identity_key=?",
    )
    .bind(previous)
    .bind(user)
    .bind(key)
    .execute(&app.state.db)
    .await
    .unwrap();
    let undo = feedback_receipt(&app, &cookie, key, "dismiss").await;
    assert_eq!(
        write(&app, &cookie, "DELETE", &undo, json!(null)).await,
        StatusCode::NO_CONTENT
    );
    let restored: i64 = sqlx::query_scalar(
        "SELECT dismissed_until FROM recommendation_candidates WHERE user_id=? AND identity_key=?",
    )
    .bind(user)
    .bind(key)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(restored, previous);
    assert!(!user_books::contains(&app.state.db, user, id).await.unwrap());
    assert!(
        !user_books::contains(&app.state.db, other.id, id)
            .await
            .unwrap()
    );
    let acquisitions: i64 = sqlx::query_scalar("SELECT count(*) FROM acquisitions")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(acquisitions, 0);
}

#[tokio::test]
async fn feedback_undo_cannot_overwrite_newer_feedback_or_expire_into_a_valid_receipt() {
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![metadata(
        "stale-undo",
        &["Space opera"],
    )])))
    .await;
    let (user, cookie) = reader(&app).await;
    interest(&app, user, "space opera").await;
    let payload = get(&app, &cookie, "/api/recommendations").await;
    let key = payload["items"][0]["recommendationKey"].as_str().unwrap();
    let first = feedback_receipt(&app, &cookie, key, "like").await;
    let second = feedback_receipt(&app, &cookie, key, "not_for_me").await;
    let id = get(&app, &cookie, "/api/profile/rejected").await["items"][0]["bookId"]
        .as_i64()
        .unwrap();
    assert_eq!(
        write(&app, &cookie, "DELETE", &first, json!(null)).await,
        StatusCode::NOT_FOUND
    );
    user_books::set_preference(&app.state.db, user, id, None)
        .await
        .unwrap();
    assert_eq!(
        write(&app, &cookie, "DELETE", &second, json!(null)).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        user_books::preference(&app.state.db, user, id)
            .await
            .unwrap(),
        None
    );
    let expired = feedback_receipt(&app, &cookie, key, "dismiss").await;
    sqlx::query("UPDATE recommendation_feedback_undo SET expires_at=unixepoch()-1 WHERE user_id=?")
        .bind(user)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        write(&app, &cookie, "DELETE", &expired, json!(null)).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn feedback_undo_rechecks_child_assignment_after_profile_changes() {
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![metadata(
        "child-undo",
        &["Space opera"],
    )])))
    .await;
    let (user, cookie) = reader(&app).await;
    interest(&app, user, "space opera").await;
    let payload = get(&app, &cookie, "/api/recommendations").await;
    let key = payload["items"][0]["recommendationKey"].as_str().unwrap();
    let undo = feedback_receipt(&app, &cookie, key, "like").await;
    let id = get(&app, &cookie, "/api/profile/liked").await["items"][0]["bookId"]
        .as_i64()
        .unwrap();
    sqlx::query("UPDATE users SET profile_type='child',can_discover=1 WHERE id=?")
        .bind(user)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        write(&app, &cookie, "DELETE", &undo, json!(null)).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        user_books::preference(&app.state.db, user, id)
            .await
            .unwrap()
            .as_deref(),
        Some("liked")
    );
}
