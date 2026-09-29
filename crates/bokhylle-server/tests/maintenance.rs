use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;

mod common;

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
async fn image_backfill_fetches_covers_authors_and_repairs_titles() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 2).unwrap();

    let metadata = Arc::new(FakeMetadataProvider::new(vec![]));
    metadata.set_cover(vec![0xFFu8; 4096]);
    metadata.set_author("Andy Weir", "OL123A", vec![0xFFu8; 4096]);

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
    let cookie = common::login(&test_app, "admin", "password123").await;

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

    let (_, books) = get_json(&test_app, "/api/books?pageSize=50", &cookie).await;
    let hail_mary = books["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["title"] == "Project Hail Mary")
        .expect("Project Hail Mary indexed");
    let book_id = hail_mary["id"].as_i64().unwrap();

    // simulate the historical bug: the book's only author is filename junk
    sqlx::query("DELETE FROM book_authors WHERE book_id = ?")
        .bind(book_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let junk_id = sqlx::query(
        "INSERT INTO authors (name, normalized_name) VALUES ('Project Hail Mary (v2)', 'project hail mary v2')",
    )
    .execute(&test_app.state.db)
    .await
    .unwrap()
    .last_insert_rowid();
    sqlx::query("INSERT INTO book_authors (book_id, author_id, position) VALUES (?, ?, 0)")
        .bind(book_id)
        .bind(junk_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    // fixture covers are 1x1 stubs, so every book starts without a usable cover
    let missing: i64 = sqlx::query_scalar("SELECT count(*) FROM books WHERE cover_path IS NULL")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert!(missing >= 2, "expected missing covers, got {missing}");

    bokhylle_server::maintenance::run(&test_app.state)
        .await
        .unwrap();

    let status = bokhylle_server::maintenance::status();
    assert!(status.covers_fetched >= 2, "status: {status:?}");
    assert!(status.authors_resolved >= 1, "status: {status:?}");
    assert_eq!(status.books_repaired, 1, "status: {status:?}");

    let (cover_path, author): (Option<String>, String) = sqlx::query_as(
        "SELECT b.cover_path, a.name
         FROM books b
         JOIN book_authors ba ON ba.book_id = b.id
         JOIN authors a ON a.id = ba.author_id
         WHERE b.id = ?",
    )
    .bind(book_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(author, "Andy Weir", "junk author must be replaced");
    let cover_path = cover_path.expect("cover path set");
    assert!(std::path::Path::new(&cover_path).exists());

    let olid: Option<String> =
        sqlx::query_scalar("SELECT olid FROM authors WHERE name = 'Andy Weir'")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(olid.as_deref(), Some("OL123A"));

    let junk_remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM authors WHERE id = ?")
        .bind(junk_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(junk_remaining, 0, "orphaned junk author pruned");
}

#[tokio::test]
async fn metadata_job_reports_failures_and_cancel_requires_a_running_job() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "admin", "password123").await;

    let metadata = bokhylle_metadata::MetadataResult {
        provider: "fake".to_string(),
        provider_key: String::new(),
        title: "Orphan Book".to_string(),
        authors: vec!["Some Author".to_string()],
        ..Default::default()
    };
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &metadata,
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
         VALUES (?, '/tmp/orphan.epub', 'epub', 10, 'orphan-digest')",
    )
    .bind(edition_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    bokhylle_server::maintenance::run_metadata(&test_app.state, false)
        .await
        .unwrap();

    let status = bokhylle_server::maintenance::metadata_status();
    assert_eq!(status.books_failed, 1);
    assert_eq!(status.failures.len(), 1);
    assert_eq!(status.failures[0].book_id, book_id);
    assert_eq!(status.failures[0].title, "Orphan Book");
    // Without a key or ISBN the job falls back to a title/author search;
    // the fake provider has no match, so the reported reason is the search
    // outcome rather than "no provider key or ISBN".
    assert_eq!(status.failures[0].reason, "no metadata found");

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/maintenance/metadata/cancel")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}
