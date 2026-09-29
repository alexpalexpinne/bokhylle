use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_metadata::MetadataResult;
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

async fn add_book(state: &bokhylle_server::AppState, key: &str, title: &str) -> i64 {
    bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: key.to_string(),
            title: title.to_string(),
            authors: vec!["Household Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

async fn add_file(state: &bokhylle_server::AppState, book_id: i64, digest: &str) {
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
    .bind(format!("/tmp/household-{digest}.epub"))
    .bind(digest)
    .execute(&state.db)
    .await
    .unwrap();
}

async fn user_id(test_app: &common::TestApp, username: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap()
}

async fn app() -> common::TestApp {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("parent", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("dad", "password123", Role::User)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("emma", "password123", Role::User)
        .await
        .unwrap();
    let emma_id = user_id(&test_app, "emma").await;
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(emma_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    test_app
}

#[tokio::test]
async fn administrator_can_set_up_a_child_reader_without_cross_profile_access() {
    let test_app = app().await;
    let parent = common::login(&test_app, "parent", "password123").await;
    let dad = common::login(&test_app, "dad", "password123").await;
    let emma = common::login(&test_app, "emma", "password123").await;
    let emma_id = user_id(&test_app, "emma").await;
    let dad_id = user_id(&test_app, "dad").await;
    let readers = format!("/api/admin/children/{emma_id}/readers");

    let (status, _) = request(&test_app, "GET", &readers, &dad, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(&test_app, "GET", &readers, &emma, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &test_app,
        "GET",
        &format!("/api/admin/children/{dad_id}/readers"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, first) = request(
        &test_app,
        "POST",
        &readers,
        &parent,
        Some(json!({"name":"Emma Kindle","address":"emma@kindle.example","connector":"email","deviceType":"kindle"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(first["userId"], emma_id);
    assert_eq!(first["isDefault"], true);
    let first_id = first["id"].as_i64().unwrap();
    let (address, source) = bokhylle_server::delivery::default_reader(&test_app.state, emma_id)
        .await
        .unwrap();
    assert_eq!(address.as_deref(), Some("emma@kindle.example"));
    assert_eq!(source, "personal");

    let (status, _) = request(&test_app, "GET", "/api/delivery-targets", &emma, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, configured) = request(&test_app, "GET", &readers, &parent, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(configured[0]["id"], first_id);

    let (status, second) = request(
        &test_app,
        "POST",
        &readers,
        &parent,
        Some(json!({"name":"Emma PocketBook","address":"emma@pbsync.example","connector":"email","deviceType":"pocketbook"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let second_id = second["id"].as_i64().unwrap();
    let (status, _) = request(
        &test_app,
        "POST",
        &format!("{readers}/{second_id}/default"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (address, _) = bokhylle_server::delivery::default_reader(&test_app.state, emma_id)
        .await
        .unwrap();
    assert_eq!(address.as_deref(), Some("emma@pbsync.example"));

    let other = test_app
        .state
        .auth
        .create_user("otherchild", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(other.id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/children/{}/readers/{first_id}", other.id),
        &parent,
        Some(json!({"address":"wrong@kindle.example"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let tokens = format!("/api/admin/children/{emma_id}/reader-tokens");
    let (status, created) = request(
        &test_app,
        "POST",
        &tokens,
        &parent,
        Some(json!({"name":"Emma KOReader"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(created["token"].as_str().unwrap().len() > 30);
    let token_id = created["id"].as_i64().unwrap();
    let (status, listed) = request(&test_app, "GET", &tokens, &parent, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["tokens"][0]["name"], "Emma KOReader");
    assert!(listed["tokens"][0].get("token").is_none());
    let (status, _) = request(&test_app, "GET", &tokens, &dad, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &test_app,
        "DELETE",
        &format!("{tokens}/{token_id}"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn adult_shelves_are_private_and_only_admins_manage_children() {
    let test_app = app().await;
    let parent = common::login(&test_app, "parent", "password123").await;
    let dad = common::login(&test_app, "dad", "password123").await;
    let emma = common::login(&test_app, "emma", "password123").await;
    let emma_id = user_id(&test_app, "emma").await;
    let dad_id = user_id(&test_app, "dad").await;

    let first = add_book(&test_app.state, "/works/OLHOUSE1W", "House One").await;
    let second = add_book(&test_app.state, "/works/OLHOUSE2W", "House Two").await;
    add_file(&test_app.state, first, "house-one-digest").await;
    add_file(&test_app.state, second, "house-two-digest").await;
    bokhylle_server::user_books::add(&test_app.state.db, emma_id, first, "parent_assigned")
        .await
        .unwrap();
    bokhylle_server::user_books::add(&test_app.state.db, dad_id, second, "manual")
        .await
        .unwrap();

    // Only administrators learn which child shelves exist.
    let (status, _) = request(&test_app, "GET", "/api/household/members", &dad, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, members) =
        request(&test_app, "GET", "/api/household/members", &parent, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(members["members"].as_array().unwrap().len(), 1);
    assert_eq!(members["members"][0]["username"], "emma");
    let (status, _) = request(&test_app, "GET", "/api/household/members", &emma, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The admin can inspect a child, while another adult cannot use direct
    // scope parameters to inspect either that child or another adult.
    let (status, page) = request(
        &test_app,
        "GET",
        &format!("/api/books?user={emma_id}"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1, "Emma's shelf holds one book: {page}");
    assert_eq!(page["items"][0]["title"], "House One");
    let (status, results) = request(
        &test_app,
        "GET",
        &format!("/api/books/search?q=House&user={emma_id}"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(results.as_array().unwrap().len(), 1);
    for path in [
        format!("/api/books?user={emma_id}"),
        format!("/api/books/search?q=House&user={emma_id}"),
        format!("/api/books/facets?user={emma_id}"),
        format!("/api/authors?user={emma_id}"),
    ] {
        let (status, _) = request(&test_app, "GET", &path, &dad, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
    let (status, _) = request(
        &test_app,
        "GET",
        &format!("/api/books?user={dad_id}"),
        &parent,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "even admins cannot browse an adult shelf"
    );
    let (status, own) = request(
        &test_app,
        "GET",
        &format!("/api/books?user={dad_id}"),
        &dad,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(own["total"], 1);

    // A child passing another member's id still reads only their own shelf.
    let (status, page) = request(
        &test_app,
        "GET",
        &format!("/api/books?user={dad_id}"),
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1, "children stay self-scoped: {page}");
    assert_eq!(page["items"][0]["title"], "House One");

    // A reader cannot assign a child; an admin can.
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/users/{emma_id}/shelf/{second}"),
        &dad,
        Some(json!({ "onShelf": true })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/users/{emma_id}/shelf/{second}"),
        &parent,
        Some(json!({ "onShelf": true })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let on_shelf: i64 =
        sqlx::query_scalar("SELECT on_shelf FROM user_books WHERE user_id = ? AND book_id = ?")
            .bind(emma_id)
            .bind(second)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(on_shelf, 1);
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/users/{dad_id}/shelf/{second}"),
        &dad,
        Some(json!({ "onShelf": true })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a non-admin adult cannot assign an adult's shelf"
    );
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/users/{emma_id}/shelf/{second}"),
        &emma,
        Some(json!({ "onShelf": false })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "children never self-curate");

    // Child assignments are visible only to the administrator.
    let (status, _) = request(
        &test_app,
        "GET",
        &format!("/api/books/{second}/shelf-users"),
        &dad,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, shelf) = request(
        &test_app,
        "GET",
        &format!("/api/books/{second}/shelf-users"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shelf["users"].as_array().unwrap().len(), 1);
    assert_eq!(shelf["users"][0]["displayName"], "emma");
    assert_eq!(shelf["users"][0]["onShelf"], true);
    let (status, _) = request(
        &test_app,
        "GET",
        &format!("/api/books/{second}/shelf-users"),
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Admins use the personal-shelf endpoint for their own books; this path
    // cannot modify another adult's shelf.
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/users/{dad_id}/shelf/{second}"),
        &parent,
        Some(json!({ "onShelf": true })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn reader_without_acquisition_permission_can_use_owned_books_but_not_add_files() {
    let test_app = app().await;
    let parent = common::login(&test_app, "parent", "password123").await;
    let dad = common::login(&test_app, "dad", "password123").await;
    let dad_id = user_id(&test_app, "dad").await;
    let owned = add_book(&test_app.state, "/works/OLOWNED", "Owned Book").await;
    let missing = add_book(&test_app.state, "/works/OLMISSING", "Missing Book").await;
    add_file(&test_app.state, owned, "owned-digest").await;

    let (status, updated) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{dad_id}"),
        &parent,
        Some(json!({ "canAcquire": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["canAcquire"], false);
    let (status, me) = request(&test_app, "GET", "/api/auth/me", &dad, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["canAcquire"], false);

    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/books/{owned}/shelf"),
        &dad,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, own) = request(&test_app, "GET", "/api/books?mine=true", &dad, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(own["total"], 1);

    let (status, _) = request(
        &test_app,
        "POST",
        &format!("/api/books/{missing}/acquisitions"),
        &dad,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request(
        &test_app,
        "POST",
        "/api/discover/acquisitions",
        &dad,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLMISSING" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let acquisitions: i64 = sqlx::query_scalar("SELECT count(*) FROM acquisitions")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(acquisitions, 0);
}

#[tokio::test]
async fn converting_a_child_to_an_adult_keeps_the_selected_acquisition_permission() {
    let test_app = app().await;
    let parent = common::login(&test_app, "parent", "password123").await;
    let emma_id = user_id(&test_app, "emma").await;

    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{emma_id}"),
        &parent,
        Some(json!({ "canAcquire": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{emma_id}/profile-type"),
        &parent,
        Some(json!({ "profileType": "adult" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, users) = request(&test_app, "GET", "/api/admin/users", &parent, None).await;
    assert_eq!(status, StatusCode::OK);
    let emma = users
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["id"] == emma_id)
        .unwrap();
    let profile: String = sqlx::query_scalar("SELECT profile_type FROM users WHERE id = ?")
        .bind(emma_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(profile, "adult");
    assert_eq!(emma["canAcquire"], false);
}
