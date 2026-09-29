use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bokhylle_server::auth::Role;
use bokhylle_server::providers::{ProviderFactory, SettingsProviderFactory};
use bokhylle_server::settings;
use serde_json::json;
use tower::ServiceExt;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;

#[tokio::test]
async fn torznab_can_replace_prowlarr_without_changing_an_existing_default() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api"))
        .and(query_param("t", "caps"))
        .and(query_param("apikey", "torznab-secret"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(r#"<caps><server version="3.0"/></caps>"#, "application/xml"),
        )
        .mount(&server)
        .await;

    let library = tempfile::tempdir().unwrap();
    let app = common::test_app_with_factory(
        library.path().to_path_buf(),
        Arc::new(SettingsProviderFactory),
    )
    .await;
    app.state
        .settings
        .set(settings::PROWLARR_URL, &json!("http://127.0.0.1:9696"))
        .await
        .unwrap();
    app.state
        .settings
        .set(settings::PROWLARR_API_KEY, &json!("existing-secret"))
        .await
        .unwrap();
    app.state
        .settings
        .set(
            settings::TORZNAB_URL,
            &json!(format!("{}/api", server.uri())),
        )
        .await
        .unwrap();
    app.state
        .settings
        .set(settings::TORZNAB_API_KEY, &json!("torznab-secret"))
        .await
        .unwrap();

    let factory = SettingsProviderFactory;
    assert_eq!(
        factory.indexer(&app.state).await.unwrap().unwrap().name(),
        "prowlarr"
    );
    app.state
        .settings
        .set(settings::INDEXER_PROVIDER, &json!("torznab"))
        .await
        .unwrap();
    assert_eq!(
        factory.indexer(&app.state).await.unwrap().unwrap().name(),
        "torznab"
    );
    app.state
        .settings
        .set(settings::INDEXER_PROVIDER, &json!("auto"))
        .await
        .unwrap();
    app.state
        .settings
        .set(settings::PROWLARR_API_KEY, &json!(""))
        .await
        .unwrap();
    assert_eq!(
        factory.indexer(&app.state).await.unwrap().unwrap().name(),
        "torznab"
    );

    app.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&app, "admin", "password123").await;
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/integrations/torznab/test")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
