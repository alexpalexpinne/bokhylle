use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bokhylle_server::app;
use bokhylle_server::auth::Role;
use bokhylle_server::demo::{DemoState, validate_installation};
use tower::ServiceExt;

mod common;

async fn enter(router: &axum::Router, profile: &str) -> axum::response::Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/demo/enter")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"profile":"{profile}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn cookie(response: &axum::response::Response) -> String {
    response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn get_json(router: &axum::Router, path: &str, session: &str) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header(header::COOKIE, session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap()
}

#[tokio::test]
async fn demo_entry_switch_and_guard() {
    let test = common::test_app().await;
    let mut state = test.state.clone();
    state.demo = Some(Arc::new(DemoState::default()));
    let router = app(state);

    let adult = enter(&router, "adult").await;
    assert_eq!(adult.status(), StatusCode::OK);
    let adult_cookie = cookie(&adult);
    let body = to_bytes(adult.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["profileType"], "adult");
    let adult_name = json["user"]["username"].as_str().unwrap().to_string();

    let child = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/demo/switch")
                .header(header::COOKIE, adult_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(child.status(), StatusCode::OK);
    let body = to_bytes(child.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["user"]["profileType"], "child");
    assert_ne!(json["user"]["username"], adult_name);

    let other = enter(&router, "adult").await;
    let body = to_bytes(other.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_ne!(json["user"]["username"], adult_name);
    let other_child = json["user"]["username"]
        .as_str()
        .unwrap()
        .replace("demo_adult_", "demo_child_");
    let other_child_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(other_child)
        .fetch_one(&test.state.db)
        .await
        .unwrap();
    let cross_visitor = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/users/{other_child_id}/shelf/1"))
                .header(header::COOKIE, adult_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"onShelf":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross_visitor.status(), StatusCode::FORBIDDEN);

    let members = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/household/members")
                .header(header::COOKIE, cookie(&enter(&router, "adult").await))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(members.status(), StatusCode::OK);
    let body = to_bytes(members.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["members"].as_array().unwrap().len(), 1);
    assert_eq!(json["members"][0]["profileType"], "child");

    let blocked = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);

    let blocked = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/discover/acquisitions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn demo_seeds_new_reader_choices_without_exposing_adult_shelf_to_child() {
    let test = common::test_app().await;
    let mut state = test.state.clone();
    state.demo = Some(Arc::new(DemoState::default()));
    let mut book_ids = Vec::new();
    for (title, normalized) in [
        ("The Secret Garden", "the secret garden"),
        ("The Picture of Dorian Gray", "the picture of dorian gray"),
    ] {
        let book_id = sqlx::query("INSERT INTO books (title, normalized_title) VALUES (?, ?)")
            .bind(title)
            .bind(normalized)
            .execute(&state.db)
            .await
            .unwrap()
            .last_insert_rowid();
        let edition_id = sqlx::query("INSERT INTO editions (book_id, title) VALUES (?, ?)")
            .bind(book_id)
            .bind(title)
            .execute(&state.db)
            .await
            .unwrap()
            .last_insert_rowid();
        sqlx::query(
            "INSERT INTO book_files (edition_id, path, format, size, sha256)
             VALUES (?, ?, 'epub', 1, ?)",
        )
        .bind(edition_id)
        .bind(format!("{normalized}.epub"))
        .bind(normalized)
        .execute(&state.db)
        .await
        .unwrap();
        book_ids.push(book_id);
    }

    let router = app(state);
    let adult_cookie = cookie(&enter(&router, "adult").await);
    let adult_book = get_json(
        &router,
        &format!("/api/books/{}", book_ids[1]),
        &adult_cookie,
    )
    .await;
    assert_eq!(adult_book["onShelf"], true);

    let switched = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/demo/switch")
                .header(header::COOKIE, adult_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let child_cookie = cookie(&switched);
    let child_book = get_json(
        &router,
        &format!("/api/books/{}", book_ids[0]),
        &child_cookie,
    )
    .await;
    assert_eq!(child_book["onShelf"], true);
    let hidden = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/books/{}", book_ids[1]))
                .header(header::COOKIE, child_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn demo_requires_marked_storage_without_regular_accounts() {
    let test = common::test_app().await;
    let config = &test.state.paths.config_dir;
    assert!(validate_installation(&test.state.db, config).await.is_err());
    std::fs::write(config.join(".bokhylle-demo"), "bokhylle-demo-v1\n").unwrap();
    assert!(validate_installation(&test.state.db, config).await.is_ok());
    test.state
        .auth
        .create_user("existing", "unrelated-password", Role::User)
        .await
        .unwrap();
    assert!(validate_installation(&test.state.db, config).await.is_err());
}

#[tokio::test]
async fn demo_get_finishes_durably_and_send_never_uses_delivery() {
    let test = common::test_app().await;
    let mut state = test.state.clone();
    state.demo = Some(Arc::new(DemoState::default()));
    let router = app(state.clone());
    let adult_cookie = cookie(&enter(&router, "adult").await);
    let other_cookie = cookie(&enter(&router, "adult").await);

    let book_id = sqlx::query(
        "INSERT INTO books (title, normalized_title) VALUES ('Demo Sample', 'demo sample')",
    )
    .execute(&state.db)
    .await
    .unwrap()
    .last_insert_rowid();
    let edition_id = sqlx::query("INSERT INTO editions (book_id, title) VALUES (?, 'Demo Sample')")
        .bind(book_id)
        .execute(&state.db)
        .await
        .unwrap()
        .last_insert_rowid();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, 'sample.epub', 'epub', 1, 'demo-sample-hash')",
    )
    .bind(edition_id)
    .execute(&state.db)
    .await
    .unwrap();

    sqlx::query("INSERT INTO books_fts (rowid, title, author) VALUES (?, 'Demo Sample', '')")
        .bind(book_id)
        .execute(&state.db)
        .await
        .unwrap();

    let started = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/demo/get")
                .header(header::COOKIE, &adult_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"bookId":{book_id},"sendWhenReady":true}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(started.status(), StatusCode::OK);
    let started: serde_json::Value =
        serde_json::from_slice(&to_bytes(started.into_body(), usize::MAX).await.unwrap()).unwrap();
    let again = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/demo/get")
                .header(header::COOKIE, &adult_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"bookId":{book_id}}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    let again: serde_json::Value =
        serde_json::from_slice(&to_bytes(again.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(again["id"], started["id"]);
    let catalogue = get_json(
        &router,
        "/api/discover?q=Demo%20Sample&type=title&source=local",
        &adult_cookie,
    )
    .await;
    assert_eq!(catalogue["books"][0]["status"], "DOWNLOADING");
    assert_eq!(catalogue["books"][0]["onShelf"], false);

    let other_activity = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/demo/activity")
                .header(header::COOKIE, &other_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(other_activity.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["gets"].as_array().unwrap().is_empty());

    sqlx::query("UPDATE demo_gets SET ready_at = unixepoch() - 1")
        .execute(&state.db)
        .await
        .unwrap();
    bokhylle_server::demo::tick(&state).await.unwrap();
    bokhylle_server::demo::tick(&state).await.unwrap();

    let activity = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/demo/activity")
                .header(header::COOKIE, &adult_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(activity.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["gets"][0]["status"], "READY");
    assert_eq!(json["sends"].as_array().unwrap().len(), 1);
    assert_eq!(json["sends"][0]["status"], "PREPARING");

    let get_owned = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/demo/get")
                .header(header::COOKIE, &adult_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"bookId":{book_id}}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_owned.status(), StatusCode::CONFLICT);

    let send_request = || {
        Request::builder()
            .method("POST")
            .uri("/api/demo/send")
            .header(header::COOKIE, &adult_cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(r#"{{"bookId":{book_id}}}"#)))
            .unwrap()
    };
    let (send_one, send_two) = tokio::join!(
        router.clone().oneshot(send_request()),
        router.clone().oneshot(send_request()),
    );
    for result in [send_one, send_two] {
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["id"], json["sends"][0]["id"]);
    }
    let progress = get_json(&router, "/api/demo/activity", &adult_cookie).await;
    assert_eq!(progress["sends"].as_array().unwrap().len(), 1);
    sqlx::query("UPDATE demo_sends SET created_at = unixepoch() - 4")
        .execute(&state.db)
        .await
        .unwrap();
    let progress = get_json(&router, "/api/demo/activity", &adult_cookie).await;
    assert_eq!(progress["sends"][0]["status"], "SENDING");
    sqlx::query("UPDATE demo_sends SET created_at = unixepoch() - 9")
        .execute(&state.db)
        .await
        .unwrap();
    // Activity derives progress from durable timestamps, including after restart.
    let progress = get_json(&app(state.clone()), "/api/demo/activity", &adult_cookie).await;
    assert_eq!(progress["sends"][0]["status"], "DELIVERED");
    let private = get_json(&router, "/api/demo/activity", &other_cookie).await;
    assert!(private["sends"].as_array().unwrap().is_empty());
    let catalogue = get_json(
        &router,
        "/api/discover?q=Demo%20Sample&type=title&source=local",
        &adult_cookie,
    )
    .await;
    assert_eq!(catalogue["books"][0]["status"], "IN_LIBRARY");
    assert_eq!(catalogue["books"][0]["onShelf"], true);

    let owner_id: i64 = sqlx::query_scalar("SELECT user_id FROM demo_gets WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    bokhylle_server::user_books::remove(&state.db, owner_id, book_id)
        .await
        .unwrap();
    let retry = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/demo/get")
                .header(header::COOKIE, &adult_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"bookId":{book_id}}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::OK);
    let restarted: Option<i64> =
        sqlx::query_scalar("SELECT completed_at FROM demo_gets WHERE user_id = ? AND book_id = ?")
            .bind(owner_id)
            .bind(book_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(restarted.is_none());

    let other_send = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/demo/send")
                .header(header::COOKIE, &other_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"bookId":{book_id}}}"#)))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(other_send.status(), StatusCode::FORBIDDEN);

    let deliveries: i64 = sqlx::query_scalar("SELECT count(*) FROM deliveries")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(deliveries, 0);
}

#[tokio::test]
async fn demo_child_requests_are_visible_and_decidable_only_within_the_visitor_pair() {
    let test = common::test_app().await;
    let mut state = test.state.clone();
    state.demo = Some(Arc::new(DemoState::default()));
    let book_id = sqlx::query(
        "INSERT INTO books (title, normalized_title) VALUES ('Black Beauty', 'black beauty')",
    )
    .execute(&state.db)
    .await
    .unwrap()
    .last_insert_rowid();
    let edition_id =
        sqlx::query("INSERT INTO editions (book_id, title) VALUES (?, 'Black Beauty')")
            .bind(book_id)
            .execute(&state.db)
            .await
            .unwrap()
            .last_insert_rowid();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, 'black-beauty.epub', 'epub', 1, 'demo-black-beauty')",
    )
    .bind(edition_id)
    .execute(&state.db)
    .await
    .unwrap();

    let router = app(state.clone());
    let first = cookie(&enter(&router, "adult").await);
    let second = cookie(&enter(&router, "adult").await);
    let first_notifications = get_json(&router, "/api/notifications", &first).await;
    let second_notifications = get_json(&router, "/api/notifications", &second).await;
    let first_request_id = first_notifications["pendingRequestItems"][0]["id"]
        .as_i64()
        .unwrap();
    let second_request_id = second_notifications["pendingRequestItems"][0]["id"]
        .as_i64()
        .unwrap();
    assert_ne!(first_request_id, second_request_id);
    assert_eq!(
        first_notifications["pendingRequestItems"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        second_notifications["pendingRequestItems"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(first_notifications["unread"], 1);

    let cross_pair = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/demo/requests/{second_request_id}/approve"))
                .header(header::COOKIE, &first)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross_pair.status(), StatusCode::FORBIDDEN);

    let approved = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/demo/requests/{first_request_id}/approve"))
                .header(header::COOKIE, &first)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(approved.status(), StatusCode::OK);
    let first_notifications = get_json(&router, "/api/notifications", &first).await;
    let second_notifications = get_json(&router, "/api/notifications", &second).await;
    assert!(
        first_notifications["pendingRequestItems"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        second_notifications["pendingRequestItems"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let child_id: i64 = sqlx::query_scalar("SELECT user_id FROM book_requests WHERE id = ?")
        .bind(first_request_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert!(
        bokhylle_server::user_books::contains(&state.db, child_id, book_id)
            .await
            .unwrap()
    );
    let acquisitions: i64 = sqlx::query_scalar("SELECT count(*) FROM acquisitions")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(acquisitions, 0);
}
