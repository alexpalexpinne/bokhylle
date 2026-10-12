use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bokhylle_server::auth::Role;
use serde_json::{Value, json};
use tower::ServiceExt;

mod common;

async fn request(
    app: &common::TestApp,
    method: &str,
    path: &str,
    cookie: &str,
) -> (StatusCode, Value) {
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
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
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn alert(app: &common::TestApp, user: i64, book: Option<i64>, read: bool) -> i64 {
    sqlx::query(
        "INSERT INTO notifications (user_id, kind, title, book_id, read)
         VALUES (?, 'ready', 'A fictional book is ready', ?, ?)",
    )
    .bind(user)
    .bind(book)
    .bind(read)
    .execute(&app.state.db)
    .await
    .unwrap()
    .last_insert_rowid()
}

async fn book(app: &common::TestApp, title: &str) -> i64 {
    sqlx::query("INSERT INTO books (title, normalized_title) VALUES (?, ?)")
        .bind(title)
        .bind(title.to_lowercase())
        .execute(&app.state.db)
        .await
        .unwrap()
        .last_insert_rowid()
}

#[tokio::test]
async fn dismissal_is_persistent_and_scoped_to_the_authenticated_profile() {
    let app = common::test_app().await;
    let reader = app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let other = app
        .state
        .auth
        .create_user("other", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let unread = alert(&app, reader.id, None, false).await;
    let read = alert(&app, reader.id, None, true).await;
    let other_alert = alert(&app, other.id, None, false).await;

    for (id, session, expected) in [
        (unread, "", StatusCode::UNAUTHORIZED),
        (other_alert, cookie.as_str(), StatusCode::NOT_FOUND),
        (99999, cookie.as_str(), StatusCode::NOT_FOUND),
        (read, cookie.as_str(), StatusCode::NO_CONTENT),
    ] {
        assert_eq!(
            request(&app, "DELETE", &format!("/api/notifications/{id}"), session)
                .await
                .0,
            expected
        );
    }
    let (status, listed) = request(&app, "GET", "/api/notifications", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["unread"], 1);
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["id"], unread);

    assert_eq!(
        request(
            &app,
            "DELETE",
            &format!("/api/notifications/{unread}"),
            &cookie
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            "DELETE",
            &format!("/api/notifications/{unread}"),
            &cookie
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let listed = request(&app, "GET", "/api/notifications", &cookie).await.1;
    assert_eq!(listed["unread"], 0);
    assert_eq!(listed["items"], json!([]));
    let other_cookie = common::login(&app, "other", "password123").await;
    let other_list = request(&app, "GET", "/api/notifications", &other_cookie)
        .await
        .1;
    assert_eq!(other_list["items"][0]["id"], other_alert);
    assert_eq!(other_list["unread"], 1);
}

#[tokio::test]
async fn child_dismissal_preserves_the_shelf_visibility_boundary() {
    let app = common::test_app().await;
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
    let assigned = book(&app, "The Paper Kite").await;
    let historical = book(&app, "The Hidden Atlas").await;
    bokhylle_server::user_books::add(&app.state.db, child.id, assigned, "assigned")
        .await
        .unwrap();
    let visible = alert(&app, child.id, Some(assigned), false).await;
    let hidden = alert(&app, child.id, Some(historical), false).await;
    let general = alert(&app, child.id, None, false).await;
    let cookie = common::login(&app, "child", "password123").await;

    let listed = request(&app, "GET", "/api/notifications", &cookie).await.1;
    assert_eq!(listed["unread"], 2);
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        request(
            &app,
            "DELETE",
            &format!("/api/notifications/{hidden}"),
            &cookie
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for id in [visible, general] {
        assert_eq!(
            request(&app, "DELETE", &format!("/api/notifications/{id}"), &cookie)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
    }
    let listed = request(&app, "GET", "/api/notifications", &cookie).await.1;
    assert_eq!(listed["unread"], 0);
    assert_eq!(listed["items"], json!([]));
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM notifications WHERE id = ?")
        .bind(hidden)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(remaining, 1);
}

#[tokio::test]
async fn dismissing_an_alert_keeps_requests_and_downloads_active() {
    let app = common::test_app().await;
    let admin = app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let requester = app
        .state
        .auth
        .create_user("requester", "password123", Role::User)
        .await
        .unwrap();
    let book_id = book(&app, "Where Maps End").await;
    sqlx::query("INSERT INTO book_requests (user_id, book_id) VALUES (?, ?)")
        .bind(requester.id)
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO acquisitions (id, book_id, user_id, status) VALUES ('fictional-download', ?, ?, 'QUEUED')")
        .bind(book_id).bind(admin.id).execute(&app.state.db).await.unwrap();
    let id = alert(&app, admin.id, Some(book_id), false).await;
    sqlx::query("UPDATE notifications SET kind = 'request', acquisition_id = 'fictional-download' WHERE id = ?")
        .bind(id).execute(&app.state.db).await.unwrap();
    let cookie = common::login(&app, "admin", "password123").await;
    assert_eq!(
        request(&app, "DELETE", &format!("/api/notifications/{id}"), &cookie)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let listed = request(&app, "GET", "/api/notifications", &cookie).await.1;
    assert_eq!(listed["items"], json!([]));
    assert_eq!(listed["pendingRequests"], 1);
    assert_eq!(listed["pendingRequestItems"][0]["status"], "requested");
    let status: String =
        sqlx::query_scalar("SELECT status FROM acquisitions WHERE id = 'fictional-download'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(status, "QUEUED");
}
