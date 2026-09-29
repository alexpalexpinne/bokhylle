use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use bokhylle_metadata::MetadataResult;
use bokhylle_server::auth::Role;

mod common;

async fn add_book(state: &bokhylle_server::AppState, key: &str, title: &str) -> i64 {
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: key.to_string(),
            title: title.to_string(),
            authors: vec!["Child Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
        .bind(book_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, 'epub', 10, ?)",
    )
    .bind(edition_id)
    .bind(format!("/tmp/child-{key}.epub"))
    .bind(format!("digest-{key}"))
    .execute(&state.db)
    .await
    .unwrap();
    book_id
}

async fn get(
    test_app: &common::TestApp,
    uri: &str,
    cookie: &str,
) -> (StatusCode, serde_json::Value) {
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}

async fn post(test_app: &common::TestApp, uri: &str, cookie: &str) -> StatusCode {
    test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn put(test_app: &common::TestApp, uri: &str, cookie: &str, payload: &str) -> StatusCode {
    test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(uri)
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn children_see_only_their_shelf_and_cannot_download_or_discover() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("kid", "password123", Role::User)
        .await
        .unwrap();
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(kid_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    let on_shelf = add_book(&test_app.state, "/works/OLCHILD1W", "Child Shelf Book").await;
    let other = add_book(&test_app.state, "/works/OLCHILD2W", "Household Book").await;
    bokhylle_server::user_books::add(&test_app.state.db, kid_id, on_shelf, "parent_assigned")
        .await
        .unwrap();

    let kid_cookie = common::login(&test_app, "kid", "password123").await;
    let admin_cookie = common::login(&test_app, "admin", "password123").await;

    // The session reports the child profile.
    let (status, me) = get(&test_app, "/api/auth/me", &kid_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["profileType"], "child");

    // Household browsing is restricted to the shelf.
    let (status, page) = get(&test_app, "/api/books?pageSize=24", &kid_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["title"], "Child Shelf Book");

    let (status, _) = get(&test_app, &format!("/api/books/{other}"), &kid_cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, page) = get(&test_app, "/api/books/search?q=book", &kid_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page.as_array().unwrap().len(), 1);

    // Discovery, acquisition and downloads are refused outright.
    for uri in [
        "/api/discover/search?q=book",
        "/api/acquisitions",
        "/api/collections",
        "/api/authors",
        "/api/home/updates",
    ] {
        let (status, _) = get(&test_app, uri, &kid_cookie).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri} should be forbidden");
    }

    let file_id: i64 = sqlx::query_scalar("SELECT id FROM book_files LIMIT 1")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let (status, _) = get(
        &test_app,
        &format!("/api/books/{on_shelf}/files/{file_id}/download"),
        &kid_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Home is the shelf only.
    let (status, rails) = get(&test_app, "/api/home/rails", &kid_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rails.as_array().unwrap().len(), 1);
    assert_eq!(rails[0]["title"], "My shelf");

    // A parent assigns another book; the child sees it appear.
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/users/{kid_id}/shelf/{other}"))
                .header(header::COOKIE, &admin_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"onShelf":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (_, page) = get(&test_app, "/api/books?pageSize=24", &kid_cookie).await;
    assert_eq!(page["total"], 2);
}

#[tokio::test]
async fn children_cannot_mutate_shelves_or_peek_outside_them() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin2", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("kid2", "password123", Role::User)
        .await
        .unwrap();
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid2'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(kid_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    let assigned = add_book(&test_app.state, "/works/OLCHILD3W", "Assigned Book").await;
    let hidden = add_book(&test_app.state, "/works/OLCHILD4W", "Hidden Book").await;
    bokhylle_server::user_books::add(&test_app.state.db, kid_id, assigned, "parent_assigned")
        .await
        .unwrap();

    let kid_cookie = common::login(&test_app, "kid2", "password123").await;

    // A child cannot curate their own shelf through the API.
    let mutations = [
        ("PUT", format!("/api/books/{hidden}/shelf")),
        ("DELETE", format!("/api/books/{assigned}/shelf")),
        ("POST", "/api/books/shelf/claim-all".to_string()),
    ];
    for (method, uri) in mutations {
        let response = test_app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(&uri)
                    .header(header::COOKIE, &kid_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"onShelf":true,"preference":"liked"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} {uri} must be forbidden for children"
        );
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user_books WHERE user_id = ? AND on_shelf = 1")
            .bind(kid_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(count, 1, "no shelf changes may slip through");

    // Liking is taste, not membership: allowed for an assigned book, and
    // refused for anything outside the shelf without leaking its existence.
    let status = put(
        &test_app,
        &format!("/api/books/{assigned}/preference"),
        &kid_cookie,
        r#"{"preference":"liked"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (on_shelf, preference): (i64, Option<String>) = sqlx::query_as(
        "SELECT on_shelf, preference FROM user_books WHERE user_id = ? AND book_id = ?",
    )
    .bind(kid_id)
    .bind(assigned)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(on_shelf, 1, "liking must not change shelf membership");
    assert_eq!(preference.as_deref(), Some("liked"));

    let status = put(
        &test_app,
        &format!("/api/books/{hidden}/preference"),
        &kid_cookie,
        r#"{"preference":"liked"}"#,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a child may not like books outside their shelf"
    );

    // Household facets are not a child's business, and covers follow the shelf.
    let (status, _) = get(&test_app, "/api/books/facets", &kid_cookie).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = get(
        &test_app,
        &format!("/api/books/{hidden}/cover"),
        &kid_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(
        &test_app,
        &format!("/api/books/{assigned}/cover"),
        &kid_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn changing_profile_type_resets_onboarding_and_admins_can_restart_it() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin3", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("kid3", "password123", Role::User)
        .await
        .unwrap();
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid3'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin3", "password123").await;

    sqlx::query("UPDATE users SET onboarded_at = datetime('now') WHERE id = ?")
        .bind(kid_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    // Converting an account resets onboarding so the matching wizard runs.
    let status = put(
        &test_app,
        &format!("/api/admin/users/{kid_id}/profile-type"),
        &admin_cookie,
        r#"{"profileType":"child"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let onboarded: Option<String> =
        sqlx::query_scalar("SELECT onboarded_at FROM users WHERE id = ?")
            .bind(kid_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert!(onboarded.is_none(), "converting must reset onboarding");

    // Setting the same type again is a no-op, not a reset.
    sqlx::query("UPDATE users SET onboarded_at = datetime('now') WHERE id = ?")
        .bind(kid_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let status = put(
        &test_app,
        &format!("/api/admin/users/{kid_id}/profile-type"),
        &admin_cookie,
        r#"{"profileType":"child"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let onboarded: Option<String> =
        sqlx::query_scalar("SELECT onboarded_at FROM users WHERE id = ?")
            .bind(kid_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert!(onboarded.is_some(), "an unchanged type must not reset");

    // Admins can restart setup explicitly.
    let status = post(
        &test_app,
        &format!("/api/admin/users/{kid_id}/restart-onboarding"),
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let onboarded: Option<String> =
        sqlx::query_scalar("SELECT onboarded_at FROM users WHERE id = ?")
            .bind(kid_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert!(onboarded.is_none(), "restart must clear onboarding");
}

#[tokio::test]
async fn converting_an_adult_to_a_child_clears_shelf_membership_but_keeps_taste() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin4", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("convert", "password123", Role::User)
        .await
        .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'convert'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin4", "password123").await;

    let shelf_book = add_book(&test_app.state, "/works/OLCONVERT1W", "Convert Shelf Book").await;
    let liked_book = add_book(&test_app.state, "/works/OLCONVERT2W", "Convert Liked Book").await;
    bokhylle_server::user_books::add(&test_app.state.db, user_id, shelf_book, "manual")
        .await
        .unwrap();
    bokhylle_server::user_books::set_preference(
        &test_app.state.db,
        user_id,
        liked_book,
        Some("liked"),
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_subject_interests (user_id, normalized_name) VALUES (?, 'fantasy')",
    )
    .bind(user_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();
    sqlx::query("UPDATE users SET onboarded_at = datetime('now') WHERE id = ?")
        .bind(user_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    // Background adult capabilities exist before the conversion.
    let author_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('Convert Author', 'convert author')
         RETURNING id",
    )
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO author_follows (user_id, author_id, auto_acquire, baseline_at)
         VALUES (?, ?, 1, unixepoch())",
    )
    .bind(user_id)
    .bind(author_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();
    let (acquisition, _) = bokhylle_server::acquisition::create(
        &test_app.state.db,
        shelf_book,
        Some(user_id),
        None,
        None,
        true,
        false,
    )
    .await
    .unwrap();

    let status = put(
        &test_app,
        &format!("/api/admin/users/{user_id}/profile-type"),
        &admin_cookie,
        r#"{"profileType":"child"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Automation and pending delivery intents must not survive: a converted
    // child cannot receive background acquisitions.
    let (auto_acquire, delivery_target): (i64, Option<i64>) = sqlx::query_as(
        "SELECT auto_acquire, delivery_target_id FROM author_follows
         WHERE user_id = ? AND author_id = ?",
    )
    .bind(user_id)
    .bind(author_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(auto_acquire, 0, "conversion must disable author automation");
    assert!(delivery_target.is_none());
    let pending_intents: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM acquisition_requests
         WHERE acquisition_id = ? AND user_id = ? AND deliver_on_ready = 1",
    )
    .bind(&acquisition.id)
    .bind(user_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(
        pending_intents, 0,
        "conversion must clear pending delivery intents"
    );

    // A former adult's self-curated shelf is not parent approval.
    let (on_shelf, preference): (i64, Option<String>) = sqlx::query_as(
        "SELECT on_shelf, preference FROM user_books WHERE user_id = ? AND book_id = ?",
    )
    .bind(user_id)
    .bind(shelf_book)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(on_shelf, 0, "conversion must clear shelf membership");
    assert_eq!(preference, None);

    // Taste survives: the like stays off-shelf, and interests are untouched.
    let (liked_shelf, liked_preference): (i64, Option<String>) = sqlx::query_as(
        "SELECT on_shelf, preference FROM user_books WHERE user_id = ? AND book_id = ?",
    )
    .bind(user_id)
    .bind(liked_book)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(liked_shelf, 0);
    assert_eq!(liked_preference.as_deref(), Some("liked"));
    let interests: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user_subject_interests WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(interests, 1, "interests must survive the conversion");

    // admin <=> adult holds on every mutation path.
    let admin_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'admin4'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let status = put(
        &test_app,
        &format!("/api/admin/users/{admin_id}/profile-type"),
        &admin_cookie,
        r#"{"profileType":"child"}"#,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an administrator cannot be converted to a child"
    );
    let status = put(
        &test_app,
        &format!("/api/admin/users/{user_id}"),
        &admin_cookie,
        r#"{"role":"admin"}"#,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a child cannot be promoted to administrator"
    );
}

#[tokio::test]
async fn an_active_adult_session_loses_adult_access_immediately_after_conversion() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin5", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("switcher", "password123", Role::User)
        .await
        .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'switcher'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin5", "password123").await;
    let user_cookie = common::login(&test_app, "switcher", "password123").await;

    let (status, _) = get(&test_app, "/api/books/facets", &user_cookie).await;
    assert_eq!(status, StatusCode::OK, "an adult session may browse facets");

    let status = put(
        &test_app,
        &format!("/api/admin/users/{user_id}/profile-type"),
        &admin_cookie,
        r#"{"profileType":"child"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The existing cookie is re-evaluated per request: no re-login needed.
    let (status, _) = get(&test_app, "/api/books/facets", &user_cookie).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the same session must lose adult access immediately"
    );
    let (status, _) = get(&test_app, "/api/books?mine=true", &user_cookie).await;
    assert_eq!(status, StatusCode::OK, "own-shelf browsing stays available");
}

#[tokio::test]
async fn restart_onboarding_is_admin_only() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin6", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("plain", "password123", Role::User)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("kid6", "password123", Role::User)
        .await
        .unwrap();
    let plain_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'plain'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid6'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(kid_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin6", "password123").await;
    let plain_cookie = common::login(&test_app, "plain", "password123").await;
    let kid_cookie = common::login(&test_app, "kid6", "password123").await;

    let uri = format!("/api/admin/users/{plain_id}/restart-onboarding");
    let status = post(&test_app, &uri, "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no session, no restart");
    let status = post(&test_app, &uri, &plain_cookie).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a member may not restart setup"
    );
    let status = post(&test_app, &uri, &kid_cookie).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a child may not restart setup"
    );

    sqlx::query("UPDATE users SET onboarded_at = datetime('now') WHERE id = ?")
        .bind(plain_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let status = post(&test_app, &uri, &admin_cookie).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let onboarded: Option<String> =
        sqlx::query_scalar("SELECT onboarded_at FROM users WHERE id = ?")
            .bind(plain_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert!(onboarded.is_none());
}

#[tokio::test]
async fn children_cannot_change_profile_settings() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin7", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("kid7", "password123", Role::User)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin7", "password123").await;
    let kid_cookie = common::login(&test_app, "kid7", "password123").await;
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid7'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let status = put(
        &test_app,
        &format!("/api/admin/users/{kid_id}/profile-type"),
        &admin_cookie,
        r#"{"profileType":"child"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // A child controls likes and interests, not account configuration.
    let status = put(
        &test_app,
        "/api/profile",
        &kid_cookie,
        r#"{"displayName":"Sneaky","acquisitionMode":"ask"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let display_name: Option<String> =
        sqlx::query_scalar("SELECT display_name FROM users WHERE id = ?")
            .bind(kid_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert!(display_name.is_none(), "nothing may be applied");
}

#[tokio::test]
async fn children_can_like_but_not_dislike() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin8", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("kid8", "password123", Role::User)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin8", "password123").await;
    let kid_cookie = common::login(&test_app, "kid8", "password123").await;
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid8'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let status = put(
        &test_app,
        &format!("/api/admin/users/{kid_id}/profile-type"),
        &admin_cookie,
        r#"{"profileType":"child"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let book = add_book(&test_app.state, "/works/OLKIDPREFW", "Kid Preference Book").await;
    bokhylle_server::user_books::add(&test_app.state.db, kid_id, book, "parent_assigned")
        .await
        .unwrap();

    let status = put(
        &test_app,
        &format!("/api/books/{book}/preference"),
        &kid_cookie,
        r#"{"preference":"liked"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let status = put(
        &test_app,
        &format!("/api/books/{book}/preference"),
        &kid_cookie,
        r#"{"preference":"not_for_me"}"#,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the child product only exposes Like"
    );

    let status = put(
        &test_app,
        &format!("/api/books/{book}/preference"),
        &kid_cookie,
        r#"{"preference":null}"#,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
