use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;
use bokhylle_server::library::{import_metadata, queries};

mod common;

fn book(
    provider_key: &str,
    title: &str,
    author: &str,
    series: Option<&str>,
    subjects: &[&str],
) -> MetadataResult {
    MetadataResult {
        provider: "fake".to_string(),
        provider_key: provider_key.to_string(),
        title: title.to_string(),
        authors: vec![author.to_string()],
        year: Some(2015),
        language: Some("en".to_string()),
        series: series.map(str::to_string),
        subjects: subjects.iter().map(|subject| subject.to_string()).collect(),
        ..Default::default()
    }
}

async fn add_book(state: &bokhylle_server::AppState, metadata: &MetadataResult) -> i64 {
    let book_id = import_metadata::upsert_book_from_metadata(&state.db, metadata)
        .await
        .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
        .bind(book_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    let path = format!("/tmp/subjects-test/{}.epub", metadata.provider_key);
    let digest = format!("digest-{}", metadata.provider_key);
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, 'epub', 100, ?)",
    )
    .bind(edition_id)
    .bind(&path)
    .bind(&digest)
    .execute(&state.db)
    .await
    .unwrap();
    book_id
}

async fn get_json(
    test_app: &common::TestApp,
    uri: &str,
    cookie: &str,
) -> (StatusCode, serde_json::Value) {
    let response = test_app
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
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn combined_book_filters_apply_to_list_search_and_letters() {
    let test_app = common::test_app().await;
    let user_id = test_app
        .state
        .auth
        .create_user("filter_reader", "password123", Role::User)
        .await
        .unwrap()
        .id;

    let target = add_book(
        &test_app.state,
        &book(
            "/works/OLFILTER1W",
            "Amber Atlas",
            "Zed Author",
            Some("Atlas Cycle"),
            &["Science fiction"],
        ),
    )
    .await;
    let _other = add_book(
        &test_app.state,
        &book(
            "/works/OLFILTER2W",
            "Azure Atlas",
            "Another Author",
            Some("Other Cycle"),
            &["Mystery"],
        ),
    )
    .await;
    bokhylle_server::user_books::add(&test_app.state.db, user_id, target, "manual")
        .await
        .unwrap();
    let collection = bokhylle_server::collections::create(&test_app.state.db, "Atlas shelf")
        .await
        .unwrap();
    bokhylle_server::collections::add_book(&test_app.state.db, collection.id, target)
        .await
        .unwrap();

    let filters = queries::BookFilters {
        mine: Some(user_id),
        kind: None,
        format: Some("epub".to_string()),
        language: Some("en".to_string()),
        series: Some("Atlas Cycle".to_string()),
        subject: Some("science fiction".to_string()),
        collection: Some(collection.id),
        letter: Some("a".to_string()),
        missing: Some("cover".to_string()),
    };
    let page = queries::list_books(&test_app.state.db, "title", 1, 24, &filters)
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, target);

    for (name, excluded) in [
        (
            "shelf",
            queries::BookFilters {
                mine: Some(user_id + 1),
                ..filters.clone()
            },
        ),
        (
            "format",
            queries::BookFilters {
                format: Some("pdf".to_string()),
                ..filters.clone()
            },
        ),
        (
            "language",
            queries::BookFilters {
                language: Some("de".to_string()),
                ..filters.clone()
            },
        ),
        (
            "series",
            queries::BookFilters {
                series: Some("Other Cycle".to_string()),
                ..filters.clone()
            },
        ),
        (
            "subject",
            queries::BookFilters {
                subject: Some("mystery".to_string()),
                ..filters.clone()
            },
        ),
        (
            "collection",
            queries::BookFilters {
                collection: Some(collection.id + 1),
                ..filters.clone()
            },
        ),
        (
            "letter",
            queries::BookFilters {
                letter: Some("b".to_string()),
                ..filters.clone()
            },
        ),
        (
            "missing",
            queries::BookFilters {
                missing: Some("language".to_string()),
                ..filters.clone()
            },
        ),
    ] {
        let page = queries::list_books(&test_app.state.db, "title", 1, 24, &excluded)
            .await
            .unwrap();
        assert_eq!(page.total, 0, "{name} filter");
    }

    let search = queries::search_books(&test_app.state.db, "Atlas", 50, &filters)
        .await
        .unwrap();
    assert_eq!(
        search.iter().map(|book| book.id).collect::<Vec<_>>(),
        vec![target]
    );

    let letters = queries::book_letters(&test_app.state.db, "title", &filters)
        .await
        .unwrap();
    assert_eq!(letters, vec!["a"]);

    let author_filters = queries::BookFilters {
        letter: Some("z".to_string()),
        ..filters
    };
    let by_author = queries::list_books(&test_app.state.db, "author", 1, 24, &author_filters)
        .await
        .unwrap();
    assert_eq!(by_author.total, 1);
    assert_eq!(by_author.items[0].id, target);
    let author_letters = queries::book_letters(&test_app.state.db, "author", &author_filters)
        .await
        .unwrap();
    assert_eq!(author_letters, vec!["z"]);
}

