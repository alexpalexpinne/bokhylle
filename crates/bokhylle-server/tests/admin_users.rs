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

async fn owned_book(test_app: &common::TestApp, key: &str) -> i64 {
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &bokhylle_metadata::MetadataResult {
            provider: "fake".into(),
            provider_key: key.into(),
            title: key.into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
        .bind(book_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, ?, 'epub', 10, ?)")
        .bind(edition_id).bind(format!("/tmp/{key}.epub")).bind(key)
        .execute(&test_app.state.db).await.unwrap();
    book_id
}

#[tokio::test]
async fn child_creation_assigns_only_selected_owned_books_and_preserves_access_modes() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin = common::login(&test_app, "admin", "password123").await;
    let first = owned_book(&test_app, "The Lantern Map").await;
    let second = owned_book(&test_app, "River Letters").await;
    let hidden = owned_book(&test_app, "Hidden Household Book").await;

    for (name, discover, ask) in [
        ("assigned", false, false),
        ("search", false, true),
        ("explore", true, true),
        ("custom", true, false),
    ] {
        let (status, created) = request(&test_app, "POST", "/api/admin/users", &admin, Some(json!({
            "username": name, "credential": "482915", "credentialType": "pin", "profileType": "child",
            "displayName": " Young Reader ", "preferredLanguages": [" EN ", "sv"],
            "canDiscover": discover, "canRequest": ask, "canAcquire": true,
            "startingBookIds": [first, first, second], "avatarPreset": "owl",
        }))).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(created["displayName"], "Young Reader");
        assert_eq!(created["avatarPreset"], "owl");
        assert_eq!(created["preferredLanguages"], json!(["en", "sv"]));
        assert_eq!(created["canDiscover"], discover);
        assert_eq!(created["canRequest"], ask);
        assert_eq!(created["canAcquire"], false);
        let child = common::login(&test_app, name, "482915").await;
        let (status, books) = get_json(&test_app, "/api/books?mine=false", &child).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            books["total"], 2,
            "duplicate starting ids must not create extra shelf rows"
        );
        assert!(
            books["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|book| book["id"] == first || book["id"] == second)
        );
        let (status, _) = get_json(&test_app, &format!("/api/books/{hidden}"), &child).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "unassigned household books stay hidden in every mode"
        );
    }
}

