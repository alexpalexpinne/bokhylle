use axum::body::{Body, to_bytes};
use axum::http::{Request, Response, StatusCode, header};
use tower::ServiceExt;

use bokhylle_server::auth::Role;

mod common;

async fn login_request(
    test_app: &common::TestApp,
    username: &str,
    password: &str,
) -> Response<axum::body::Body> {
    let body = serde_json::json!({ "username": username, "password": password });

    test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn session_cookie(response: &Response<axum::body::Body>) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn login_me_logout_flow() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();

    let response = login_request(&test_app, "alice", "password123").await;
    assert_eq!(response.status(), StatusCode::OK);

    let cookie = session_cookie(&response);
    assert!(cookie.starts_with("bokhylle_session="));

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["username"], "alice");
    assert_eq!(json["user"]["role"], "user");
    assert!(json["user"].get("password_hash").is_none());

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/logout")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn display_name_can_be_set_and_cleared() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();

    let response = login_request(&test_app, "alice", "password123").await;
    let cookie = session_cookie(&response);

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie)
                .body(Body::from(r#"{"displayName":"Alice Liddell"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["displayName"], "Alice Liddell");

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["displayName"], "Alice Liddell");

    // an empty name clears the display name
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie)
                .body(Body::from(r#"{"displayName":"  "}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["user"]["displayName"].is_null());
}

#[tokio::test]
async fn a_rejected_profile_update_changes_nothing() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();

    let response = login_request(&test_app, "alice", "password123").await;
    let cookie = session_cookie(&response);

    let update = |body: &'static str| {
        let request = Request::builder()
            .method("PUT")
            .uri("/api/profile")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, cookie.clone())
            .body(Body::from(body))
            .unwrap();
        test_app.router.clone().oneshot(request)
    };

    let response = update(r#"{"displayName":"Alice Baseline","preferredLanguages":["en"]}"#)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // A valid display name plus an invalid email must not half-apply.
    let response = update(r#"{"displayName":"Alice Changed","notificationEmail":"not-an-email"}"#)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let (display_name, languages): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT display_name, preferred_languages FROM users WHERE username = 'alice'",
    )
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(display_name.as_deref(), Some("Alice Baseline"));
    assert_eq!(languages.as_deref(), Some(r#"["en"]"#));
}

#[tokio::test]
async fn profile_preferences_roundtrip() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();

    let response = login_request(&test_app, "alice", "password123").await;
    let cookie = session_cookie(&response);

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie)
                .body(Body::from(
                    r#"{"preferredFormat":"pdf","preferredLanguage":"sv","acquisitionMode":"ask"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["preferredFormat"], "pdf");
    assert_eq!(json["user"]["acquisitionMode"], "ask");

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["preferredLanguage"], "sv");
    assert_eq!(json["user"]["acquisitionMode"], "ask");

    // clearing works with an empty string
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie)
                .body(Body::from(r#"{"preferredFormat":""}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["user"]["preferredFormat"].is_null());

    // invalid acquisition mode is rejected
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie)
                .body(Body::from(r#"{"acquisitionMode":"sometimes"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn appearance_is_persisted_and_scoped_to_each_profile() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user_with_profile("child", "246813", Role::User, "pin", "child")
        .await
        .unwrap();
    let alice = session_cookie(&login_request(&test_app, "alice", "password123").await);
    let bob = session_cookie(&login_request(&test_app, "bob", "password123").await);
    let child = session_cookie(&login_request(&test_app, "child", "246813").await);

    let request = |method: &str, cookie: &str, body: serde_json::Value| {
        test_app.router.clone().oneshot(
            Request::builder()
                .method(method)
                .uri(if method == "GET" {
                    "/api/auth/me"
                } else {
                    "/api/profile"
                })
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, cookie)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
    };
    for cookie in [&alice, &bob, &child] {
        let response = request("GET", cookie, serde_json::json!({})).await.unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["user"]["shelfFinish"], "oak");
        assert_eq!(json["user"]["shelfDecorations"], true);
        assert_eq!(json["user"]["spotlightRotation"], true);
    }
    let response = request("PUT", &alice, serde_json::json!({"shelfFinish":"black", "shelfDecorations":false, "spotlightRotation":false})).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = request("PUT", &bob, serde_json::json!({"shelfFinish":"metal"}))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response = request("PUT", &child, serde_json::json!({"shelfFinish":"metal"}))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Updating another preference leaves the saved appearance intact.
    let response = request(
        "PUT",
        &alice,
        serde_json::json!({"preferredLanguages":["sv"]}),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    for (cookie, finish, decorations, rotation, profile) in [
        (&alice, "black", false, false, "adult"),
        (&bob, "metal", true, true, "adult"),
        (&child, "oak", true, true, "child"),
    ] {
        let response = request("GET", cookie, serde_json::json!({})).await.unwrap();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["user"]["shelfFinish"], finish);
        assert_eq!(json["user"]["shelfDecorations"], decorations);
        assert_eq!(json["user"]["spotlightRotation"], rotation);
        assert_eq!(json["user"]["profileType"], profile);
        assert_eq!(json["user"]["role"], "user");
    }
    // Invalid finishes reject the whole payload before any profile write.
    let response = request("PUT", &alice, serde_json::json!({"shelfFinish":"walnut", "displayName":"Changed", "shelfDecorations":true})).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let saved: (String, i64, Option<String>) = sqlx::query_as(
        "SELECT shelf_finish, shelf_decorations, display_name FROM users WHERE username = 'alice'",
    )
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(saved, ("black".into(), 0, None));
    let response = request("PUT", "", serde_json::json!({"shelfFinish":"oak"}))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn members_may_use_a_six_digit_pin_but_admins_need_a_password() {
    let test_app = common::test_app().await;

    test_app
        .state
        .auth
        .create_user("emma", "246810", Role::User)
        .await
        .expect("members can use a 6-digit PIN");
    let response = login_request(&test_app, "emma", "246810").await;
    assert_eq!(response.status(), StatusCode::OK);

    test_app
        .state
        .auth
        .create_user_with_type("emma2", "246810", Role::User, "pin")
        .await
        .unwrap();
    let response = login_request(&test_app, "emma2", "246810").await;
    assert_eq!(response.status(), StatusCode::OK);

    let error = test_app
        .state
        .auth
        .create_user_with_type("blocked", "123456", Role::User, "pin")
        .await
        .expect_err("blocklisted PIN is rejected");
    assert!(error.to_string().contains("easy to guess"));

    let error = test_app
        .state
        .auth
        .create_user_with_type("root2", "246810", Role::Admin, "pin")
        .await
        .expect_err("admins must use passwords");
    assert!(
        error
            .to_string()
            .contains("administrators must use a password")
    );

    let error = test_app
        .state
        .auth
        .create_user("root3", "1234", Role::Admin)
        .await
        .expect_err("admins need at least 8 characters");
    assert!(error.to_string().contains("at least 8"));
}

#[tokio::test]
async fn login_user_picker_is_public_and_hides_disabled_accounts() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("emma", "246810", Role::User)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let bob = test_app
        .state
        .auth
        .verify_login("bob", "password123")
        .await
        .unwrap()
        .unwrap();
    test_app
        .state
        .auth
        .update_user_admin(bob.id, None, None, None, Some(true))
        .await
        .unwrap();

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let users = json["users"].as_array().unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0]["username"], "emma");
    assert_eq!(users[0]["authMode"], "pin");
    assert!(users[0].get("password").is_none());
}

