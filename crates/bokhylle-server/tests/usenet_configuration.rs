use std::sync::Arc;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use bokhylle_server::{
    auth::Role,
    providers::{ProviderFactory, SettingsProviderFactory},
    settings,
};
use serde_json::json;
use tower::ServiceExt;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_string_contains, method, path, query_param},
};

mod common;

#[tokio::test]
async fn configured_usenet_connectors_are_testable_only_by_admins_and_keys_stay_private() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/newznab/api"))
        .and(query_param("t", "caps"))
        .and(query_param("apikey", "newznab-private-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("<caps><server version=\"1.0\"/></caps>", "application/xml"),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sab/api"))
        .and(body_string_contains("mode=version"))
        .and(body_string_contains("apikey=sab-private-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version": "4.0"})))
        .expect(1)
        .mount(&server)
        .await;
    let library = tempfile::tempdir().unwrap();
    let app =
        common::test_app_with_factory(library.path().into(), Arc::new(SettingsProviderFactory))
            .await;
    for (key, value) in [
        (
            settings::NEWZNAB_URL,
            format!("{}/newznab/api", server.uri()),
        ),
        (settings::NEWZNAB_API_KEY, "newznab-private-key".into()),
        (settings::SABNZBD_URL, format!("{}/sab/api", server.uri())),
        (settings::SABNZBD_API_KEY, "sab-private-key".into()),
    ] {
        app.state.settings.set(key, &json!(value)).await.unwrap();
    }
    assert_eq!(
        SettingsProviderFactory
            .indexer(&app.state)
            .await
            .unwrap()
            .unwrap()
            .name(),
        "newznab"
    );
    assert_eq!(
        SettingsProviderFactory
            .nzb_downloader(&app.state)
            .await
            .unwrap()
            .unwrap()
            .name(),
        "sabnzbd"
    );
    app.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    app.state
        .auth
        .create_user("member", "password123", Role::User)
        .await
        .unwrap();
    let admin = common::login(&app, "admin", "password123").await;
    let member = common::login(&app, "member", "password123").await;
    for connector in ["newznab", "sabnzbd"] {
        for (cookie, expected) in [(&member, StatusCode::FORBIDDEN), (&admin, StatusCode::OK)] {
            let response = app
                .router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/admin/integrations/{connector}/test"))
                        .header(header::COOKIE, cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }
    }
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/settings")
                .header(header::COOKIE, &admin)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    let body = std::str::from_utf8(&body).unwrap();
    assert!(!body.contains("newznab-private-key"));
    assert!(!body.contains("sab-private-key"));
}
