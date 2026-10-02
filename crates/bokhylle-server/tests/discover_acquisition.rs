use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;

mod common;

fn hail_mary() -> MetadataResult {
    MetadataResult {
        provider: "fake".to_string(),
        provider_key: "/works/OL1W".to_string(),
        title: "Project Hail Mary".to_string(),
        authors: vec!["Andy Weir".to_string()],
        year: Some(2021),
        language: Some("en".to_string()),
        isbn13: Some("9780593135204".to_string()),
        cover_id: Some("123".to_string()),
        description: Some("A lone astronaut must save the earth from disaster.".to_string()),
        subjects: vec!["Science fiction".to_string(), "Space opera".to_string()],
        ..Default::default()
    }
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

#[tokio::test]
async fn wanted_authors_without_owned_books_are_hidden() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![hail_mary()]));
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, _) = post_json(
        &test_app,
        "/api/discover/acquisitions",
        &cookie,
        json!({ "provider": "fake", "providerKey": "/works/OL1W" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, authors) = get_json(&test_app, "/api/authors", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        authors.as_array().unwrap().len(),
        0,
        "authors of wanted books must not appear: {authors}"
    );
}

#[tokio::test]
async fn book_detail_returns_metadata_and_status() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![hail_mary()]));
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, detail) = get_json(
        &test_app,
        "/api/discover/book?providerKey=/works/OL1W",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["title"], "Project Hail Mary");
    assert_eq!(detail["authors"][0], "Andy Weir");
    assert!(detail["description"].is_string());
    assert_eq!(detail["status"], "NOT_IN_LIBRARY");

    let (status, _) = get_json(
        &test_app,
        "/api/discover/book?providerKey=/works/UNKNOWN",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // a wanted book with an active acquisition reports as downloading
    let (status, _) = post_json(
        &test_app,
        "/api/discover/acquisitions",
        &cookie,
        json!({ "provider": "fake", "providerKey": "/works/OL1W" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (_, detail) = get_json(
        &test_app,
        "/api/discover/book?providerKey=/works/OL1W",
        &cookie,
    )
    .await;
    assert_eq!(detail["status"], "DOWNLOADING");
}

#[tokio::test]
async fn book_detail_reports_owned_file_ids() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 2).unwrap();

    let provider = Arc::new(FakeMetadataProvider::new(vec![hail_mary()]));
    let test_app = common::test_app_full(
        library_dir.path().to_path_buf(),
        provider,
        Arc::new(FakeIndexerProvider::default()),
        Arc::new(FakeDownloadProvider::default()),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/library/scan")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    for _ in 0..200 {
        let (_, status) = get_json(&test_app, "/api/library/scan/status", &cookie).await;
        if status["running"] == false && status["summary"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let (status, detail) = get_json(
        &test_app,
        "/api/discover/book?providerKey=/works/OL1W",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["status"], "IN_LIBRARY");
    assert!(detail["ownedBookId"].is_i64());
    assert!(detail["ownedFileId"].is_i64(), "detail: {detail}");
}

#[tokio::test]
async fn release_preview_lists_seeders_before_acquiring() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![hail_mary()]));
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(vec![
        bokhylle_acquisition::model::ReleaseCandidate {
            source: None,
            method: None,
            id: "r1".to_string(),
            title: "Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB".to_string(),
            indexer: Some("IPTorrents".to_string()),
            size_bytes: 2_900_000,
            seeders: Some(41),
            leechers: Some(6),
            download_url: Some("http://indexer.test/1".to_string()),
            magnet_url: Some(
                "magnet:?xt=urn:btih:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string(),
            ),
            info_url: None,
            detected_title: None,
            detected_author: None,
            detected_format: None,
            detected_language: None,
            detected_volume: None,
            is_collection: false,
            is_audiobook: false,
            is_comic: false,
        },
    ]));
    let test_app = common::test_app_full(
        tempfile::tempdir().unwrap().path().to_path_buf(),
        provider,
        indexer,
        Arc::new(FakeDownloadProvider::default()),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, body) = get_json(
        &test_app,
        "/api/discover/releases?providerKey=/works/OL1W&format=epub",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["releases"][0]["seeders"], 41);
    assert_eq!(body["releases"][0]["format"], "epub");
    assert_eq!(body["releases"][0]["method"], "torrent");
    assert_eq!(
        body["releases"][0]["releaseName"],
        "Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB"
    );
    assert_eq!(body["releases"][0]["leechers"], 6);
    assert_eq!(body["releases"][0]["indexer"], "IPTorrents");

    sqlx::query("UPDATE users SET can_acquire = 0 WHERE username = 'reader'")
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let (status, restricted) = get_json(
        &test_app,
        "/api/discover/releases?providerKey=/works/OL1W&format=epub",
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        restricted["releases"][0]["releaseName"].is_null(),
        "a cached preview must still apply the viewer's permissions"
    );
    assert!(restricted["releases"][0]["indexer"].is_null());

    test_app
        .state
        .auth
        .create_user("root", "password123", Role::Admin)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "root", "password123").await;
    let (status, admin_body) = get_json(
        &test_app,
        "/api/discover/releases?providerKey=/works/OL1W&format=epub",
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(admin_body["releases"][0]["releaseName"].is_string());
    assert_eq!(admin_body["releases"][0]["leechers"], 6);
    assert_eq!(
        admin_body, body,
        "adults who can acquire and admins see the same choices"
    );
}

