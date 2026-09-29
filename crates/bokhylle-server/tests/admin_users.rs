use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_server::auth::Role;

mod common;

async fn request(
    test_app: &common::TestApp,
    method: &str,
    uri: &str,
    cookie: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
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

    let response = test_app
        .router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn get_json(test_app: &common::TestApp, uri: &str, cookie: &str) -> (StatusCode, Value) {
    request(test_app, "GET", uri, cookie, None).await
}

#[tokio::test]
async fn admins_manage_household_users() {
    let test_app = common::test_app().await;
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

    let (status, _) = get_json(&test_app, "/api/admin/users", &bob_cookie).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, users) = get_json(&test_app, "/api/admin/users", &admin_cookie).await;
    assert_eq!(status, StatusCode::OK);
    let users = users.as_array().unwrap();
    assert_eq!(users.len(), 2);
    let admin_id = users
        .iter()
        .find(|user| user["username"] == "admin")
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    let (status, created) = request(
        &test_app,
        "POST",
        "/api/admin/users",
        &admin_cookie,
        Some(json!({
            "username": "carol",
            "password": "password123",
            "role": "user",
            "displayName": "Carol Reader"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["displayName"], "Carol Reader");
    assert_eq!(created["readerCount"], 0);
    let carol_id = created["id"].as_i64().unwrap();

    let (status, _) = request(
        &test_app,
        "POST",
        "/api/admin/users",
        &admin_cookie,
        Some(json!({ "username": "carol", "password": "password123" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // disabling a user revokes login and existing sessions
    let (status, updated) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{carol_id}"),
        &admin_cookie,
        Some(json!({ "disabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["disabled"], true);

    let (status, _) = request(
        &test_app,
        "POST",
        "/api/auth/login",
        "",
        Some(json!({ "username": "carol", "password": "password123" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // an admin cannot disable or demote their own account
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{admin_id}"),
        &admin_cookie,
        Some(json!({ "disabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{admin_id}"),
        &admin_cookie,
        Some(json!({ "role": "user" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let bob_id_early = users.iter().find(|user| user["username"] == "bob").unwrap()["id"]
        .as_i64()
        .unwrap();

    // partial updates preserve the disabled state
    let (status, bob) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{bob_id_early}"),
        &admin_cookie,
        Some(json!({ "displayName": "Bob" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bob["disabled"], false);

    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{bob_id_early}"),
        &admin_cookie,
        Some(json!({ "disabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, bob) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{bob_id_early}"),
        &admin_cookie,
        Some(json!({ "displayName": "Bobby" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bob["disabled"], true, "a partial update must not re-enable");
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{bob_id_early}"),
        &admin_cookie,
        Some(json!({ "disabled": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // bob can be promoted and then re-enabled carol appears in the list
    let (status, promoted) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{bob_id_early}"),
        &admin_cookie,
        Some(json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(promoted["role"], "admin");

    let (_, list) = get_json(&test_app, "/api/admin/users", &admin_cookie).await;
    assert_eq!(list.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn admins_set_ordered_reading_languages() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin", "password123").await;

    let (status, created) = request(
        &test_app,
        "POST",
        "/api/admin/users",
        &admin_cookie,
        Some(json!({
            "username": "kiddo",
            "credential": "246810",
            "credentialType": "pin",
            "role": "user",
            "preferredLanguages": ["sv", "en"],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["preferredLanguages"], json!(["sv", "en"]));

    let kid_id = created["id"].as_i64().unwrap();
    let (first, stored): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT preferred_language, preferred_languages FROM users WHERE id = ?")
            .bind(kid_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(first.as_deref(), Some("sv"), "the first entry is preferred");
    assert_eq!(stored.as_deref(), Some(r#"["sv","en"]"#));

    let (status, updated) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{kid_id}"),
        &admin_cookie,
        Some(json!({ "preferredLanguages": ["en"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["preferredLanguages"], json!(["en"]));
}

#[tokio::test]
async fn create_user_applies_the_profile_type_atomically() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin", "password123").await;

    let (status, created) = request(
        &test_app,
        "POST",
        "/api/admin/users",
        &admin_cookie,
        Some(json!({
            "username": "kid",
            "password": "password123",
            "role": "user",
            "profileType": "child",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, profiles) = get_json(&test_app, "/api/admin/users/profiles", &admin_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        profiles["users"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| { entry["userId"] == created["id"] && entry["profileType"] == "child" }),
        "the created account must be a child immediately: {profiles}"
    );

    let kid_cookie = common::login(&test_app, "kid", "password123").await;
    let (status, _) = get_json(&test_app, "/api/discover/search?q=dune", &kid_cookie).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the child guard must be effective without a follow-up request"
    );

    let (status, _) = request(
        &test_app,
        "POST",
        "/api/admin/users",
        &admin_cookie,
        Some(json!({
            "username": "tinyadmin",
            "password": "password123",
            "role": "admin",
            "profileType": "child",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = request(
        &test_app,
        "POST",
        "/api/admin/users",
        &admin_cookie,
        Some(json!({
            "username": "weird",
            "password": "password123",
            "profileType": "teenager",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
