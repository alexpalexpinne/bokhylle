use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod common;

#[tokio::test]
async fn unknown_route_returns_not_found_envelope() {
    let test_app = common::test_app().await;

    let response = test_app
        .router
        .oneshot(
            Request::builder()
                .uri("/api/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.headers().contains_key("x-request-id"));

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "not_found");
    assert_eq!(json["message"], "resource not found");
    assert!(json.get("details").is_some());
}
