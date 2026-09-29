use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;

use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_server::auth::Role;

mod common;

async fn post_empty(test_app: &common::TestApp, uri: &str, cookie: &str) -> (StatusCode, Value) {
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
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
async fn integration_tests_report_versions_and_require_admin() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let test_app = common::test_app_with_providers(
        library_dir.path().to_path_buf(),
        Arc::new(FakeIndexerProvider::default()),
        Arc::new(FakeDownloadProvider::default()),
    )
    .await;

    test_app
        .state
        .auth
        .create_user("root", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "root", "password123").await;
    let user_cookie = common::login(&test_app, "bob", "password123").await;

    let (status, _) = post_empty(
        &test_app,
        "/api/admin/integrations/prowlarr/test",
        &user_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = post_empty(
        &test_app,
        "/api/admin/integrations/prowlarr/test",
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], "fake-indexer 1.0");

    let (status, body) = post_empty(
        &test_app,
        "/api/admin/integrations/qbittorrent/test",
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], "fake-downloader 1.0");
}

#[tokio::test]
async fn integration_tests_fail_clearly_when_unconfigured() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("root", "password123", Role::Admin)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "root", "password123").await;

    let (status, body) = post_empty(
        &test_app,
        "/api/admin/integrations/prowlarr/test",
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "unprocessable_entity");
}
