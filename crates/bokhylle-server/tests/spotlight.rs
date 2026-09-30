mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, header};
use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use serde_json::json;
use tower::ServiceExt;

async fn spotlight_json(app: &common::TestApp, cookie: &str) -> serde_json::Value {
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/home/spotlight")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn household_recommendations_require_personal_affinity_and_respect_exclusions() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("adult", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    app.state
        .auth
        .create_user_with_profile(
            "child",
            "246810",
            bokhylle_server::auth::Role::User,
            "pin",
            "child",
        )
        .await
        .unwrap();
    let adult: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'adult'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let child: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'child'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let cookie = common::login(&app, "adult", "password123").await;
    let mut cooking = Vec::new();
    for index in 0..9 {
        let children = index < 6;
        let book = MetadataResult {
            provider: "fake".into(), provider_key: format!("personal-candidate-{index}"),
            title: format!("{} {index}", if children { "Little Lantern" } else { "Quiet Kitchen" }),
            authors: vec![if children { "Story Artist" } else { "Quiet Cook" }.into()],
            subjects: vec![if children { "Children's stories" } else { "Cooking" }.into()],
            description: Some("An imaginary book with a deliberately long description that comfortably exceeds the Spotlight excerpt threshold and belongs only to an isolated test library.".into()),
            language: Some("en".into()), ..Default::default()
        };
        let id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
            &app.state.db,
            &book,
        )
        .await
        .unwrap();
        let edition: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, ?, 'epub', 1, ?)")
            .bind(edition).bind(format!("fixture-{index}.epub")).bind(format!("personal-{index}")).execute(&app.state.db).await.unwrap();
        if children {
            bokhylle_server::user_books::add(&app.state.db, child, id, "parent_assigned")
                .await
                .unwrap();
            bokhylle_server::user_books::set_preference(&app.state.db, child, id, Some("liked"))
                .await
                .unwrap();
        } else {
            cooking.push(id);
        }
    }
    assert!(
        spotlight_json(&app, &cookie).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        bokhylle_server::library::queries::home_rails(&app.state.db, adult)
            .await
            .unwrap()
            .is_empty()
    );

    // An unshelved household copy is useful when it matches an explicit interest.
    sqlx::query(
        "INSERT INTO user_subject_interests (user_id, normalized_name) VALUES (?, 'cooking')",
    )
    .bind(adult)
    .execute(&app.state.db)
    .await
    .unwrap();
    let items = spotlight_json(&app, &cookie).await;
    assert_eq!(items["items"].as_array().unwrap().len(), 3);
    assert!(
        items["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| cooking.contains(&item["bookId"].as_i64().unwrap()))
    );
    let rails = bokhylle_server::library::queries::home_rails(&app.state.db, adult)
        .await
        .unwrap();
    assert_eq!(rails.len(), 1);
    assert_eq!(rails[0].books.len(), 3);

    bokhylle_server::user_books::set_preference(
        &app.state.db,
        adult,
        cooking[0],
        Some("not_for_me"),
    )
    .await
    .unwrap();
    let items = spotlight_json(&app, &cookie).await;
    assert_eq!(items["items"].as_array().unwrap().len(), 2);
    assert!(
        items["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["bookId"] != cooking[0])
    );
    bokhylle_server::library::queries::set_subject_hidden(&app.state.db, adult, "cooking", true)
        .await
        .unwrap();
    assert!(
        spotlight_json(&app, &cookie).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        bokhylle_server::library::queries::home_rails(&app.state.db, adult)
            .await
            .unwrap()
            .is_empty()
    );

    // Exclusions affect recommendations, not adult household access or search.
    let page = bokhylle_server::library::queries::list_books(
        &app.state.db,
        "recent",
        1,
        24,
        &Default::default(),
    )
    .await
    .unwrap();
    assert_eq!(page.total, 9);
    let hits = bokhylle_server::library::queries::search_books(
        &app.state.db,
        "Little Lantern",
        10,
        &Default::default(),
    )
    .await
    .unwrap();
    assert_eq!(hits.len(), 6);
}

#[tokio::test]
async fn following_an_author_and_liking_a_book_supply_household_affinity() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let reader: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'reader'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let mut ids = Vec::new();
    for index in 0..6 {
        let unrelated = index >= 3;
        let book = MetadataResult {
            provider: "fake".into(), provider_key: format!("follow-candidate-{index}"),
            title: format!("Paper Trails {index}"),
            authors: vec![if unrelated { "Another Author" } else { "Nora Vale" }.into()],
            subjects: if unrelated { vec!["Fiction".into(), "Large type books".into()] } else { vec!["Adventure".into(), "Fiction".into(), "Large type books".into()] },
            description: Some("An imaginary book with a deliberately long description that comfortably exceeds the Spotlight excerpt threshold and belongs only to an isolated test library.".into()),
            ..Default::default()
        };
        let id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
            &app.state.db,
            &book,
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) SELECT id, ?, 'epub', 1, ? FROM editions WHERE book_id = ?")
            .bind(format!("follow-{index}.epub")).bind(format!("follow-{index}")).bind(id).execute(&app.state.db).await.unwrap();
        ids.push(id);
    }
    assert!(
        spotlight_json(&app, &cookie).await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    sqlx::query("INSERT INTO author_follows (user_id, author_id) SELECT ?, id FROM authors WHERE name = 'Nora Vale'")
        .bind(reader).execute(&app.state.db).await.unwrap();
    assert_eq!(
        spotlight_json(&app, &cookie).await["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    sqlx::query("DELETE FROM author_follows WHERE user_id = ?")
        .bind(reader)
        .execute(&app.state.db)
        .await
        .unwrap();
    bokhylle_server::user_books::set_preference(&app.state.db, reader, ids[0], Some("liked"))
        .await
        .unwrap();
    assert_eq!(
        spotlight_json(&app, &cookie).await["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(
        !bokhylle_server::user_books::contains(&app.state.db, reader, ids[0])
            .await
            .unwrap()
    );
    let rails = bokhylle_server::library::queries::home_rails(&app.state.db, reader)
        .await
        .unwrap();
    assert!(!rails.is_empty());
    assert!(
        rails
            .iter()
            .flat_map(|rail| &rail.books)
            .all(|book| ids[..3].contains(&book.id)),
        "two generic tags must not qualify unrelated books for the liked-book rail"
    );
}

#[tokio::test]
async fn child_suggestions_require_discover_and_never_use_household_source() {
    let description = "A long introduction to this imaginary book with enough detail to qualify for the home spotlight. Its length is deliberately beyond the shelf threshold for this test.";
    let shelf = MetadataResult {
        provider: "fake".to_string(),
        provider_key: "child-shelf".to_string(),
        title: "Assigned Book".to_string(),
        authors: vec!["Shared Author".to_string()],
        description: Some(description.to_string()),
        ..Default::default()
    };
    let household = MetadataResult {
        provider: "fake".to_string(),
        provider_key: "household-book".to_string(),
        title: "Household Book".to_string(),
        authors: vec!["Shared Author".to_string()],
        description: Some(description.to_string()),
        cover_id: Some("household-cover".to_string()),
        ..Default::default()
    };
    let external = MetadataResult {
        provider: "fake".to_string(),
        provider_key: "outside-book".to_string(),
        title: "Outside Book".to_string(),
        authors: vec!["Shared Author".to_string()],
        description: Some(description.to_string()),
        cover_id: Some("outside-cover".to_string()),
        ..Default::default()
    };
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![
        shelf.clone(),
        household.clone(),
        external,
    ])))
    .await;
    app.state
        .auth
        .create_user_with_profile(
            "child",
            "246810",
            bokhylle_server::auth::Role::User,
            "pin",
            "child",
        )
        .await
        .unwrap();
    let child_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'child'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let shelf_id =
        bokhylle_server::library::import_metadata::upsert_book_from_metadata(&app.state.db, &shelf)
            .await
            .unwrap();
    let household_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &household,
    )
    .await
    .unwrap();
    bokhylle_server::user_books::add(&app.state.db, child_id, shelf_id, "parent_assigned")
        .await
        .unwrap();
    bokhylle_server::user_books::set_preference(&app.state.db, child_id, shelf_id, Some("liked"))
        .await
        .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(household_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, '/tmp/household-test.epub', 'epub', 1, 'household-test')")
        .bind(edition_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    let cookie = common::login(&app, "child", "246810").await;
    let disabled = spotlight_json(&app, &cookie).await;
    assert!(disabled["recommendations"].as_array().unwrap().is_empty());
    assert!(
        disabled["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["source"] == "shelf")
    );

    sqlx::query("UPDATE users SET can_discover = 1, can_request = 0 WHERE id = ?")
        .bind(child_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let enabled = spotlight_json(&app, &cookie).await;
    let items = enabled["items"].as_array().unwrap();
    let suggestions = enabled["recommendations"].as_array().unwrap();
    assert!(items.iter().all(|item| item["source"] != "household"));
    assert!(
        suggestions
            .iter()
            .all(|item| item["source"] == "discover" && item["bookId"].is_null())
    );
    assert!(
        items
            .iter()
            .chain(suggestions)
            .any(|item| item["providerKey"] == "outside-book")
    );
    assert!(
        !items
            .iter()
            .chain(suggestions)
            .any(|item| item["providerKey"] == "household-book")
    );
    assert!(
        !suggestions
            .iter()
            .any(|item| item["providerKey"] == "child-shelf")
    );
}

#[tokio::test]
async fn spotlight_uses_the_shelf_and_hides_when_sources_are_disabled() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("shelfie", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "shelfie", "password123").await;

    let metadata = MetadataResult {
        provider: "openlibrary".to_string(),
        provider_key: "/works/OLSPOTW".to_string(),
        title: "Spotlight Book".to_string(),
        authors: vec!["Some Author".to_string()],
        description: Some(
            "A description long enough to qualify as a spotlight blurb: it comfortably exceeds \
             one hundred and twenty characters so the candidate filter keeps it."
                .to_string(),
        ),
        ..Default::default()
    };
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &metadata,
    )
    .await
    .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'shelfie'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_books (user_id, book_id, source, on_shelf) VALUES (?, ?, 'manual', 1)",
    )
    .bind(user_id)
    .bind(book_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/home/spotlight")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&body).unwrap();
    let items = payload["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["reasonLabel"], "From your shelf");
    assert!(items[0]["blurb"].is_string());

    app.state
        .settings
        .set("home.spotlight_sources", &json!([]))
        .await
        .unwrap();
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/home/spotlight")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(payload["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn wizard_likes_supply_recommendations_before_a_book_is_owned() {
    let liked = MetadataResult {
        provider: "fake".to_string(),
        provider_key: "liked".to_string(),
        title: "A liked starter".to_string(),
        authors: vec!["Seed Author".to_string()],
        subjects: vec!["Science Fiction".to_string()],
        ..Default::default()
    };
    let no_blurb = MetadataResult {
        provider: "fake".to_string(),
        provider_key: "no-blurb".to_string(),
        title: "Another book".to_string(),
        authors: vec!["Seed Author".to_string()],
        cover_id: Some("cover-1".to_string()),
        ..Default::default()
    };
    let with_blurb = MetadataResult {
        provider: "fake".to_string(),
        provider_key: "with-blurb".to_string(),
        title: "A third book".to_string(),
        authors: vec!["Seed Author".to_string()],
        cover_id: Some("cover-2".to_string()),
        language: Some("ca".to_string()),
        languages: vec!["ca".to_string(), "en".to_string()],
        description: Some("An inviting book with enough context to make a useful spotlight suggestion for a new reader.".to_string()),
        ..Default::default()
    };
    let ca_only = MetadataResult {
        provider: "fake".to_string(),
        provider_key: "ca-only".to_string(),
        title: "A Catalan book".to_string(),
        authors: vec!["Seed Author".to_string()],
        language: Some("ca".to_string()),
        languages: vec!["ca".to_string()],
        cover_id: Some("cover-3".to_string()),
        description: Some(
            "A book in a language this reader has not chosen for discovery.".to_string(),
        ),
        ..Default::default()
    };
    let app = common::test_app_with_metadata(Arc::new(FakeMetadataProvider::new(vec![
        liked.clone(),
        no_blurb,
        with_blurb,
        ca_only,
    ])))
    .await;
    app.state
        .auth
        .create_user(
            "new-reader",
            "password123",
            bokhylle_server::auth::Role::User,
        )
        .await
        .unwrap();
    let cookie = common::login(&app, "new-reader", "password123").await;
    let book_id =
        bokhylle_server::library::import_metadata::upsert_book_from_metadata(&app.state.db, &liked)
            .await
            .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'new-reader'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET preferred_languages = '[\"en\"]' WHERE id = ?")
        .bind(user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    bokhylle_server::user_books::set_preference(&app.state.db, user_id, book_id, Some("liked"))
        .await
        .unwrap();

    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/home/spotlight")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = payload["items"].as_array().unwrap();
    let recommendations = payload["recommendations"].as_array().unwrap();
    assert!(items.iter().any(|item| item["providerKey"] == "with-blurb"));
    assert!(!items.iter().any(|item| item["providerKey"] == "ca-only"));
    assert!(
        recommendations
            .iter()
            .any(|item| item["providerKey"] == "no-blurb")
    );
    assert!(
        !recommendations
            .iter()
            .any(|item| item["providerKey"] == "liked")
    );
    assert!(
        !recommendations
            .iter()
            .any(|item| item["providerKey"] == "ca-only")
    );
}

#[tokio::test]
async fn spotlight_uses_catalogue_availability_or_actual_file_language() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user(
            "english-reader",
            "password123",
            bokhylle_server::auth::Role::User,
        )
        .await
        .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'english-reader'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET preferred_languages = '[\"en\"]' WHERE id = ?")
        .bind(user_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    app.state
        .settings
        .set("home.spotlight_sources", &json!(["shelf", "household"]))
        .await
        .unwrap();

    let description = "A description that is long enough for a Spotlight suggestion and contains useful detail about this book, rather than only its title and author.";
    for index in 0..6 {
        let metadata = MetadataResult {
            provider: "fake".to_string(),
            provider_key: format!("ru-only-{index}"),
            title: format!("Russian only {index}"),
            authors: vec![format!("Author {index}")],
            language: Some("ru".to_string()),
            languages: vec!["ru".to_string()],
            description: Some(description.to_string()),
            ..Default::default()
        };
        let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
            &app.state.db,
            &metadata,
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO user_books (user_id, book_id, added_at, source, on_shelf)
             VALUES (?, ?, ?, 'manual', 1)",
        )
        .bind(user_id)
        .bind(book_id)
        .bind(100 - index)
        .execute(&app.state.db)
        .await
        .unwrap();
    }

    let dune = MetadataResult {
        provider: "openlibrary".to_string(),
        provider_key: "/works/OLDUNEW".to_string(),
        title: "Dune".to_string(),
        authors: vec!["Frank Herbert".to_string()],
        language: Some("ru".to_string()),
        languages: vec!["ru".to_string(), "en".to_string()],
        year: Some(2021),
        description: Some(description.to_string()),
        ..Default::default()
    };
    let dune_id =
        bokhylle_server::library::import_metadata::upsert_book_from_metadata(&app.state.db, &dune)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO user_books (user_id, book_id, added_at, source, on_shelf)
         VALUES (?, ?, 1, 'manual', 1)",
    )
    .bind(user_id)
    .bind(dune_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    // Reproduce a shelf entry saved before the work/edition distinction:
    // an arbitrary sampled Russian edition was presented as the work.
    sqlx::query("UPDATE books SET language = 'ru' WHERE id = ?")
        .bind(dune_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE editions SET language = 'ru', publication_year = 2021,
         isbn13 = '9780000000000', is_unknown = 0 WHERE book_id = ?",
    )
    .bind(dune_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let saved = bokhylle_server::library::queries::get_book(&app.state.db, dune_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.language, None);
    assert_eq!(saved.publication_year, None);
    assert_eq!(saved.available_languages, ["en", "ru"]);
    assert!(saved.editions[0].unknown);
    assert_eq!(saved.editions[0].isbn13, None);
    let local_dune = bokhylle_server::discovery::local_books(
        &app.state,
        bokhylle_server::discovery::SearchKind::Title,
        "Dune",
        user_id,
    )
    .await
    .unwrap();
    assert_eq!(local_dune[0].language, None);
    assert_eq!(local_dune[0].year, None);
    assert_eq!(local_dune[0].languages, ["en", "ru"]);

    for (title, language, sha) in [
        ("Russian file", "ru", "ru-file"),
        ("English file", "en", "en-file"),
    ] {
        let metadata = MetadataResult {
            provider: "fake".to_string(),
            provider_key: title.to_string(),
            title: title.to_string(),
            authors: vec![title.to_string()],
            language: Some(language.to_string()),
            // Catalogue availability must not override the language of the file.
            languages: vec!["ru".to_string(), "en".to_string()],
            description: Some(description.to_string()),
            ..Default::default()
        };
        let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
            &app.state.db,
            &metadata,
        )
        .await
        .unwrap();
        let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
            .bind(book_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO book_files (edition_id, path, format, size, sha256)
             VALUES (?, ?, 'epub', 1, ?)",
        )
        .bind(edition_id)
        .bind(format!("/tmp/{sha}.epub"))
        .bind(sha)
        .execute(&app.state.db)
        .await
        .unwrap();
    }

    // These owned books need a personal signal as well as an accepted language.
    sqlx::query("INSERT INTO author_follows (user_id, author_id) SELECT ?, id FROM authors WHERE name IN ('English file', 'Russian file')")
        .bind(user_id).execute(&app.state.db).await.unwrap();

    let cookie = common::login(&app, "english-reader", "password123").await;
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/home/spotlight")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let items = payload["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "only English-available books qualify");
    let saved_dune = items.iter().find(|item| item["title"] == "Dune").unwrap();
    assert_eq!(saved_dune["source"], "shelf");
    assert_eq!(saved_dune["language"], json!(null));
    assert_eq!(saved_dune["languages"], json!(["en", "ru"]));
    assert!(items.iter().any(|item| item["title"] == "English file"));
    assert!(!items.iter().any(|item| item["title"] == "Russian file"));
}