#[tokio::test]
async fn subject_rails_separate_series_author_and_similarity() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "admin", "password123").await;

    let leviathan = add_book(
        &test_app.state,
        &book(
            "/works/OL1W",
            "Leviathan Wakes",
            "James S. A. Corey",
            Some("The Expanse"),
            &[
                "Science fiction",
                "Space opera",
                "Interplanetary voyages",
                "Colonization",
            ],
        ),
    )
    .await;
    let caliban = add_book(
        &test_app.state,
        &book(
            "/works/OL2W",
            "Caliban's War",
            "James S. A. Corey",
            Some("The Expanse"),
            &["Science fiction", "Space opera"],
        ),
    )
    .await;
    let butcher = add_book(
        &test_app.state,
        &book(
            "/works/OL3W",
            "The Butcher of Anderson Station",
            "James S. A. Corey",
            None,
            &["Science fiction", "Short stories"],
        ),
    )
    .await;
    let children = add_book(
        &test_app.state,
        &book(
            "/works/OL4W",
            "Children of Time",
            "Adrian Tchaikovsky",
            None,
            &["Science fiction", "Space opera", "Colonization"],
        ),
    )
    .await;
    let red_rising = add_book(
        &test_app.state,
        &book(
            "/works/OL5W",
            "Red Rising",
            "Pierce Brown",
            None,
            &["Science fiction", "Dystopia"],
        ),
    )
    .await;
    let housemaid = add_book(
        &test_app.state,
        &book(
            "/works/OL6W",
            "The Housemaid",
            "Freida McFadden",
            None,
            &["Domestic thriller"],
        ),
    )
    .await;
    let noise = add_book(
        &test_app.state,
        &book(
            "/works/OL7W",
            "Catalog Noise",
            "Some Author",
            None,
            &["Fiction", "Large type books"],
        ),
    )
    .await;

    let related = queries::related_books(&test_app.state.db, leviathan)
        .await
        .unwrap();

    let series_ids: Vec<i64> = related.series.iter().map(|book| book.id).collect();
    assert_eq!(series_ids, vec![caliban]);

    let author_ids: Vec<i64> = related.author.iter().map(|book| book.id).collect();
    assert!(author_ids.contains(&butcher));
    assert!(!author_ids.contains(&caliban));
    assert!(!author_ids.contains(&leviathan));

    let similar_ids: Vec<i64> = related.similar.iter().map(|entry| entry.book.id).collect();
    assert_eq!(similar_ids.first(), Some(&children));
    assert!(similar_ids.contains(&red_rising));
    assert!(!similar_ids.contains(&leviathan));
    assert!(!similar_ids.contains(&caliban));
    assert!(!similar_ids.contains(&butcher));
    assert!(!similar_ids.contains(&housemaid));
    assert!(!similar_ids.contains(&noise));

    let shared = &related.similar[0].shared_subjects;
    assert!(shared.contains(&"Space opera".to_string()));
    assert!(shared.contains(&"Colonization".to_string()));

    // HTTP surface: detail exposes labeled subjects, the filter matches the
    // normalized value, and noise stays out of the facets.
    let (status, detail) = get_json(&test_app, &format!("/api/books/{leviathan}"), &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["subjects"][0]["name"], "Science fiction");
    assert_eq!(detail["subjects"][0]["normalized"], "science fiction");

    let (status, page) = get_json(&test_app, "/api/books?subject=space%20opera", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let titles: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"Leviathan Wakes"));
    assert!(titles.contains(&"Children of Time"));
    assert!(!titles.contains(&"The Housemaid"));

    // Text search respects the same filters as browsing.
    let (status, hits) = get_json(
        &test_app,
        "/api/books/search?q=Leviathan&subject=space%20opera",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hits.as_array().unwrap().len(), 1);
    let (status, misses) = get_json(
        &test_app,
        "/api/books/search?q=Leviathan&subject=domestic%20thriller",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(misses.as_array().unwrap().is_empty());

    let (status, facets) = get_json(&test_app, "/api/books/facets?scope=household", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let facet_names: Vec<&str> = facets["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|facet| facet["name"].as_str().unwrap())
        .collect();
    assert!(facet_names.contains(&"Space opera"));
    assert!(!facet_names.contains(&"Fiction"));
    assert!(!facet_names.contains(&"Large type books"));

    let (status, related_json) = get_json(
        &test_app,
        &format!("/api/books/{leviathan}/related"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(related_json["similar"][0]["book"]["id"], children);
}

#[tokio::test]
async fn metadata_enrichment_fills_subjects_by_provider_key_and_isbn() {
    let metadata = Arc::new(FakeMetadataProvider::new(vec![book(
        "/works/OL1W",
        "Leviathan Wakes",
        "James S. A. Corey",
        Some("The Expanse"),
        &["Science fiction", "Space opera"],
    )]));

    let library_dir = tempfile::tempdir().unwrap();

    let test_app = common::test_app_full(
        library_dir.path().to_path_buf(),
        metadata,
        Arc::new(FakeIndexerProvider::default()),
        Arc::new(FakeDownloadProvider::default()),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();

    let mut without_subjects = book(
        "/works/OL1W",
        "Leviathan Wakes",
        "James S. A. Corey",
        Some("The Expanse"),
        &[],
    );
    without_subjects.isbn13 = Some("9780316129084".to_string());
    let first = add_book(&test_app.state, &without_subjects).await;

    let mut isbn_only = book("/works/OL9W", "Unknown Edition", "Some Author", None, &[]);
    isbn_only.isbn13 = Some("9780000000002".to_string());
    let second = add_book(&test_app.state, &isbn_only).await;

    bokhylle_server::maintenance::run_metadata(&test_app.state, false)
        .await
        .unwrap();

    let first_detail = queries::get_book(&test_app.state.db, first)
        .await
        .unwrap()
        .unwrap();
    assert!(
        first_detail
            .subjects
            .iter()
            .any(|s| s.name == "Space opera")
    );

    let second_detail = queries::get_book(&test_app.state.db, second)
        .await
        .unwrap()
        .unwrap();
    assert!(
        second_detail
            .subjects
            .iter()
            .any(|s| s.name == "Space opera")
    );
}

#[tokio::test]
async fn home_rails_use_own_shelf_and_requests_without_household_fallback() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("emma", "password123", Role::User)
        .await
        .unwrap();
    let emma_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'emma'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    let mut next_key = 100;
    let mut books: Vec<(i64, String)> = Vec::new();
    for (subject, count) in [
        ("Space opera", 4),
        ("Dystopia", 4),
        ("Fantasy", 4),
        ("Colonization", 3),
    ] {
        for index in 0..count {
            let key = format!("/works/OL{next_key}W");
            next_key += 1;
            let id = add_book(
                &test_app.state,
                &book(
                    &key,
                    &format!("{subject} Book {index}"),
                    &format!("Author {subject}"),
                    None,
                    &[subject],
                ),
            )
            .await;
            books.push((id, subject.to_string()));
        }
    }

    for (id, _) in books
        .iter()
        .filter(|(_, subject)| subject == "Colonization")
    {
        let acquisition_id = format!("acq-{id}");
        sqlx::query(
            "INSERT INTO acquisitions (id, book_id, user_id, status) VALUES (?, ?, ?, 'READY')",
        )
        .bind(&acquisition_id)
        .bind(id)
        .bind(emma_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO acquisition_requests (acquisition_id, user_id, deliver_on_ready)
             VALUES (?, ?, 0)",
        )
        .bind(&acquisition_id)
        .bind(emma_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    }

    let admin_cookie = common::login(&test_app, "admin", "password123").await;
    let (status, admin_rails) = get_json(&test_app, "/api/home/rails", &admin_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(admin_rails.as_array().unwrap().is_empty());
    let admin_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'admin'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let space_opera = books
        .iter()
        .find(|(_, subject)| subject == "Space opera")
        .unwrap()
        .0;
    bokhylle_server::user_books::add(&test_app.state.db, admin_id, space_opera, "manual")
        .await
        .unwrap();
    let (_, admin_rails) = get_json(&test_app, "/api/home/rails", &admin_cookie).await;
    let admin_titles: Vec<&str> = admin_rails
        .as_array()
        .unwrap()
        .iter()
        .map(|rail| rail["title"].as_str().unwrap())
        .collect();
    assert!(admin_titles.contains(&"Space opera"));
    assert_eq!(admin_titles.len(), 1);
    assert!(
        !admin_titles
            .iter()
            .any(|title| title.starts_with("Because you"))
    );
    assert!(admin_rails[0]["subject"].is_string());

    let emma_cookie = common::login(&test_app, "emma", "password123").await;
    let (status, emma_rails) = get_json(&test_app, "/api/home/rails", &emma_cookie).await;
    assert_eq!(status, StatusCode::OK);
    let emma_titles: Vec<&str> = emma_rails
        .as_array()
        .unwrap()
        .iter()
        .map(|rail| rail["title"].as_str().unwrap())
        .collect();
    // Emma's own requests qualify Colonization; unrelated household subjects
    // and the administrator's shelf do not supply her interests.
    assert_eq!(emma_titles[0], "Colonization");
    assert_eq!(emma_titles.len(), 1);
    assert!(
        !emma_titles
            .iter()
            .any(|title| title.starts_with("Because you"))
    );
}

async fn put_json(
    test_app: &common::TestApp,
    uri: &str,
    cookie: &str,
    body: serde_json::Value,
) -> StatusCode {
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(uri)
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    response.status()
}

async fn create_user(test_app: &common::TestApp, username: &str, role: Role) -> i64 {
    test_app
        .state
        .auth
        .create_user(username, "password123", role)
        .await
        .unwrap();
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap()
}

async fn request_books(
    test_app: &common::TestApp,
    user_id: i64,
    books: &[(i64, String)],
    subject: &str,
) {
    for (id, book_subject) in books {
        if book_subject != subject {
            continue;
        }
        let acquisition_id = format!("acq-{user_id}-{id}");
        sqlx::query(
            "INSERT INTO acquisitions (id, book_id, user_id, status) VALUES (?, ?, ?, 'READY')",
        )
        .bind(&acquisition_id)
        .bind(id)
        .bind(user_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO acquisition_requests (acquisition_id, user_id, deliver_on_ready)
             VALUES (?, ?, 0)",
        )
        .bind(&acquisition_id)
        .bind(user_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn home_rails_rank_by_personal_signals_and_respect_hidden_subjects() {
    let test_app = common::test_app().await;
    let alex = create_user(&test_app, "alex", Role::User).await;
    let emma = create_user(&test_app, "emma", Role::User).await;

    let mut next_key = 300;
    let mut shelf: Vec<(i64, String)> = Vec::new();
    for subject in ["Space opera", "Domestic thriller", "Fantasy"] {
        for index in 0..4 {
            let key = format!("/works/OL{next_key}W");
            next_key += 1;
            let id = add_book(
                &test_app.state,
                &book(
                    &key,
                    &format!("{subject} {index}"),
                    &format!("Author {subject}"),
                    None,
                    &[subject],
                ),
            )
            .await;
            shelf.push((id, subject.to_string()));
        }
    }

    request_books(&test_app, alex, &shelf, "Space opera").await;
    request_books(&test_app, emma, &shelf, "Domestic thriller").await;

    let alex_cookie = common::login(&test_app, "alex", "password123").await;
    let emma_cookie = common::login(&test_app, "emma", "password123").await;

    let (status, alex_rails) = get_json(&test_app, "/api/home/rails", &alex_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(alex_rails[0]["subject"], "space opera");

    let (status, emma_rails) = get_json(&test_app, "/api/home/rails", &emma_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(emma_rails[0]["subject"], "domestic thriller");
    assert_eq!(emma_rails.as_array().unwrap().len(), 1);
    sqlx::query(
        "INSERT INTO user_subject_interests (user_id, normalized_name) VALUES (?, 'space opera')",
    )
    .bind(emma)
    .execute(&test_app.state.db)
    .await
    .unwrap();
    let (_, emma_rails) = get_json(&test_app, "/api/home/rails", &emma_cookie).await;
    assert_eq!(emma_rails.as_array().unwrap().len(), 2);

    // Hiding is per user.
    let status = put_json(
        &test_app,
        "/api/home/subjects/space%20opera",
        &emma_cookie,
        serde_json::json!({ "hidden": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, emma_rails) = get_json(&test_app, "/api/home/rails", &emma_cookie).await;
    let emma_subjects: Vec<&str> = emma_rails
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|rail| rail["subject"].as_str())
        .collect();
    assert!(!emma_subjects.contains(&"space opera"));

    let (_, alex_rails) = get_json(&test_app, "/api/home/rails", &alex_cookie).await;
    assert_eq!(alex_rails[0]["subject"], "space opera");

    let (_, hidden) = get_json(&test_app, "/api/home/subjects", &emma_cookie).await;
    assert_eq!(hidden["hidden"], serde_json::json!(["space opera"]));

    // Restoring brings the rail back.
    let status = put_json(
        &test_app,
        "/api/home/subjects/space%20opera",
        &emma_cookie,
        serde_json::json!({ "hidden": false }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, emma_rails) = get_json(&test_app, "/api/home/rails", &emma_cookie).await;
    let emma_subjects: Vec<&str> = emma_rails
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|rail| rail["subject"].as_str())
        .collect();
    assert!(emma_subjects.contains(&"space opera"));

    // Unknown subjects are rejected.
    let status = put_json(
        &test_app,
        "/api/home/subjects/nonexistent",
        &emma_cookie,
        serde_json::json!({ "hidden": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
