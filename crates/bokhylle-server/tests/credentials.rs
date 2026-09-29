use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_server::auth::{Role, hash_password};

mod common;

async fn request(
    test_app: &common::TestApp,
    method: &str,
    uri: &str,
    cookie: &str,
    payload: Option<Value>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = match payload {
        Some(payload) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&payload).unwrap())
        }
        None => Body::empty(),
    };
    test_app
        .router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap()
}

fn session_cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let value = value.to_str().ok()?;
            Some(value.split(';').next().unwrap_or_default().to_string())
        })
        .unwrap_or_default()
}

async fn login(
    test_app: &common::TestApp,
    username: &str,
    secret: &str,
    remember: bool,
) -> axum::response::Response {
    request(
        test_app,
        "POST",
        "/api/auth/login",
        "",
        Some(json!({ "username": username, "password": secret, "remember": remember })),
    )
    .await
}

#[tokio::test]
async fn legacy_credentials_upgrade_to_the_typed_format() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("old", "legacy-secret", Role::User)
        .await
        .unwrap();

    // simulate a pre-migration row: raw hash, version 1
    sqlx::query(
        "UPDATE users SET credential_type = 'legacy', credential_version = 1, password_hash = ?
         WHERE username = 'old'",
    )
    .bind(hash_password("legacy-secret").unwrap())
    .execute(&test_app.state.db)
    .await
    .unwrap();

    let response = login(&test_app, "old", "legacy-secret", true).await;
    assert_eq!(response.status(), StatusCode::OK);

    let (version, credential_type): (i64, String) = sqlx::query_as(
        "SELECT credential_version, credential_type FROM users WHERE username = 'old'",
    )
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(version, 2, "legacy hash is upgraded in place");
    assert_eq!(credential_type, "legacy");

    // the upgraded hash still verifies the same secret
    let response = login(&test_app, "old", "legacy-secret", true).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn changing_a_credential_rotates_every_session() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user_with_type("emma", "246810", Role::User, "pin")
        .await
        .unwrap();

    let first = session_cookie(&login(&test_app, "emma", "246810", true).await);
    let second = session_cookie(&login(&test_app, "emma", "246810", true).await);
    assert!(!first.is_empty() && !second.is_empty());

    let wrong = request(
        &test_app,
        "PUT",
        "/api/profile/credential",
        &first,
        Some(json!({ "current": "000111", "credentialType": "pin", "credential": "135790" })),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    let response = request(
        &test_app,
        "PUT",
        "/api/profile/credential",
        &first,
        Some(json!({
            "current": "246810",
            "credentialType": "password",
            "credential": "newpassword123"
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let rotated = session_cookie(&response);
    assert!(!rotated.is_empty());

    for cookie in [&first, &second] {
        let response = request(&test_app, "GET", "/api/auth/me", cookie, None).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "old session revoked"
        );
    }

    let response = request(&test_app, "GET", "/api/auth/me", &rotated, None).await;
    assert_eq!(response.status(), StatusCode::OK, "fresh session works");

    assert_eq!(
        login(&test_app, "emma", "newpassword123", true)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        login(&test_app, "emma", "246810", true).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn admin_reset_revokes_sessions_and_promotion_needs_a_password() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin_cookie = session_cookie(&login(&test_app, "admin", "password123", true).await);
    test_app
        .state
        .auth
        .create_user_with_type("emma", "246810", Role::User, "pin")
        .await
        .unwrap();
    let emma = test_app
        .state
        .auth
        .verify_login("emma", "246810")
        .await
        .unwrap()
        .unwrap();
    let emma_cookie = session_cookie(&login(&test_app, "emma", "246810", true).await);

    // promoting without a password is rejected
    let response = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{}", emma.id),
        &admin_cookie,
        Some(json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // promoting with a password works
    let response = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{}", emma.id),
        &admin_cookie,
        Some(json!({
            "role": "admin",
            "credentialType": "password",
            "credential": "emmapassword1"
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let promoted: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(promoted["role"], "admin");

    // resetting the credential revokes that user's active sessions
    let response = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{}", emma.id),
        &admin_cookie,
        Some(json!({ "role": "user", "credentialType": "pin", "credential": "135790" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = request(&test_app, "GET", "/api/auth/me", &emma_cookie, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        login(&test_app, "emma", "135790", true).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn non_remembered_sessions_are_short_and_do_not_roll() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();
    let cookie = session_cookie(&login(&test_app, "alice", "password123", false).await);

    let (remembered, remaining): (i64, i64) =
        sqlx::query_as("SELECT remembered, expires_at - unixepoch() FROM sessions")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(remembered, 0);
    assert!(
        remaining <= 12 * 3600 && remaining > 11 * 3600,
        "12h session, got {remaining}"
    );

    sqlx::query("UPDATE sessions SET expires_at = unixepoch() + 1000")
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let response = request(&test_app, "GET", "/api/auth/me", &cookie, None).await;
    assert_eq!(response.status(), StatusCode::OK);

    let remaining: i64 = sqlx::query_scalar("SELECT expires_at - unixepoch() FROM sessions")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert!(
        remaining < 3600,
        "non-remembered sessions must not roll, got {remaining}"
    );
}

#[tokio::test]
async fn credential_change_preserves_the_session_kind() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user_with_type("emma", "246810", Role::User, "pin")
        .await
        .unwrap();

    // sign in WITHOUT remember-this-device, then change the credential
    let cookie = session_cookie(&login(&test_app, "emma", "246810", false).await);
    let response = request(
        &test_app,
        "PUT",
        "/api/profile/credential",
        &cookie,
        Some(json!({
            "current": "246810",
            "credentialType": "password",
            "credential": "newpassword123"
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let rotated = session_cookie(&response);

    let (remembered, remaining): (i64, i64) =
        sqlx::query_as("SELECT remembered, expires_at - unixepoch() FROM sessions")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(
        remembered, 0,
        "rotation must not turn this into a remembered device"
    );
    assert!(
        remaining <= 12 * 3600,
        "session stays short, got {remaining}"
    );

    let response = request(&test_app, "GET", "/api/auth/me", &rotated, None).await;
    assert_eq!(response.status(), StatusCode::OK);
}