#[tokio::test]
async fn unauthenticated_me_is_rejected_with_envelope() {
    let test_app = common::test_app().await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "unauthorized");
}

#[tokio::test]
async fn bad_credentials_are_rejected() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();

    let response = login_request(&test_app, "alice", "wrong-password").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "unauthorized");
    assert_eq!(json["message"], "authentication required");
}

#[tokio::test]
async fn login_rate_limit_blocks_after_repeated_failures() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();

    for _ in 0..10 {
        assert_eq!(
            login_request(&test_app, "alice", "wrong-password")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    assert_eq!(
        login_request(&test_app, "alice", "password123")
            .await
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn admin_settings_require_admin_role() {
    let test_app = common::test_app().await;
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

    let admin_cookie = session_cookie(&login_request(&test_app, "root", "password123").await);
    let user_cookie = session_cookie(&login_request(&test_app, "bob", "password123").await);

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/settings")
                .header(header::COOKIE, &user_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/admin/settings")
                .header(header::COOKIE, &admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

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

    let body = to_bytes(
        test_app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/settings")
                    .header(header::COOKIE, &admin_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .into_body(),
        usize::MAX,
    )
    .await
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["settings"]["library.preferred_format"], "pdf");

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/admin/settings/not.a.setting")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &admin_cookie)
                .body(Body::from(r#"{"value": "x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let secret = "super-secret-key-123";
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/admin/settings/integrations.prowlarr.api_key")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &admin_cookie)
                .body(Body::from(format!(r#"{{"value": "{secret}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(
        test_app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/settings")
                    .header(header::COOKIE, &admin_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .into_body(),
        usize::MAX,
    )
    .await
    .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        !text.contains(secret),
        "secret must never be returned by the API"
    );
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["secretsConfigured"]["integrations.prowlarr.api_key"],
        true
    );

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/admin/settings/integrations.prowlarr.api_key")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &admin_cookie)
                .body(Body::from(r#"{"value": 42}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn cross_origin_mutations_are_rejected() {
    let test_app = common::test_app().await;

    let body = serde_json::json!({ "username": "alice", "password": "password123" });

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://evil.example")
                .header(header::HOST, "localhost:8080")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "forbidden");
}

#[tokio::test]
async fn security_headers_are_present() {
    let test_app = common::test_app().await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .unwrap(),
        "nosniff"
    );
    assert_eq!(
        response.headers().get(header::X_FRAME_OPTIONS).unwrap(),
        "DENY"
    );
    assert!(
        response
            .headers()
            .contains_key(header::CONTENT_SECURITY_POLICY)
    );
    assert!(
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("script-src 'self' 'wasm-unsafe-eval'")
    );
}

#[tokio::test]
async fn sessions_roll_their_expiry_forward() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();

    let cookie = session_cookie(&login_request(&test_app, "alice", "password123").await);

    sqlx::query("UPDATE sessions SET expires_at = unixepoch() + 1000")
        .execute(&test_app.state.db)
        .await
        .unwrap();

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let remaining: i64 = sqlx::query_scalar("SELECT expires_at - unixepoch() FROM sessions")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert!(
        remaining > 20 * 24 * 3600,
        "an almost-expired session should be extended, got {remaining}s"
    );
}
