mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bokhylle_metadata::MetadataResult;
use bokhylle_server::auth::Role;
use tower::ServiceExt;

fn md5_hex(value: &str) -> String {
    use md5::Digest;
    hex::encode(md5::Md5::digest(value.as_bytes()))
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn kosync(
    method: &str,
    uri: &str,
    user: &str,
    key: &str,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-auth-user", user)
        .header("x-auth-key", key);
    match body {
        Some(value) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&value).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn insert_book(
    app: &common::TestApp,
    title: &str,
    author: &str,
    path: &std::path::Path,
) -> i64 {
    let metadata = MetadataResult {
        provider: "openlibrary".to_string(),
        provider_key: format!("/works/{title}"),
        title: title.to_string(),
        authors: vec![author.to_string()],
        ..Default::default()
    };
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &metadata,
    )
    .await
    .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let size = std::fs::metadata(path).unwrap().len() as i64;
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, 'epub', ?, ?)",
    )
    .bind(edition_id)
    .bind(path.to_str().unwrap())
    .bind(size)
    .bind(format!("sha-{book_id}"))
    .execute(&app.state.db)
    .await
    .unwrap();
    book_id
}

#[tokio::test]
async fn kosync_authenticates_with_a_reader_token_and_syncs_progress() {
    let temp = tempfile::tempdir().unwrap();
    let book_path = temp.path().join("synced-book.epub");
    let bytes: Vec<u8> = (0..5000u32).map(|index| (index % 251) as u8).collect();
    std::fs::write(&book_path, &bytes).unwrap();

    let app = common::test_app_with_library_root(temp.path().to_path_buf()).await;
    let user = app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();
    let (_, token) = bokhylle_server::reader_tokens::create(&app.state.db, user.id, "KOReader")
        .await
        .unwrap();
    let key = md5_hex(&token);
    let book_id = insert_book(&app, "Synced Book", "Alice Author", &book_path).await;
    let document = bokhylle_server::partial_md5(&book_path).unwrap();

    let health = app
        .router
        .clone()
        .oneshot(Request::get("/healthcheck").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(json(health).await["state"], "OK");

    let anonymous = app
        .router
        .clone()
        .oneshot(kosync("GET", "/users/auth", "alice", "", None))
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json(anonymous).await["code"], 2001);

    let wrong_key = md5_hex("99999999999999999999999999999999");
    let registered = app
        .router
        .clone()
        .oneshot(kosync(
            "POST",
            "/users/create",
            "alice",
            &wrong_key,
            Some(serde_json::json!({ "username": "alice", "password": wrong_key })),
        ))
        .await
        .unwrap();
    assert_eq!(registered.status(), StatusCode::FORBIDDEN);

    let registered = app
        .router
        .clone()
        .oneshot(kosync(
            "POST",
            "/users/create",
            "alice",
            "irrelevant",
            Some(serde_json::json!({ "username": "alice", "password": key })),
        ))
        .await
        .unwrap();
    assert_eq!(registered.status(), StatusCode::CREATED);
    assert_eq!(json(registered).await["username"], "alice");

    let taken = app
        .router
        .clone()
        .oneshot(kosync(
            "POST",
            "/users/create",
            "alice",
            "irrelevant",
            Some(serde_json::json!({ "username": "someone-else", "password": key })),
        ))
        .await
        .unwrap();
    assert_eq!(taken.status(), StatusCode::PAYMENT_REQUIRED);
    assert_eq!(json(taken).await["code"], 2002);

    // A bound token authenticates only under its registered username.
    let wrong_user = app
        .router
        .clone()
        .oneshot(kosync("GET", "/users/auth", "someone-else", &key, None))
        .await
        .unwrap();
    assert_eq!(wrong_user.status(), StatusCode::UNAUTHORIZED);

    let authorized = app
        .router
        .clone()
        .oneshot(kosync("GET", "/users/auth", "alice", &key, None))
        .await
        .unwrap();
    assert_eq!(authorized.status(), StatusCode::OK);
    assert_eq!(json(authorized).await["authorized"], "OK");

    let pushed = app
        .router
        .clone()
        .oneshot(kosync(
            "PUT",
            "/syncs/progress",
            "alice",
            &key,
            Some(serde_json::json!({
                "document": document,
                "progress": "/body/DocFragment[11]/body/div/p[7]/text().123",
                "percentage": 0.4213,
                "device": "Kobo_clara",
                "device_id": "1A2B3C4D",
            })),
        ))
        .await
        .unwrap();
    assert_eq!(pushed.status(), StatusCode::OK);
    let pushed = json(pushed).await;
    assert_eq!(pushed["document"], document);
    assert!(pushed["timestamp"].as_i64().unwrap() > 0);

    // KOReader sends valid JSON; the reference server never inspects the
    // request Content-Type, so neither do we.
    let pushed = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/syncs/progress")
                .header("x-auth-user", "alice")
                .header("x-auth-key", &key)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "document": document,
                        "progress": "/body/DocFragment[11]/body/div/p[7]/text().123",
                        "percentage": 0.4213,
                        "device": "Kobo_clara",
                        "device_id": "1A2B3C4D",
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pushed.status(), StatusCode::OK);

    let pulled = app
        .router
        .clone()
        .oneshot(kosync(
            "GET",
            &format!("/syncs/progress/{document}"),
            "alice",
            &key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(pulled.status(), StatusCode::OK);
    let pulled = json(pulled).await;
    assert_eq!(pulled["percentage"], 0.4213);
    assert_eq!(
        pulled["progress"],
        "/body/DocFragment[11]/body/div/p[7]/text().123"
    );
    assert_eq!(pulled["device"], "Kobo_clara");
    assert_eq!(pulled["device_id"], "1A2B3C4D");

    let unknown = app
        .router
        .clone()
        .oneshot(kosync(
            "GET",
            "/syncs/progress/neverseenatall0000000000000000ff",
            "alice",
            &key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(unknown.status(), StatusCode::OK);
    assert_eq!(json(unknown).await, serde_json::json!({}));

    let missing = app
        .router
        .clone()
        .oneshot(kosync(
            "PUT",
            "/syncs/progress",
            "alice",
            &key,
            Some(serde_json::json!({ "percentage": 0.5, "progress": "1", "device": "K" })),
        ))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::FORBIDDEN);
    assert_eq!(json(missing).await["code"], 2004);

    let lowered = app
        .router
        .clone()
        .oneshot(kosync(
            "PUT",
            "/syncs/progress",
            "alice",
            &key,
            Some(serde_json::json!({
                "document": document,
                "progress": "42",
                "percentage": 0.1,
                "device": "Kobo_clara",
            })),
        ))
        .await
        .unwrap();
    assert_eq!(lowered.status(), StatusCode::OK);
    let pulled = app
        .router
        .clone()
        .oneshot(kosync(
            "GET",
            &format!("/syncs/progress/{document}"),
            "alice",
            &key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(json(pulled).await["percentage"], 0.1);

    let cookie = common::login(&app, "alice", "password123").await;
    let continue_reading = app
        .router
        .clone()
        .oneshot(
            Request::get("/api/books/continue")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(continue_reading.status(), StatusCode::OK);
    let items = json(continue_reading).await;
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["book"]["id"], book_id);
    assert_eq!(items[0]["percentage"], 0.1);
}

#[tokio::test]
async fn filename_checksums_map_and_children_only_see_their_shelf() {
    let temp = tempfile::tempdir().unwrap();
    let book_path = temp.path().join("shelf-book.epub");
    std::fs::write(&book_path, vec![3u8; 9000]).unwrap();

    let app = common::test_app_with_library_root(temp.path().to_path_buf()).await;
    let child = app
        .state
        .auth
        .create_user_with_profile("kid", "password123", Role::User, "password", "child")
        .await
        .unwrap();
    let (_, token) = bokhylle_server::reader_tokens::create(&app.state.db, child.id, "KOReader")
        .await
        .unwrap();
    let key = md5_hex(&token);
    let book_id = insert_book(&app, "Shelf Book", "Kid Author", &book_path).await;

    // KOReader's filename-checksum mode hashes the served filename.
    let document = md5_hex("Shelf_Book.epub");
    let progress = serde_json::json!({
        "document": document,
        "progress": "12",
        "percentage": 0.5,
        "device": "Kindle",
    });
    let denied = app
        .router
        .clone()
        .oneshot(kosync(
            "PUT",
            "/syncs/progress",
            "kid",
            &key,
            Some(progress.clone()),
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        app.router
            .clone()
            .oneshot(kosync(
                "GET",
                &format!("/syncs/progress/{document}"),
                "kid",
                &key,
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    let cookie = common::login(&app, "kid", "password123").await;
    let items = app
        .router
        .clone()
        .oneshot(
            Request::get("/api/books/continue")
                .header(header::COOKIE, cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        json(items).await,
        serde_json::json!([]),
        "off-shelf progress stays hidden from children"
    );

    bokhylle_server::user_books::add(&app.state.db, child.id, book_id, "request")
        .await
        .unwrap();
    let pushed = app
        .router
        .clone()
        .oneshot(kosync(
            "PUT",
            "/syncs/progress",
            "kid",
            &key,
            Some(progress.clone()),
        ))
        .await
        .unwrap();
    assert_eq!(pushed.status(), StatusCode::OK);
    let pulled = app
        .router
        .clone()
        .oneshot(kosync(
            "GET",
            &format!("/syncs/progress/{document}"),
            "kid",
            &key,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(json(pulled).await["progress"], "12");
    let items = app
        .router
        .clone()
        .oneshot(
            Request::get("/api/books/continue")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let items = json(items).await;
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["book"]["title"], "Shelf Book");

    sqlx::query("DELETE FROM user_books WHERE user_id = ? AND book_id = ?")
        .bind(child.id)
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        app.router
            .clone()
            .oneshot(kosync(
                "GET",
                &format!("/syncs/progress/{document}"),
                "kid",
                &key,
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.router
            .clone()
            .oneshot(kosync(
                "PUT",
                "/syncs/progress",
                "kid",
                &key,
                Some(progress)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
