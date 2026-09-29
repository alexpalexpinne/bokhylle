use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bokhylle_server::auth::Role;
use tower::ServiceExt;

mod common;

fn picture() -> Vec<u8> {
    base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lQAAAABJRU5ErkJggg==",
    )
    .unwrap()
}

async fn request(
    app: &common::TestApp,
    method: &str,
    cookie: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri("/api/profile/avatar");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let response = app
        .router
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

async fn me_avatar_version(app: &common::TestApp, cookie: &str) -> serde_json::Value {
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice::<serde_json::Value>(&body).unwrap()["user"]["avatarVersion"].clone()
}

#[tokio::test]
async fn avatar_is_private_to_each_account_and_can_be_removed() {
    let app = common::test_app().await;
    let alice = app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();
    app.state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(alice.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let alice_cookie = common::login(&app, "alice", "password123").await;
    let bob_cookie = common::login(&app, "bob", "password123").await;
    let picture = picture();
    assert!(me_avatar_version(&app, &alice_cookie).await.is_null());

    assert_eq!(
        request(&app, "GET", None, None, vec![]).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "PUT", None, Some("image/png"), picture.clone())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "GET", Some(&alice_cookie), None, vec![])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            Some(&alice_cookie),
            Some("image/png"),
            picture.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, headers, data) = request(&app, "GET", Some(&alice_cookie), None, vec![]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(data, picture);
    assert_eq!(
        request(&app, "GET", Some(&bob_cookie), None, vec![])
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    let version: i64 = sqlx::query_scalar("SELECT avatar_version FROM users WHERE id = ?")
        .bind(alice.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(version, 1);
    let stored: (Vec<u8>, String) =
        sqlx::query_as("SELECT data, mime FROM user_avatars WHERE user_id = ?")
            .bind(alice.id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, (picture, "image/png".into()));
    assert_eq!(me_avatar_version(&app, &alice_cookie).await, 1);
    assert_eq!(
        request(&app, "DELETE", Some(&alice_cookie), None, vec![])
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "GET", Some(&alice_cookie), None, vec![])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(me_avatar_version(&app, &alice_cookie).await.is_null());
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM user_avatars WHERE user_id = ?")
        .bind(alice.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn avatar_rejects_invalid_type_and_oversize_without_replacing_picture() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let picture = picture();
    assert_eq!(
        request(
            &app,
            "PUT",
            Some(&cookie),
            Some("image/png"),
            picture.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let invalid = b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec();
    assert_eq!(
        request(&app, "PUT", Some(&cookie), Some("image/svg+xml"), invalid)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            Some(&cookie),
            Some("image/png"),
            vec![0; 1024 * 1024 + 1]
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            Some(&cookie),
            Some("image/jpeg"),
            picture.clone()
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(&app, "GET", Some(&cookie), None, vec![]).await.2,
        picture
    );
}
