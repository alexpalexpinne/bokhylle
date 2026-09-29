use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use bokhylle_server::auth::Role;
use bokhylle_server::observability::LogCapture;

mod common;

#[tokio::test]
async fn logs_events_but_never_secrets() {
    let test_app = common::test_app().await;
    let test_credential = "test-only-password-123";

    test_app
        .state
        .auth
        .create_user("alice", test_credential, Role::User)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("root", test_credential, Role::Admin)
        .await
        .unwrap();

    let capture = LogCapture::new();

    let login_body = serde_json::json!({ "username": "alice", "password": test_credential });
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&login_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let session_token = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("bokhylle_session=")
        .to_string();

    let bad_body = serde_json::json!({ "username": "alice", "password": "wrong-password" });
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&bad_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let admin_login_body = serde_json::json!({ "username": "root", "password": test_credential });
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&admin_login_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let admin_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/admin/settings/library.preferred_format")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &admin_cookie)
                .body(Body::from(r#"{"value": "pdf"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let logs = capture.logs();

    assert!(logs.contains("auth.login"), "logs: {logs}");
    assert!(logs.contains("auth.login.failed"), "logs: {logs}");
    assert!(logs.contains("settings.updated"), "logs: {logs}");

    assert!(!logs.contains(test_credential), "password leaked into logs");
    assert!(
        !logs.contains(&session_token),
        "session token leaked into logs"
    );
}