#[tokio::test]
async fn discover_add_creates_wanted_book_and_acquisition() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![hail_mary()]));
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let payload = json!({ "provider": "fake", "providerKey": "/works/OL1W" });
    let (status, created) = post_json(
        &test_app,
        "/api/discover/acquisitions",
        &cookie,
        payload.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(created["status"], "REQUESTED");
    assert_eq!(created["duplicate"], false);
    let book_id = created["bookId"].as_i64().unwrap();
    let acquisition_id = created["id"].as_str().unwrap().to_string();

    let (_, library) = get_json(&test_app, "/api/books", &cookie).await;
    assert_eq!(
        library["total"], 0,
        "wanted books are not in the library yet"
    );

    let (status, duplicate) =
        post_json(&test_app, "/api/discover/acquisitions", &cookie, payload).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["id"], acquisition_id);
    assert_eq!(duplicate["bookId"], book_id);

    let (_, search) = get_json(&test_app, "/api/discover/search?q=project", &cookie).await;
    assert_eq!(search[0]["status"], "DOWNLOADING");
}

#[tokio::test]
async fn discover_add_validates_provider_and_key() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![hail_mary()]));
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, _) = post_json(
        &test_app,
        "/api/discover/acquisitions",
        &cookie,
        json!({ "provider": "openlibrary", "providerKey": "/works/OL1W" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post_json(
        &test_app,
        "/api/discover/acquisitions",
        &cookie,
        json!({ "provider": "fake", "providerKey": "/works/UNKNOWN" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn scanned_file_merges_into_wanted_book() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let provider = Arc::new(FakeMetadataProvider::new(vec![hail_mary()]));
    let test_app =
        common::test_app_with_library_and_metadata(library_dir.path().to_path_buf(), provider)
            .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let (status, created) = post_json(
        &test_app,
        "/api/discover/acquisitions",
        &cookie,
        json!({ "provider": "fake", "providerKey": "/works/OL1W" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let status = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/library/scan")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::ACCEPTED);
    for _ in 0..200 {
        let (_, status) = get_json(&test_app, "/api/library/scan/status", &cookie).await;
        if status["running"] == false && status["summary"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let book_count: i64 = sqlx::query_scalar("SELECT count(*) FROM books")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(
        book_count, 2,
        "wanted book merged with the scanned file; mystery book is separate"
    );

    let (_, library) = get_json(&test_app, "/api/books", &cookie).await;
    assert_eq!(library["total"], 2);
    let items = library["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|book| book["title"] == "Project Hail Mary")
    );

    let (_, search) = get_json(&test_app, "/api/discover/search?q=project", &cookie).await;
    assert_eq!(search[0]["status"], "IN_LIBRARY");

    let acquisition_id = created["id"].as_str().unwrap();
    let stored = bokhylle_server::acquisition::get(&test_app.state.db, acquisition_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.book_id, created["bookId"].as_i64().unwrap());
}
