use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;

mod common;

fn metadata_result(
    provider_key: &str,
    title: &str,
    authors: &[&str],
    isbn13: Option<&str>,
) -> MetadataResult {
    MetadataResult {
        provider: "fake".to_string(),
        provider_key: provider_key.to_string(),
        title: title.to_string(),
        authors: authors.iter().map(|author| author.to_string()).collect(),
        isbn13: isbn13.map(str::to_string),
        cover_id: Some("123".to_string()),
        ..Default::default()
    }
}

async fn post_json(
    test_app: &common::TestApp,
    uri: &str,
    cookie: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, cookie)
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn get_json(test_app: &common::TestApp, uri: &str, cookie: &str) -> (StatusCode, Value) {
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
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn trigger_scan(test_app: &common::TestApp, cookie: &str) {
    let status = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/library/scan")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::ACCEPTED);

    for _ in 0..200 {
        let (_, status) = get_json(test_app, "/api/library/scan/status", cookie).await;
        if status["running"] == false && status["summary"].is_object() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("scan did not finish");
}

#[tokio::test]
async fn search_reports_library_status_and_deduplicates() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let provider = Arc::new(FakeMetadataProvider::new(vec![
        metadata_result(
            "/works/OL1W",
            "Project Hail Mary",
            &["Andy Weir"],
            Some("9780593135204"),
        ),
        metadata_result(
            "/works/OL2W",
            "Project Hail Mary",
            &["Andy Weir"],
            Some("9780593135204"),
        ),
        metadata_result("/works/OL3W", "Dune", &["Frank Herbert"], None),
    ]));

    let test_app = common::test_app_with_library_and_metadata(
        library_dir.path().to_path_buf(),
        provider.clone(),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;
    trigger_scan(&test_app, &cookie).await;

    let (status, results) =
        get_json(&test_app, "/api/discover/search?q=project+hail", &cookie).await;
    assert_eq!(status, StatusCode::OK);

    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 2, "duplicate library match should collapse");
    assert_eq!(results[0]["title"], "Project Hail Mary");
    assert_eq!(results[0]["status"], "IN_LIBRARY");
    assert_eq!(results[1]["title"], "Dune");
    assert_eq!(results[1]["status"], "NOT_IN_LIBRARY");
    assert_eq!(results[0]["coverId"], "123");
}

#[tokio::test]
async fn search_caches_results_and_serves_stale_on_failure() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![metadata_result(
        "/works/OL1W",
        "Project Hail Mary",
        &["Andy Weir"],
        Some("9780593135204"),
    )]));
    let test_app = common::test_app_with_metadata(provider.clone()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, first) = get_json(&test_app, "/api/discover/search?q=hail+mary", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first.as_array().unwrap().len(), 1);
    assert_eq!(provider.calls(), 1);

    let (_, second) = get_json(&test_app, "/api/discover/search?q=hail+mary", &cookie).await;
    assert_eq!(second.as_array().unwrap().len(), 1);
    assert_eq!(provider.calls(), 1, "second search should hit the cache");

    sqlx::query("UPDATE metadata_cache SET expires_at = 0")
        .execute(&test_app.state.db)
        .await
        .unwrap();
    provider.set_failing(true);

    let (status, stale) = get_json(&test_app, "/api/discover/search?q=hail+mary", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stale.as_array().unwrap().len(), 1);
    assert_eq!(provider.calls(), 2, "expired cache forces a provider call");

    sqlx::query("DELETE FROM metadata_cache")
        .execute(&test_app.state.db)
        .await
        .unwrap();

    let (status, body) = get_json(&test_app, "/api/discover/search?q=hail+mary", &cookie).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "service_unavailable");
}

