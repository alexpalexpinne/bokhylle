mod common;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use bokhylle_metadata::MetadataResult;
use bokhylle_server::auth::Role;
use bokhylle_server::services::sharing::{self, BookSharing};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(
    app: &common::TestApp,
    method: &str,
    path: &str,
    cookie: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
    let body = payload
        .map(|value| Body::from(value.to_string()))
        .unwrap_or_else(Body::empty);
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn book(app: &common::TestApp, key: &str) -> (i64, i64) {
    let id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &MetadataResult {
            provider: "fake".into(),
            provider_key: key.into(),
            title: format!("Fictional {key}"),
            authors: vec![format!("Author {key}")],
            language: Some("sv".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let edition: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let path = app.state.paths.library_root.join(format!("{key}.epub"));
    tokio::fs::write(&path, b"fictional book content")
        .await
        .unwrap();
    let file = sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, ?, 'epub', 22, ?)")
        .bind(edition).bind(path.to_str().unwrap()).bind(key).execute(&app.state.db).await.unwrap().last_insert_rowid();
    (id, file)
}

#[tokio::test]
async fn borrowing_never_grants_sharing_controls_or_private_access() {
    let app = common::test_app().await;
    let owner = app
        .state
        .auth
        .create_user("owner", "password123", Role::User)
        .await
        .unwrap();
    let borrower = app
        .state
        .auth
        .create_user("borrower", "password123", Role::User)
        .await
        .unwrap();
    let child = app
        .state
        .auth
        .create_user("child", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(child.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (id, file) = book(&app, "borrowed").await;
    sharing::choose(&app.state.db, owner.id, id, None)
        .await
        .unwrap();
    let borrower_cookie = common::login(&app, "borrower", "password123").await;
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("/api/books/{id}/shelf"),
            &borrower_cookie,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        sharing::state(&app.state.db, borrower.id, id)
            .await
            .unwrap()
            .sharing,
        None
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("/api/books/{id}/sharing"),
            &borrower_cookie,
            Some(json!({"sharing":"private"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    bokhylle_server::user_books::add(&app.state.db, child.id, id, "parent_assigned")
        .await
        .unwrap();
    sharing::set(&app.state.db, &owner, &[id], BookSharing::Private)
        .await
        .unwrap();
    for path in [
        format!("/api/books/{id}"),
        format!("/api/books/{id}/files/{file}/download"),
    ] {
        assert_eq!(
            request(&app, "GET", &path, &borrower_cookie, None).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(&app, "GET", "/api/books?mine=true", &borrower_cookie, None)
            .await
            .1["total"],
        0
    );
    assert!(
        sharing::can_access(&app.state.db, child.id, id)
            .await
            .unwrap()
    );
    assert!(
        sharing::can_access(&app.state.db, owner.id, id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn scheduled_sends_are_requester_scoped_and_keep_the_selected_address() {
    let app = common::test_app().await;
    let owner = app
        .state
        .auth
        .create_user("owner", "password123", Role::User)
        .await
        .unwrap();
    let other = app
        .state
        .auth
        .create_user("other", "password123", Role::Admin)
        .await
        .unwrap();
    let child = app
        .state
        .auth
        .create_user("child", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(child.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (id, _) = book(&app, "scheduled").await;
    let (acquisition, _) = bokhylle_server::acquisition::create(
        &app.state.db,
        id,
        Some(owner.id),
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();
    let target = sqlx::query("INSERT INTO delivery_targets (user_id, type, name, address, is_default) VALUES (?, 'other', 'Original reader', 'original@example.test', 1)")
        .bind(owner.id).execute(&app.state.db).await.unwrap().last_insert_rowid();
    let foreign_target = sqlx::query("INSERT INTO delivery_targets (user_id, type, name, address) VALUES (?, 'other', 'Other reader', 'other@example.test')")
        .bind(other.id).execute(&app.state.db).await.unwrap().last_insert_rowid();
    let owner_cookie = common::login(&app, "owner", "password123").await;
    let other_cookie = common::login(&app, "other", "password123").await;
    let child_cookie = common::login(&app, "child", "password123").await;
    let path = format!("/api/acquisitions/{}/delivery", acquisition.id);
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            &other_cookie,
            Some(json!({"enabled":true,"targetId":foreign_target}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            &child_cookie,
            Some(json!({"enabled":true}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            &owner_cookie,
            Some(json!({"enabled":true,"targetId":foreign_target}))
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, view) = request(
        &app,
        "PUT",
        &path,
        &owner_cookie,
        Some(json!({"enabled":true,"targetId":target})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["requestedByMe"], true);
    assert_eq!(view["scheduledDeliveryAddress"], "original@example.test");
    let other_view = request(
        &app,
        "GET",
        &format!("/api/acquisitions/{}", acquisition.id),
        &other_cookie,
        None,
    )
    .await
    .1;
    assert_eq!(other_view["scheduledDeliveryAddress"], Value::Null);
    assert_eq!(other_view["deliverOnReady"], false);
    sqlx::query("UPDATE delivery_targets SET address = 'changed@example.test' WHERE id = ?")
        .bind(target)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("DELETE FROM delivery_targets WHERE id = ?")
        .bind(target)
        .execute(&app.state.db)
        .await
        .unwrap();
    let intents =
        bokhylle_server::acquisition_requests::pending_deliveries(&app.state.db, &acquisition.id)
            .await
            .unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(
        intents[0].delivery_address.as_deref(),
        Some("original@example.test")
    );
    assert_eq!(intents[0].delivery_target_id, None);
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            &owner_cookie,
            Some(json!({"enabled":false}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        bokhylle_server::acquisition_requests::pending_deliveries(&app.state.db, &acquisition.id)
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("UPDATE acquisitions SET status = 'READY' WHERE id = ?")
        .bind(&acquisition.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "PUT",
            &path,
            &owner_cookie,
            Some(json!({"enabled":false}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn private_books_are_hidden_across_http_opds_and_shared_services() {
    let app = common::test_app().await;
    let owner = app
        .state
        .auth
        .create_user("owner", "password123", Role::User)
        .await
        .unwrap();
    let other = app
        .state
        .auth
        .create_user("other", "password123", Role::Admin)
        .await
        .unwrap();
    let owner_cookie = common::login(&app, "owner", "password123").await;
    let other_cookie = common::login(&app, "other", "password123").await;
    let (id, file) = book(&app, "hidden").await;
    sharing::choose(&app.state.db, owner.id, id, None)
        .await
        .unwrap();
    bokhylle_server::user_books::add(&app.state.db, owner.id, id, "manual")
        .await
        .unwrap();
    let (status, state) = request(
        &app,
        "PUT",
        &format!("/api/books/{id}/sharing"),
        &owner_cookie,
        Some(json!({"sharing":"private"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["sharedInHousehold"], false);
    let author: i64 = sqlx::query_scalar("SELECT author_id FROM book_authors WHERE book_id = ?")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    for path in [
        format!("/api/books/{id}"),
        format!("/api/books/{id}/cover"),
        format!("/api/books/{id}/related"),
        format!("/api/books/{id}/files/{file}/download"),
        format!("/api/books/{id}/files/{file}/content"),
        format!("/api/books/{id}/files/{file}/position"),
        format!("/api/books/{id}/collections"),
        format!("/api/books/{id}/shelf-users"),
        format!("/api/authors/{author}"),
        format!("/api/authors/{author}/profile"),
        format!("/api/authors/{author}/photo"),
    ] {
        assert_eq!(
            request(&app, "GET", &path, &other_cookie, None).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("/api/books/{id}/shelf"),
            &other_cookie,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for path in ["/api/books", "/api/books?mine=true"] {
        let (status, page) = request(&app, "GET", path, &other_cookie, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["total"], 0);
    }
    for path in [
        "/api/books/search?q=Fictional",
        "/api/books/recent?scope=household",
        "/api/books/highlights?scope=household",
        "/api/authors?scope=household",
    ] {
        let (status, data) = request(&app, "GET", path, &other_cookie, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(data.as_array().unwrap().len(), 0, "{path}: {data}");
    }
    let (_, facets) = request(
        &app,
        "GET",
        "/api/books/facets?scope=household",
        &other_cookie,
        None,
    )
    .await;
    assert!(facets["languages"].as_array().unwrap().is_empty());
    assert!(
        bokhylle_server::services::books::get(&app.state, &other, id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        bokhylle_server::services::books::get(&app.state, &owner, id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        bokhylle_server::services::reader::readable_file(&app.state, &other, id, file)
            .await
            .is_err()
    );
    assert!(
        bokhylle_server::services::reader::readable_file(&app.state, &owner, id, file)
            .await
            .is_ok()
    );
    assert!(
        bokhylle_server::services::books::add(&app.state, &other, id, false)
            .await
            .is_err()
    );
    assert!(
        bokhylle_server::services::delivery::send_book(&app.state, &other, id, None)
            .await
            .is_err()
    );
    let (_, token) = bokhylle_server::reader_tokens::create(&app.state.db, other.id, "reader")
        .await
        .unwrap();
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("reader:{token}"))
    );
    let path: String = sqlx::query_scalar("SELECT path FROM book_files WHERE id = ?")
        .bind(file)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let document = bokhylle_server::partial_md5(std::path::Path::new(&path)).unwrap();
    use md5::Digest;
    let key = hex::encode(md5::Md5::digest(token.as_bytes()));
    assert!(
        bokhylle_server::reader_tokens::register_sync(&app.state.db, &key, "other")
            .await
            .unwrap()
    );
    for (method, uri, body) in [
        ("GET", format!("/syncs/progress/{document}"), Body::empty()),
        (
            "PUT",
            "/syncs/progress".into(),
            Body::from(json!({"document":document,"progress":"1","percentage":0.2}).to_string()),
        ),
    ] {
        let response = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("x-auth-user", "other")
                    .header("x-auth-key", &key)
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "KOReader must not grant access to a private file"
        );
    }
    for path in [
        "/opds/all".to_string(),
        "/opds/recent".into(),
        "/opds/authors".into(),
        format!("/opds/books/{id}/download"),
    ] {
        let response = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&path)
                    .header(header::AUTHORIZATION, &basic)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        if path.ends_with("download") {
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        } else {
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert!(
                !String::from_utf8_lossy(&bytes).contains("hidden"),
                "{path}"
            );
        }
    }
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/api/books?user={}", owner.id),
            &other_cookie,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/api/books/{id}"),
            &owner_cookie,
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    bokhylle_server::user_books::claim_all(&app.state.db, other.id)
        .await
        .unwrap();
    assert!(
        !bokhylle_server::user_books::contains(&app.state.db, other.id, id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn approval_uses_borrowing_for_local_files_and_saved_sharing_for_acquisitions() {
    let app = common::test_app().await;
    let owner = app
        .state
        .auth
        .create_user("requester", "password123", Role::User)
        .await
        .unwrap();
    let admin = app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    for has_file in [true, false] {
        sqlx::query("UPDATE users SET default_book_sharing = 'private', preferred_languages = '[\"sv\"]' WHERE id = ?")
            .bind(owner.id).execute(&app.state.db).await.unwrap();
        let (id, file) = book(
            &app,
            if has_file {
                "ready-request"
            } else {
                "queued-request"
            },
        )
        .await;
        if !has_file {
            sqlx::query("DELETE FROM book_files WHERE id = ?")
                .bind(file)
                .execute(&app.state.db)
                .await
                .unwrap();
        }
        let outcome = bokhylle_server::services::requests::create_for_book(&app.state, &owner, id)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET default_book_sharing = 'shared' WHERE id = ?")
            .bind(owner.id)
            .execute(&app.state.db)
            .await
            .unwrap();
        bokhylle_server::services::requests::approve(&app.state, &admin, outcome.request.id)
            .await
            .unwrap();
        assert_eq!(
            sharing::state(&app.state.db, owner.id, id)
                .await
                .unwrap()
                .sharing,
            if has_file {
                None
            } else {
                Some(BookSharing::Private)
            }
        );
        assert_eq!(
            sharing::can_access(&app.state.db, admin.id, id)
                .await
                .unwrap(),
            has_file
        );
        assert!(
            bokhylle_server::user_books::contains(&app.state.db, owner.id, id)
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn hidden_collections_and_child_assignments_cannot_grant_access_by_guessing_ids() {
    let app = common::test_app().await;
    let owner = app
        .state
        .auth
        .create_user("owner", "password123", Role::User)
        .await
        .unwrap();
    let admin = app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&app, "admin", "password123").await;
    let (hidden, _) = book(&app, "hidden-collection").await;
    let (public, _) = book(&app, "public-collection").await;
    sharing::choose(&app.state.db, owner.id, hidden, None)
        .await
        .unwrap();
    bokhylle_server::user_books::add(&app.state.db, owner.id, hidden, "manual")
        .await
        .unwrap();
    sharing::set(&app.state.db, &owner, &[hidden], BookSharing::Private)
        .await
        .unwrap();
    let collection =
        bokhylle_server::collections::create(&app.state.db, "Fictional private collection")
            .await
            .unwrap();
    bokhylle_server::collections::add_book(&app.state.db, collection.id, hidden)
        .await
        .unwrap();
    assert!(
        request(&app, "GET", "/api/collections", &cookie, None)
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    for (method, uri, body) in [
        ("GET", format!("/api/collections/{}", collection.id), None),
        (
            "DELETE",
            format!("/api/collections/{}", collection.id),
            None,
        ),
        (
            "POST",
            format!("/api/collections/{}/books", collection.id),
            Some(json!({"bookId":public})),
        ),
    ] {
        assert_eq!(
            request(&app, method, &uri, &cookie, body).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let (status, response) = request(&app, "POST", "/api/admin/users", &cookie,
        Some(json!({"username":"newchild","credential":"482617","credentialType":"pin","profileType":"child","startingBookIds":[hidden]}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{response}");
    assert!(
        !sharing::can_access(&app.state.db, admin.id, hidden)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn request_overrides_are_saved_once_and_children_always_keep_private_access() {
    let app = common::test_app().await;
    let owner = app
        .state
        .auth
        .create_user("requester", "password123", Role::User)
        .await
        .unwrap();
    let (id, _) = book(&app, "request-override").await;
    let (request_id, duplicate) = bokhylle_server::book_requests::create_with_sharing(
        &app.state.db,
        id,
        owner.id,
        Some(BookSharing::Private),
    )
    .await
    .unwrap();
    assert!(!duplicate);
    let (again, duplicate) = bokhylle_server::book_requests::create_with_sharing(
        &app.state.db,
        id,
        owner.id,
        Some(BookSharing::Shared),
    )
    .await
    .unwrap();
    assert_eq!(again, request_id);
    assert!(duplicate);
    let saved: String = sqlx::query_scalar("SELECT sharing FROM book_requests WHERE id = ?")
        .bind(request_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(saved, "private");
    assert!(
        sharing::state(&app.state.db, owner.id, id)
            .await
            .unwrap()
            .sharing
            .is_none(),
        "unapproved requests do not grant access"
    );

    let child = app
        .state
        .auth
        .create_user("child", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(child.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (request_id, _) = bokhylle_server::book_requests::create_with_sharing(
        &app.state.db,
        id,
        child.id,
        Some(BookSharing::Shared),
    )
    .await
    .unwrap();
    let saved: String = sqlx::query_scalar("SELECT sharing FROM book_requests WHERE id = ?")
        .bind(request_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(saved, "private");
}

#[tokio::test]
async fn sharing_preserves_coowners_and_survives_shelf_removal_and_account_deletion() {
    let app = common::test_app().await;
    let first = app
        .state
        .auth
        .create_user("first", "password123", Role::User)
        .await
        .unwrap();
    let second = app
        .state
        .auth
        .create_user("second", "password123", Role::User)
        .await
        .unwrap();
    let third = app
        .state
        .auth
        .create_user("third", "password123", Role::User)
        .await
        .unwrap();
    let (id, _) = book(&app, "coowned").await;
    for user in [&first, &second] {
        sharing::choose(&app.state.db, user.id, id, None)
            .await
            .unwrap();
        bokhylle_server::user_books::add(&app.state.db, user.id, id, "manual")
            .await
            .unwrap();
    }
    sharing::set(&app.state.db, &first, &[id], BookSharing::Private)
        .await
        .unwrap();
    assert!(
        sharing::can_access(&app.state.db, third.id, id)
            .await
            .unwrap()
    );
    sharing::set(&app.state.db, &second, &[id], BookSharing::Private)
        .await
        .unwrap();
    assert!(
        !sharing::can_access(&app.state.db, third.id, id)
            .await
            .unwrap()
    );
    for user in [&first, &second] {
        assert!(
            sharing::can_access(&app.state.db, user.id, id)
                .await
                .unwrap()
        );
    }
    bokhylle_server::user_books::remove(&app.state.db, first.id, id)
        .await
        .unwrap();
    assert!(
        sharing::can_access(&app.state.db, first.id, id)
            .await
            .unwrap()
    );
    assert!(
        !sharing::can_access(&app.state.db, third.id, id)
            .await
            .unwrap()
    );
    for user in [&first, &second] {
        sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(user.id)
            .execute(&app.state.db)
            .await
            .unwrap();
    }
    assert!(
        !sharing::can_access(&app.state.db, third.id, id)
            .await
            .unwrap(),
        "removing the final owner must not publish a private book"
    );
}

#[tokio::test]
async fn request_sharing_and_account_defaults_are_frozen_before_download() {
    let app = common::test_app().await;
    let owner = app
        .state
        .auth
        .create_user("owner", "password123", Role::User)
        .await
        .unwrap();
    let other = app
        .state
        .auth
        .create_user("other", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "owner", "password123").await;
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/profile",
            &cookie,
            Some(json!({"defaultBookSharing":"private"}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let metadata = MetadataResult {
        provider: "fake".into(),
        provider_key: "new-book".into(),
        title: "New fictional book".into(),
        ..Default::default()
    };
    let id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &metadata,
    )
    .await
    .unwrap();
    bokhylle_server::acquisition::create(
        &app.state.db,
        id,
        Some(owner.id),
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();
    assert!(
        !sharing::can_access(&app.state.db, other.id, id)
            .await
            .unwrap()
    );
    request(
        &app,
        "PUT",
        "/api/profile",
        &cookie,
        Some(json!({"defaultBookSharing":"shared"})),
    )
    .await;
    assert_eq!(
        sharing::state(&app.state.db, owner.id, id)
            .await
            .unwrap()
            .sharing,
        Some(BookSharing::Private)
    );
    let second = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &MetadataResult {
            provider: "fake".into(),
            provider_key: "override".into(),
            title: "Override fictional book".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (status, _) = request(
        &app,
        "POST",
        &format!("/api/books/{second}/acquisitions"),
        &cookie,
        Some(json!({"sharing":"private"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        sharing::state(&app.state.db, owner.id, second)
            .await
            .unwrap()
            .sharing,
        Some(BookSharing::Private)
    );
    assert!(
        !sharing::can_access(&app.state.db, other.id, second)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn bulk_changes_are_atomic_and_children_cannot_change_sharing() {
    let app = common::test_app().await;
    let owner = app
        .state
        .auth
        .create_user("owner", "password123", Role::User)
        .await
        .unwrap();
    let other = app
        .state
        .auth
        .create_user("other", "password123", Role::User)
        .await
        .unwrap();
    let child = app
        .state
        .auth
        .create_user("child", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(child.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let cookie = common::login(&app, "owner", "password123").await;
    let child_cookie = common::login(&app, "child", "password123").await;
    let (first, _) = book(&app, "first").await;
    let (second, _) = book(&app, "second").await;
    sharing::choose(&app.state.db, owner.id, first, None)
        .await
        .unwrap();
    bokhylle_server::user_books::add(&app.state.db, owner.id, first, "manual")
        .await
        .unwrap();
    sharing::choose(&app.state.db, other.id, second, None)
        .await
        .unwrap();
    bokhylle_server::user_books::add(&app.state.db, other.id, second, "manual")
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/books/sharing",
            &cookie,
            Some(json!({"bookIds":[first,second],"sharing":"private"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sharing::state(&app.state.db, owner.id, first)
            .await
            .unwrap()
            .sharing,
        Some(BookSharing::Shared)
    );
    sharing::choose(&app.state.db, owner.id, second, None)
        .await
        .unwrap();
    bokhylle_server::user_books::add(&app.state.db, owner.id, second, "manual")
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/books/sharing",
            &cookie,
            Some(json!({"bookIds":[first,second],"sharing":"private"}))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(
        !sharing::can_access(&app.state.db, other.id, first)
            .await
            .unwrap()
    );
    assert!(
        sharing::can_access(&app.state.db, other.id, second)
            .await
            .unwrap()
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("/api/books/{first}/sharing"),
            &child_cookie,
            Some(json!({"sharing":"shared"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/books/sharing",
            &child_cookie,
            Some(json!({"bookIds":[first],"sharing":"shared"}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
