use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

mod common;

fn write_dist() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><html><body>app</body></html>",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("assets/app-abc123.js"),
        "console.log('app')",
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn serves_index_and_spa_fallback_without_caching() {
    let dist = write_dist();
    let test_app = common::test_app_with_web_root(dist.path().to_path_buf()).await;

    for uri in ["/", "/library"] {
        let response = test_app
            .router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK, "uri: {uri}");
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache",
            "uri: {uri}"
        );

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.starts_with("text/html"), "uri: {uri}");
    }
}

#[tokio::test]
async fn hashed_assets_are_immutable() {
    let dist = write_dist();
    let test_app = common::test_app_with_web_root(dist.path().to_path_buf()).await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/app-abc123.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=31536000, immutable"
    );
}

#[tokio::test]
async fn missing_asset_returns_404_instead_of_caching_the_app_html() {
    let dist = write_dist();
    let test_app = common::test_app_with_web_root(dist.path().to_path_buf()).await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/removed-chunk.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.headers().get(header::CACHE_CONTROL).is_none());
}

#[tokio::test]
async fn unknown_api_paths_still_return_error_envelope() {
    let dist = write_dist();
    let test_app = common::test_app_with_web_root(dist.path().to_path_buf()).await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "not_found");
}

#[tokio::test]
async fn serves_the_generated_openapi_document() {
    let test_app = common::test_app().await;
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let document: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(document, bokhylle_server::openapi_document());
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(
        document["components"]["securitySchemes"]["sessionCookie"]["name"],
        "bokhylle_session"
    );
    assert_eq!(
        document["paths"]["/api/admin/backup"]["get"]["responses"]["200"]["content"]["application/octet-stream"]
            ["schema"]["format"],
        "binary"
    );
    assert_eq!(
        document["paths"]["/api/auth/logout"]["post"]["security"],
        serde_json::json!([])
    );
    assert!(document["paths"]["/api/auth/logout-all"]["post"]["responses"]["204"]["headers"]["Set-Cookie"].is_object());
}