#[tokio::test]
async fn search_validates_input() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, _) = get_json(&test_app, "/api/discover/search?q=", &cookie).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = get_json(&test_app, "/api/discover/search?q=dune&type=bogus", &cookie).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn covers_are_cached_on_disk() {
    let provider = Arc::new(FakeMetadataProvider::default());
    let mut cover = vec![0xFF; 4096];
    cover[..4].copy_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]);
    provider.set_cover(cover);

    let test_app = common::test_app_with_metadata(provider.clone()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/discover/cover/123?title=Project+Hail+Mary")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );
    let first = to_bytes(response.into_body(), usize::MAX).await.unwrap();

    let covers_dir = test_app
        .state
        .paths
        .config_dir
        .join("cache/provider-covers");
    let cached = std::fs::read_dir(&covers_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("cover-") && !name.ends_with(".missing"))
        });
    assert!(
        cached,
        "the cover is cached under a provider-keyed file name"
    );

    provider.set_cover(vec![0x00, 0x01, 0x02, 0x03]);
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/discover/cover/123?title=Project+Hail+Mary")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let second = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(first, second, "disk cache should serve the original bytes");

    // Provider covers are not numeric-only: Google Books identifiers are
    // full thumbnail URLs, so the proxy must accept them and fall back to
    // the placeholder when the provider cannot fetch one.
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/discover/cover/https%3A%2F%2Fexample.test%2Fcover.jpg")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn missing_cover_is_cached_but_provider_errors_are_retryable() {
    let provider = Arc::new(FakeMetadataProvider::default());
    let app = common::test_app_with_metadata(provider.clone()).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let cover = |id: &'static str| {
        Request::builder()
            .uri(format!("/api/discover/cover/{id}?title=Missing"))
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap()
    };

    for _ in 0..2 {
        let response = app.router.clone().oneshot(cover("missing")).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
    }
    assert_eq!(provider.calls(), 1, "confirmed absence is cached");

    provider.set_failing(true);
    for _ in 0..2 {
        let response = app
            .router
            .clone()
            .oneshot(cover("unavailable"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(provider.calls(), 3, "provider errors must remain retryable");

    provider.set_failing(false);
    provider.set_cover(vec![0; 16]);
    for _ in 0..2 {
        let response = app.router.clone().oneshot(cover("stub")).await.unwrap();
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
    }
    assert_eq!(
        provider.calls(),
        4,
        "a tiny stub is a cached miss, not a cover"
    );
}

#[tokio::test]
async fn cover_cache_uses_the_whole_provider_identifier() {
    let provider = Arc::new(FakeMetadataProvider::default());
    let app = common::test_app_with_metadata(provider.clone()).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let suffix = "x".repeat(60);
    let first_id = format!("first{suffix}");
    let second_id = format!("second{suffix}");
    provider.set_cover(vec![1; 1024]);
    let first = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/discover/cover/{first_id}"))
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    provider.set_cover(vec![2; 1024]);
    let second = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/discover/cover/{second_id}"))
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(second.into_body(), usize::MAX).await.unwrap()[0],
        2
    );
    assert_eq!(
        provider.calls(),
        2,
        "distinct ids must have distinct cache files"
    );
}

#[tokio::test]
async fn liking_a_discovered_book_stores_taste_without_owning_it() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![metadata_result(
        "/works/OLTASTEW",
        "Project Hail Mary",
        &["Andy Weir"],
        Some("9780593135204"),
    )]));
    let test_app = common::test_app_with_metadata(provider.clone()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, body) = post_json(
        &test_app,
        "/api/discover/like",
        &cookie,
        json!({ "providerKey": "/works/OLTASTEW" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["preference"], "liked");
    let book_id = body["bookId"].as_i64().unwrap();

    let files: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ?",
    )
    .bind(book_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(files, 0, "liking a discovered book must not create files");

    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'reader'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let (on_shelf, preference): (i64, Option<String>) = sqlx::query_as(
        "SELECT on_shelf, preference FROM user_books WHERE user_id = ? AND book_id = ?",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(on_shelf, 0, "liking must not add the book to the shelf");
    assert_eq!(preference.as_deref(), Some("liked"));

    let (status, liked) = get_json(&test_app, "/api/profile/liked", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let items = liked["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        1,
        "the liked list must expose metadata-only books"
    );
    assert_eq!(items[0]["bookId"], book_id);
    assert_eq!(items[0]["title"], "Project Hail Mary");
    assert_eq!(items[0]["readable"], false);
    assert_eq!(items[0]["onShelf"], false);
}

#[tokio::test]
async fn title_search_falls_back_to_the_core_title() {
    let provider = Arc::new(
        FakeMetadataProvider::new(vec![metadata_result(
            "/works/OLEVENTW",
            "Everything's Eventual. 14 Dark Tales",
            &["Stephen King"],
            None,
        )])
        .with_query_filter(),
    );
    let test_app = common::test_app_with_metadata(provider.clone()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, results) = get_json(
        &test_app,
        "/api/discover/search?q=Everything%27s+Eventual%3A+14+Dark+Tales&type=title",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = results.as_array().unwrap();
    assert_eq!(items.len(), 1, "the core-title fallback must find the book");
    assert_eq!(items[0]["title"], "Everything's Eventual. 14 Dark Tales");
    assert_eq!(provider.calls(), 2, "the full title is tried first");
}

#[tokio::test]
async fn provider_author_hits_stay_transient_until_followed() {
    use bokhylle_metadata::AuthorCandidate;

    let provider = Arc::new(FakeMetadataProvider::new(vec![]));
    provider.set_author_candidates(vec![AuthorCandidate {
        name: "Transient Author".to_string(),
        provider: "openlibrary".to_string(),
        provider_key: "OLTRANSIENTA".to_string(),
    }]);
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    // The local fast path answers without the provider and knows nothing yet.
    let (status, body) = get_json(
        &test_app,
        "/api/discover/authors?q=transient&source=local",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["local"].as_array().unwrap().is_empty());
    assert!(body["external"].as_array().unwrap().is_empty());

    let (status, body) = get_json(&test_app, "/api/discover/authors?q=transient", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["local"].as_array().unwrap().is_empty());
    let hit = body["external"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Transient Author")
        .expect("provider author hit");
    assert!(
        hit["authorId"].is_null(),
        "a search hit is not durable: {hit}"
    );
    assert_eq!(hit["providerKey"], "OLTRANSIENTA");

    let durable: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM authors WHERE normalized_name = 'transient author'",
    )
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(durable, 0, "searching must not create a durable author");

    let (status, created) = post_json(
        &test_app,
        "/api/discover/authors/follow",
        &cookie,
        json!({
            "name": "Transient Author",
            "provider": "openlibrary",
            "providerKey": "OLTRANSIENTA",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let author_id = created["authorId"].as_i64().expect("promoted author id");

    let (olid, followed): (Option<String>, i64) = sqlx::query_as(
        "SELECT a.olid,
                (SELECT count(*) FROM author_follows f
                 WHERE f.author_id = a.id AND f.user_id = ?)
         FROM authors a WHERE a.id = ?",
    )
    .bind(1_i64)
    .bind(author_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(olid.as_deref(), Some("OLTRANSIENTA"));
    assert_eq!(followed, 1, "following promotes and follows in one step");

    let (status, body) = get_json(
        &test_app,
        "/api/discover/authors?q=transient&source=local",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let hit = body["local"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Transient Author")
        .expect("promoted author is now local");
    assert_eq!(hit["authorId"], author_id);
    assert_eq!(hit["following"], true);
    assert!(body["external"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn author_search_promotes_exact_name_and_retries_provider_failure() {
    use bokhylle_metadata::AuthorCandidate;

    let provider = Arc::new(FakeMetadataProvider::new(vec![]).named("openlibrary"));
    provider.set_failing(true);
    let app = common::test_app_with_metadata(provider.clone()).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let path = "/api/discover/authors?q=ursula%20k%20le%20guin";
    let (_, failed) = get_json(&app, path, &cookie).await;
    assert!(failed["external"].as_array().unwrap().is_empty());

    provider.set_failing(false);
    provider.set_author_candidates(vec![
        AuthorCandidate {
            name: "Ursula K Le Guin and Friends".to_string(),
            provider: "openlibrary".to_string(),
            provider_key: "OL1A".to_string(),
        },
        AuthorCandidate {
            name: "Ursula K Le Guin".to_string(),
            provider: "openlibrary".to_string(),
            provider_key: "OL2A".to_string(),
        },
    ]);
    let (status, recovered) = get_json(&app, path, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recovered["external"][0]["name"], "Ursula K Le Guin");
    assert_eq!(recovered["external"][0]["providerKey"], "OL2A");
}

#[tokio::test]
async fn discover_books_prepend_local_catalogue_matches() {
    // The provider page does not contain the owned book, so finding it proves
    // the local prepend (not provider matching).
    let provider = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "/works/OLOTHERW".to_string(),
        title: "Unrelated Provider Book".to_string(),
        authors: vec!["Other Author".to_string()],
        language: Some("en".to_string()),
        ..Default::default()
    }]));
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLLOCAL1W".to_string(),
            title: "Local Match Book".to_string(),
            authors: vec!["Local Match Author".to_string()],
            language: Some("en".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, '/tmp/local-match.epub', 'epub', 10, 'local-match-digest')",
    )
    .bind(edition_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    let (status, body) = get_json(
        &test_app,
        "/api/discover/search/page?q=local+match&type=any",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    let first = items
        .iter()
        .find(|item| item["ownedBookId"] == book_id)
        .expect("the local match must be present");
    assert_eq!(items[0]["ownedBookId"], book_id, "local matches come first");
    assert_eq!(first["status"], "IN_LIBRARY");
    assert_eq!(
        items
            .iter()
            .filter(|item| item["ownedBookId"] == book_id)
            .count(),
        1,
        "the provider duplicate is deduplicated"
    );
}

#[tokio::test]
async fn provider_identity_matches_without_edition_columns() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "/works/OLID1W".to_string(),
        title: "Identity Match Book (Anniversary Edition)".to_string(),
        authors: vec!["Identity Author".to_string()],
        language: Some("en".to_string()),
        ..Default::default()
    }]));
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLID1W".to_string(),
            title: "Identity Match Book".to_string(),
            authors: vec!["Identity Author".to_string()],
            language: Some("en".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, '/tmp/identity-match.epub', 'epub', 10, 'identity-match-digest')",
    )
    .bind(edition_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    let linked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM book_external_ids
         WHERE book_id = ? AND provider = 'fake' AND provider_key = '/works/OLID1W'",
    )
    .bind(book_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(linked, 1, "upserting a book links its provider identity");

    // Only the identity table can match once the edition columns are gone and
    // the provider title carries a subtitle.
    sqlx::query("UPDATE editions SET provider = NULL, provider_key = NULL WHERE book_id = ?")
        .bind(book_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let (status, detail) = get_json(
        &test_app,
        "/api/discover/book?providerKey=%2Fworks%2FOLID1W",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["ownedBookId"], book_id);
    assert_eq!(detail["status"], "IN_LIBRARY");
}

#[tokio::test]
async fn following_a_non_openlibrary_author_records_its_identity() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, created) = post_json(
        &test_app,
        "/api/discover/authors/follow",
        &cookie,
        json!({
            "name": "Google Author",
            "provider": "google_books",
            "providerKey": "abc123",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let author_id = created["authorId"].as_i64().unwrap();

    let (provider, key): (String, String) = sqlx::query_as(
        "SELECT provider, provider_key FROM author_external_ids WHERE author_id = ?",
    )
    .bind(author_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(provider, "google_books");
    assert_eq!(key, "abc123");
    let olid: Option<String> = sqlx::query_scalar("SELECT olid FROM authors WHERE id = ?")
        .bind(author_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert!(
        olid.is_none(),
        "only Open Library keys set the legacy column"
    );

    let found = bokhylle_server::external_ids::author_by_provider(
        &test_app.state.db,
        "google_books",
        "abc123",
    )
    .await
    .unwrap();
    assert_eq!(found, Some(author_id));
}

#[tokio::test]
async fn author_olid_reads_the_identity_table_with_legacy_fallback() {
    let test_app = common::test_app().await;
    let legacy_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name, olid)
         VALUES ('Legacy Author', 'legacy author', 'OLLEGACYA') RETURNING id",
    )
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    let modern_id: i64 =
        sqlx::query_scalar("INSERT INTO authors (name, normalized_name) VALUES ('Modern Author', 'modern author') RETURNING id")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    bokhylle_server::external_ids::link_author(
        &test_app.state.db,
        modern_id,
        "openlibrary",
        "OLMODERNA",
    )
    .await
    .unwrap();

    assert_eq!(
        bokhylle_server::external_ids::author_olid(&test_app.state.db, legacy_id)
            .await
            .unwrap()
            .as_deref(),
        Some("OLLEGACYA")
    );
    assert_eq!(
        bokhylle_server::external_ids::author_olid(&test_app.state.db, modern_id)
            .await
            .unwrap()
            .as_deref(),
        Some("OLMODERNA")
    );
}

#[tokio::test]
async fn primary_provider_link_prefers_the_durable_identity() {
    let test_app = common::test_app().await;
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "google_books".to_string(),
            provider_key: "google-1".to_string(),
            title: "Durable Link Book".to_string(),
            authors: vec!["Link Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    bokhylle_server::external_ids::link_book(
        &test_app.state.db,
        book_id,
        "openlibrary",
        "/works/OLPREFERW",
    )
    .await
    .unwrap();

    let preferred =
        bokhylle_server::external_ids::book_provider(&test_app.state.db, book_id, &["openlibrary"])
            .await
            .unwrap();
    assert_eq!(
        preferred,
        Some(("openlibrary".to_string(), "/works/OLPREFERW".to_string())),
        "a durable provider wins even when linked later"
    );

    let oldest = bokhylle_server::external_ids::book_provider(&test_app.state.db, book_id, &[])
        .await
        .unwrap();
    assert_eq!(
        oldest,
        Some(("google_books".to_string(), "google-1".to_string())),
        "without a durable provider the oldest link is used"
    );
}

#[tokio::test]
async fn unified_discovery_ranks_local_and_applies_intent() {
    use bokhylle_metadata::AuthorCandidate;

    let provider = Arc::new(FakeMetadataProvider::new(vec![
        MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLHP1".to_string(),
            title: "Harry Potter and the Philosopher's Stone".to_string(),
            authors: vec!["J. K. Rowling".to_string()],
            year: Some(1997),
            language: Some("en".to_string()),
            ..Default::default()
        },
        MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLHP2".to_string(),
            title: "Harry Potter and the Chamber of Secrets".to_string(),
            authors: vec!["J. K. Rowling".to_string()],
            year: Some(1998),
            language: Some("en".to_string()),
            ..Default::default()
        },
    ]));
    // The provider author search only knows the namesake; the real author of
    // the matching books must be derived from the book results.
    provider.set_author_candidates(vec![AuthorCandidate {
        name: "Harry Potter".to_string(),
        provider: "openlibrary".to_string(),
        provider_key: "OLHARRYA".to_string(),
    }]);
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    // Anywhere: the provider-only namesake is suppressed, the author of the
    // matching books survives.
    let (status, body) =
        get_json(&test_app, "/api/discover?q=harry+potter&type=any", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["books"].as_array().unwrap().len(), 2);
    let external = body["authors"]["external"].as_array().unwrap();
    assert!(
        external
            .iter()
            .all(|author| author["name"] != "Harry Potter"),
        "the namesake must be suppressed: {body}"
    );
    assert!(
        external
            .iter()
            .any(|author| author["name"] == "J. K. Rowling"),
        "the matching books' author stays relevant: {body}"
    );

    // An explicit author search keeps provider results.
    let (status, body) = get_json(
        &test_app,
        "/api/discover?q=harry+potter&type=author",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["authors"]["external"]
            .as_array()
            .unwrap()
            .iter()
            .any(|author| author["name"] == "Harry Potter"),
        "explicit author search is not suppressed: {body}"
    );

    // The local fast path never waits on the provider.
    let (status, body) = get_json(
        &test_app,
        "/api/discover?q=harry+potter&type=any&source=local",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["local"], true);
    assert!(body["books"].as_array().unwrap().is_empty());
    assert!(body["authors"]["local"].as_array().unwrap().is_empty());
    assert!(body["authors"]["external"].as_array().unwrap().is_empty());
    assert!(body["provider"].is_null());
}

#[tokio::test]
async fn local_title_and_author_searches_are_column_scoped() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    for (key, title, author) in [
        ("/works/OLSCOPE1W", "Needle Title", "Other Author"),
        ("/works/OLSCOPE2W", "Other Title", "Needle Author"),
    ] {
        test_app.state.auth.count_users().await.unwrap();
        bokhylle_server::library::import_metadata::upsert_book_from_metadata(
            &test_app.state.db,
            &MetadataResult {
                provider: "fake".to_string(),
                provider_key: key.to_string(),
                title: title.to_string(),
                authors: vec![author.to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let book_titles = |body: &Value| -> Vec<String> {
        body["books"]
            .as_array()
            .unwrap()
            .iter()
            .map(|book| book["title"].as_str().unwrap().to_string())
            .collect()
    };

    let (_, body) = get_json(
        &test_app,
        "/api/discover?q=needle+title&type=title&source=local",
        &cookie,
    )
    .await;
    assert_eq!(book_titles(&body), vec!["Needle Title"]);

    let (_, body) = get_json(
        &test_app,
        "/api/discover?q=needle+author&type=author&source=local",
        &cookie,
    )
    .await;
    assert_eq!(book_titles(&body), vec!["Other Title"]);

    // A title query must not match through the author column, and vice versa.
    let (_, body) = get_json(
        &test_app,
        "/api/discover?q=needle+author&type=title&source=local",
        &cookie,
    )
    .await;
    assert!(book_titles(&body).is_empty(), "{body}");
    let (_, body) = get_json(
        &test_app,
        "/api/discover?q=needle+title&type=author&source=local",
        &cookie,
    )
    .await;
    assert!(book_titles(&body).is_empty(), "{body}");
}

#[tokio::test]
async fn ensure_materializes_an_author_without_following() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, created) = post_json(
        &test_app,
        "/api/discover/authors/ensure",
        &cookie,
        json!({
            "name": "Material Author",
            "provider": "openlibrary",
            "providerKey": "OLMATERIAA",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let author_id = created["authorId"].as_i64().unwrap();

    let followed: i64 = sqlx::query_scalar("SELECT count(*) FROM author_follows WHERE user_id = 1")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(followed, 0, "opening an author must not follow them");
    let linked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM author_external_ids
         WHERE author_id = ? AND provider = 'openlibrary' AND provider_key = 'OLMATERIAA'",
    )
    .bind(author_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(linked, 1);

    let (status, followed) = post_json(
        &test_app,
        "/api/discover/authors/follow",
        &cookie,
        json!({
            "name": "Material Author",
            "provider": "openlibrary",
            "providerKey": "OLMATERIAA",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(followed["authorId"], author_id, "follow reuses the entity");
    let followed: i64 = sqlx::query_scalar("SELECT count(*) FROM author_follows WHERE user_id = 1")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(followed, 1);
}

#[tokio::test]
async fn dominant_author_books_lead_the_results() {
    let novel = |key: &str, title: &str, number: &str| MetadataResult {
        provider: "fake".to_string(),
        provider_key: key.to_string(),
        title: title.to_string(),
        authors: vec!["J. K. Rowling".to_string()],
        series: Some("Harry Potter".to_string()),
        series_number: Some(number.to_string()),
        language: Some("en".to_string()),
        ..Default::default()
    };
    let provider = Arc::new(FakeMetadataProvider::new(vec![
        novel(
            "/works/OLHP3",
            "Harry Potter and the Prisoner of Azkaban",
            "3",
        ),
        MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLHPGUIDE".to_string(),
            title: "Harry Potter: An Unofficial Guide".to_string(),
            authors: vec!["Guide Author".to_string()],
            language: Some("en".to_string()),
            ..Default::default()
        },
        novel(
            "/works/OLHP1",
            "Harry Potter and the Philosopher's Stone",
            "1",
        ),
        novel(
            "/works/OLHP2",
            "Harry Potter and the Chamber of Secrets",
            "2",
        ),
    ]));
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, body) =
        get_json(&test_app, "/api/discover?q=harry+potter&type=any", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let titles: Vec<String> = body["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|book| book["title"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        titles,
        vec![
            "Harry Potter and the Philosopher's Stone".to_string(),
            "Harry Potter and the Chamber of Secrets".to_string(),
            "Harry Potter and the Prisoner of Azkaban".to_string(),
            "Harry Potter: An Unofficial Guide".to_string(),
        ],
        "the dominant author's series leads and companion material follows"
    );
}

/// Explicit provider selection is authoritative: only that provider is
/// queried and the Automatic fallback is never reached behind it.
#[tokio::test]
async fn explicit_provider_selection_never_falls_back() {
    let primary = Arc::new(
        FakeMetadataProvider::new(vec![metadata_result(
            "/works/OLPRIMARYW",
            "Primary Catalogue Book",
            &["Primary Author"],
            None,
        )])
        .named("openlibrary")
        .with_query_filter(),
    );
    let fallback = Arc::new(
        FakeMetadataProvider::new(vec![metadata_result(
            "/works/OLFALLBACKW",
            "Fallback Catalogue Book",
            &["Fallback Author"],
            None,
        )])
        .named("google_books")
        .with_query_filter(),
    );
    let test_app = common::test_app_with_provider_matrix(
        primary.clone(),
        Some(fallback.clone()),
        primary.clone(),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    // Automatic keeps the existing gap-filling semantics.
    let (status, auto) = get_json(
        &test_app,
        "/api/discover?q=fallback%20catalogue&type=any",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        auto["provider"], "google_books",
        "automatic gap-fills from the fallback: {auto}"
    );
    assert!(
        auto["books"]
            .as_array()
            .unwrap()
            .iter()
            .any(|book| book["title"] == "Fallback Catalogue Book")
    );

    // Explicit selection answers from that provider alone.
    let (status, explicit) = get_json(
        &test_app,
        "/api/discover?q=fallback%20catalogue&type=any&provider=google_books",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(explicit["provider"], "google_books");
    assert!(
        explicit["books"]
            .as_array()
            .unwrap()
            .iter()
            .any(|book| book["title"] == "Fallback Catalogue Book")
    );

    // Selecting the empty primary does NOT fall through to the fallback.
    let (status, primary_only) = get_json(
        &test_app,
        "/api/discover?q=fallback%20catalogue&type=any&provider=openlibrary",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(primary_only["provider"], "openlibrary");
    assert!(
        primary_only["books"].as_array().unwrap().is_empty(),
        "an explicit provider must not be silently backfilled: {primary_only}"
    );

    let (status, unknown) = get_json(
        &test_app,
        "/api/discover?q=anything&type=any&provider=does_not_exist",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{unknown}");
}

/// The first page asks the provider for more candidates than the UI shows so
/// ranking can lift books the provider ordered lower.
#[tokio::test]
async fn first_page_fetches_a_deeper_candidate_pool_and_truncates() {
    let results: Vec<MetadataResult> = (1..=70)
        .map(|index| {
            metadata_result(
                &format!("/works/OLPOOL{index}W"),
                &format!("Candidate Book {index:02}"),
                &["Pool Author"],
                None,
            )
        })
        .collect();
    let provider = Arc::new(FakeMetadataProvider::new(results).with_paging());
    let test_app = common::test_app_with_metadata(provider.clone()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, page) = get_json(
        &test_app,
        "/api/discover?q=candidate&type=any&limit=24",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        provider.last_limit(),
        50,
        "the provider is asked for a deeper first-page pool"
    );
    assert_eq!(
        page["books"].as_array().unwrap().len(),
        24,
        "the response keeps the requested page size"
    );
    let mut keys: Vec<String> = page["books"]
        .as_array()
        .unwrap()
        .iter()
        .map(|book| book["providerKey"].as_str().unwrap().to_string())
        .collect();
    let mut next = page["next"].as_str().map(str::to_string);
    let mut later_page_sizes = Vec::new();
    while let Some(token) = next {
        let continuation = token.replace('\u{1f}', "%1F").replace(':', "%3A");
        let (status, more) = get_json(
            &test_app,
            &format!("/api/discover?q=candidate&type=any&limit=24&continuation={continuation}"),
            &cookie,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let books = more["books"].as_array().unwrap();
        later_page_sizes.push(books.len());
        keys.extend(
            books
                .iter()
                .map(|book| book["providerKey"].as_str().unwrap().to_string()),
        );
        next = more["next"].as_str().map(str::to_string);
        assert!(later_page_sizes.len() <= 4, "search cursor must terminate");
    }
    assert_eq!(later_page_sizes, vec![24, 2, 20]);
    let unique: std::collections::HashSet<_> = keys.iter().collect();
    assert_eq!(keys.len(), 70);
    assert_eq!(
        unique.len(),
        70,
        "every fetched candidate appears exactly once"
    );
}

#[tokio::test]
async fn anywhere_series_search_brings_its_novels_into_the_first_page() {
    let mut results = vec![
        metadata_result(
            "/works/guide",
            "An Introduction to The Expanse",
            &["TV Writer"],
            None,
        ),
        metadata_result("/works/show", "The Expanse", &["TV Writer"], None),
    ];
    for (key, title, author) in [
        ("/works/leviathan", "Leviathan Wakes", "James S. A. Corey"),
        ("/works/caliban", "Caliban's War", "James S. A. Corey"),
        ("/works/abaddon", "Abaddon's Gate", "James S. A. Corey"),
        ("/works/unrelated", "Giraffes", "Another Author"),
    ] {
        let mut result = metadata_result(key, title, &[author], None);
        result.series = Some("The Expanse".to_string());
        results.push(result);
    }
    for result in &mut results {
        result.provider = "openlibrary".to_string();
    }
    let provider = Arc::new(
        FakeMetadataProvider::new(results)
            .named("openlibrary")
            .with_query_filter(),
    );
    let app = common::test_app_with_metadata(provider).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let (status, page) = get_json(&app, "/api/discover?q=the%20expanse&type=any", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    let books = page["books"].as_array().unwrap();
    assert_eq!(books[0]["title"], "Leviathan Wakes");
    assert!(!books.iter().any(|book| book["title"] == "Giraffes"));
}

#[tokio::test]
async fn exact_provider_title_is_not_displaced_by_related_series_books() {
    let mut original = metadata_result(
        "/works/original",
        "The Lord of the Rings",
        &["J. R. R. Tolkien"],
        None,
    );
    original.provider = "openlibrary".to_string();
    let mut related = metadata_result(
        "/works/related",
        "The Return of the Shadow",
        &["J. R. R. Tolkien"],
        None,
    );
    related.provider = "openlibrary".to_string();
    related.series = Some("The Lord of the Rings".to_string());
    let provider = Arc::new(
        FakeMetadataProvider::new(vec![original, related])
            .named("openlibrary")
            .with_query_filter(),
    );
    let app = common::test_app_with_metadata(provider.clone()).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let (status, page) = get_json(
        &app,
        "/api/discover?q=the%20lord%20of%20the%20rings&type=any",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["books"][0]["title"], "The Lord of the Rings");
    assert_eq!(
        provider.calls(),
        1,
        "an exact provider result needs no series query"
    );
}
