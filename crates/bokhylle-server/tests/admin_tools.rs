use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;

use bokhylle_server::auth::Role;

mod common;

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

#[tokio::test]
async fn maintenance_endpoints_are_admin_only_and_work() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 2).unwrap();

    let test_app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin", "password123").await;
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let bob_cookie = common::login(&test_app, "bob", "password123").await;

    for uri in [
        "/api/admin/integrity",
        "/api/admin/logs",
        "/api/admin/backup",
    ] {
        let response = test_app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::COOKIE, &bob_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "uri: {uri}");
    }

    // scan, then remove one file from disk and let integrity report it
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/library/scan")
                .header(header::COOKIE, &admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    for _ in 0..200 {
        let (_, status) = get_json(&test_app, "/api/library/scan/status", &admin_cookie).await;
        if status["running"] == false && status["summary"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let path: String = sqlx::query_scalar("SELECT path FROM book_files ORDER BY id LIMIT 1")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    std::fs::remove_file(&path).unwrap();

    let (status, report) = get_json(&test_app, "/api/admin/integrity", &admin_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(report["filesChecked"].as_u64().unwrap() >= 1);
    assert_eq!(report["missingCount"], 1);
    assert_eq!(report["missing"][0]["path"], path);

    // logs endpoint returns the expected envelope
    let (status, logs) = get_json(&test_app, "/api/admin/logs", &admin_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(logs["lines"].is_array());

    // backup streams a SQLite database
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/backup")
                .header(header::COOKIE, &admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let length: usize = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.len(), length);
    assert!(
        body.starts_with(b"SQLite format 3\x00"),
        "backup is not a sqlite file"
    );

    // A completed stream and a cancelled one both remove their temporary DB.
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/backup")
                .header(header::COOKIE, &admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);

    let cache_dir = test_app.state.paths.config_dir.join("cache");
    for _ in 0..100 {
        let remaining = std::fs::read_dir(&cache_dir)
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with("bokhylle-"));
        if !remaining {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("download backups left temporary database files behind");
}