#[tokio::test]
async fn child_creation_rolls_back_settings_and_assignments_on_failure_and_can_retry() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin = common::login(&test_app, "admin", "password123").await;
    let first = owned_book(&test_app, "First Starting Book").await;
    let second = owned_book(&test_app, "Second Starting Book").await;
    let payload = json!({ "username": "newchild", "password": "password123", "profileType": "child",
        "displayName": "New Child", "preferredLanguages": ["en"], "canDiscover": true, "startingBookIds": [first, second] });
    sqlx::query("CREATE TRIGGER fail_second_assignment BEFORE INSERT ON user_books WHEN (SELECT title FROM books WHERE id = NEW.book_id) = 'Second Starting Book' BEGIN SELECT RAISE(FAIL, 'assignment failure'); END")
        .execute(&test_app.state.db).await.unwrap();
    let (status, _) = request(
        &test_app,
        "POST",
        "/api/admin/users",
        &admin,
        Some(payload.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE username = 'newchild'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let shelves: i64 = sqlx::query_scalar("SELECT count(*) FROM user_books")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(
        users, 0,
        "credential and settings writes must roll back with assignments"
    );
    assert_eq!(
        shelves, 0,
        "the first assignment must roll back with the second"
    );
    sqlx::query("DROP TRIGGER fail_second_assignment")
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let (status, created) =
        request(&test_app, "POST", "/api/admin/users", &admin, Some(payload)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the same account can be retried: {created}"
    );

    for (name, profile, ids) in [
        ("missing", "child", vec![999999]),
        ("adult-start", "adult", vec![first]),
    ] {
        let (status, _) = request(&test_app, "POST", "/api/admin/users", &admin, Some(json!({
            "username": name, "password": "password123", "profileType": profile, "startingBookIds": ids,
        }))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE username = ?")
            .bind(name)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
    let child = common::login(&test_app, "newchild", "password123").await;
    let (status, _) = request(&test_app, "POST", "/api/admin/users", &child, Some(json!({
        "username": "unauthorized", "password": "password123", "profileType": "child", "startingBookIds": [first],
    }))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn failed_interest_save_preserves_the_previous_selection() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let reader = common::login(&test_app, "reader", "password123").await;
    let (status, _) = request(
        &test_app,
        "PUT",
        "/api/profile/interests",
        &reader,
        Some(json!({ "subjects": ["Space", "Adventure", "Fantasy"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, initial) = get_json(&test_app, "/api/profile/onboarding", &reader).await;
    assert_eq!(
        initial["interests"],
        json!(["space", "adventure", "fantasy"]),
        "interests saved in one second retain their selection order"
    );
    sqlx::query("CREATE TRIGGER fail_interest BEFORE INSERT ON user_subject_interests WHEN NEW.normalized_name = 'broken' BEGIN SELECT RAISE(FAIL, 'interest failure'); END")
        .execute(&test_app.state.db).await.unwrap();
    let (status, _) = request(
        &test_app,
        "PUT",
        "/api/profile/interests",
        &reader,
        Some(json!({ "subjects": ["Nature", "broken"] })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let (status, profile) = get_json(&test_app, "/api/profile/onboarding", &reader).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        profile["interests"],
        json!(["space", "adventure", "fantasy"]),
        "failed replacements must preserve the previous selection"
    );
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

#[tokio::test]
async fn admins_choose_marks_on_creation_and_edit_without_changing_permissions() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin = common::login(&app, "admin", "password123").await;
    for profile in ["adult", "child"] {
        let (status, created) = request(&app, "POST", "/api/admin/users", &admin, Some(json!({
            "username": profile, "credential": "482915", "credentialType": "pin", "profileType": profile,
            "canDiscover": false, "canRequest": false, "avatarPreset": "book"
        }))).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        assert_eq!(created["avatarPreset"], "book");
        assert!(created["avatarUrl"].is_null());
        let id = created["id"].as_i64().unwrap();
        let path = format!("/api/admin/users/{id}");
        let (status, edited) = request(
            &app,
            "PUT",
            &path,
            &admin,
            Some(json!({ "avatarPreset": "mountain" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(edited["avatarPreset"], "mountain");
        assert_eq!(edited["canDiscover"], created["canDiscover"]);
        assert_eq!(edited["canRequest"], created["canRequest"]);
        let (_, edited) = request(
            &app,
            "PUT",
            &path,
            &admin,
            Some(json!({ "displayName": "Reader" })),
        )
        .await;
        assert_eq!(edited["avatarPreset"], "mountain");
        assert_eq!(
            request(
                &app,
                "PUT",
                &path,
                &admin,
                Some(json!({ "avatarPreset": "invalid", "displayName": "No" }))
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let (_, users) = get_json(&app, "/api/admin/users", &admin).await;
        assert_eq!(
            users
                .as_array()
                .unwrap()
                .iter()
                .find(|user| user["id"] == id)
                .unwrap()["avatarPreset"],
            "mountain"
        );
        let reader = common::login(&app, profile, "482915").await;
        assert_eq!(
            request(
                &app,
                "PUT",
                &path,
                &reader,
                Some(json!({ "avatarPreset": "fox" }))
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert!(
            request(
                &app,
                "PUT",
                &path,
                &admin,
                Some(json!({ "avatarPreset": null }))
            )
            .await
            .1["avatarPreset"]
                .is_null()
        );
    }
    let invalid = request(
        &app,
        "POST",
        "/api/admin/users",
        &admin,
        Some(
            json!({ "username": "invalid", "credential": "password123", "avatarPreset": "../fox" }),
        ),
    )
    .await;
    assert_eq!(invalid.0, StatusCode::UNPROCESSABLE_ENTITY);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = 'invalid'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
