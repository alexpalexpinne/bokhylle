use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;

use bokhylle_server::auth::Role;

mod common;

struct FakeSmtp {
    addr: std::net::SocketAddr,
    messages: Arc<Mutex<Vec<String>>>,
}

async fn start_smtp() -> FakeSmtp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let messages = Arc::new(Mutex::new(Vec::new()));
    let messages_for_server = messages.clone();

    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            let messages = messages_for_server.clone();

            tokio::spawn(async move {
                let (reader, mut writer) = tokio::io::split(socket);
                let mut lines = BufReader::new(reader).lines();
                let mut in_data = false;
                let mut body = String::new();

                writer.write_all(b"220 fake ESMTP\r\n").await.ok();

                while let Ok(Some(line)) = lines.next_line().await {
                    if in_data {
                        if line == "." {
                            in_data = false;
                            messages.lock().unwrap().push(body.clone());
                            body.clear();
                            writer.write_all(b"250 Ok\r\n").await.ok();
                        } else {
                            body.push_str(&line);
                            body.push('\n');
                        }
                        continue;
                    }

                    let upper = line.to_ascii_uppercase();
                    if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                        writer.write_all(b"250-fake\r\n250 OK\r\n").await.ok();
                    } else if upper.starts_with("DATA") {
                        writer
                            .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                            .await
                            .ok();
                        in_data = true;
                    } else if upper.starts_with("QUIT") {
                        writer.write_all(b"221 Bye\r\n").await.ok();
                        break;
                    } else {
                        writer.write_all(b"250 OK\r\n").await.ok();
                    }
                }
            });
        }
    });

    FakeSmtp { addr, messages }
}

async fn get_json(test_app: &common::TestApp, uri: &str, cookie: &str) -> (StatusCode, Value) {
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
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn post_json(
    test_app: &common::TestApp,
    uri: &str,
    cookie: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, cookie)
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn app_with_book() -> (common::TestApp, tempfile::TempDir, String, i64, i64) {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let test_app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/library/scan")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    for _ in 0..200 {
        let (_, status) = get_json(&test_app, "/api/library/scan/status", &cookie).await;
        if status["running"] == false && status["summary"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let (_, books) = get_json(&test_app, "/api/books?pageSize=50", &cookie).await;
    let book = books["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["title"] == "Project Hail Mary")
        .expect("fixture book");
    let book_id = book["id"].as_i64().unwrap();

    let (_, detail) = get_json(&test_app, &format!("/api/books/{book_id}"), &cookie).await;
    let file_id = detail["files"][0]["id"].as_i64().unwrap();

    (test_app, library_dir, cookie, book_id, file_id)
}

async fn configure_smtp(test_app: &common::TestApp, host: &str, port: u16) {
    test_app
        .state
        .settings
        .set("smtp.host", &json!(host))
        .await
        .unwrap();
    test_app
        .state
        .settings
        .set("smtp.port", &json!(port))
        .await
        .unwrap();
    test_app
        .state
        .settings
        .set("smtp.tls", &json!("none"))
        .await
        .unwrap();
    test_app
        .state
        .settings
        .set("smtp.from", &json!("library@example.com"))
        .await
        .unwrap();
}

#[tokio::test]
async fn delivers_a_book_to_the_configured_target() {
    let (test_app, _library_dir, cookie, book_id, file_id) = app_with_book().await;
    let smtp = start_smtp().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    let (status, target) = post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "name": "Kindle", "address": "reader@kindle.example" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(target["address"], "reader@kindle.example");

    let (status, delivery) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(delivery["status"], "SENT", "delivery: {delivery}");

    {
        let messages = smtp.messages.lock().unwrap();
        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert!(message.contains("Project Hail Mary"));
        assert!(message.contains("application/epub+zip"));
        assert!(message.contains("reader@kindle.example"));
    }

    let (status, deliveries) = get_json(
        &test_app,
        &format!("/api/deliveries?bookId={book_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deliveries.as_array().unwrap().len(), 1);
    assert_eq!(deliveries[0]["status"], "SENT");
}

#[tokio::test]
async fn profile_stats_count_unique_successful_books_for_the_signed_in_profile() {
    let (test_app, _library_dir, cookie, book_id, file_id) = app_with_book().await;
    let reader_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'reader'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let other = test_app
        .state
        .auth
        .create_user("other_reader", "password123", Role::User)
        .await
        .unwrap();
    let other_cookie = common::login(&test_app, "other_reader", "password123").await;

    bokhylle_server::user_books::add(&test_app.state.db, reader_id, book_id, "manual")
        .await
        .unwrap();
    sqlx::query("UPDATE user_books SET preference = 'liked' WHERE user_id = ? AND book_id = ?")
        .bind(reader_id)
        .bind(book_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let author_id: i64 =
        sqlx::query_scalar("SELECT author_id FROM book_authors WHERE book_id = ? LIMIT 1")
            .bind(book_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    sqlx::query("INSERT INTO author_follows (user_id, author_id) VALUES (?, ?)")
        .bind(reader_id)
        .bind(author_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    for (user_id, status) in [
        (reader_id, "SENT"),
        (reader_id, "SENT"),
        (reader_id, "FAILED"),
        (other.id, "SENT"),
    ] {
        sqlx::query(
            "INSERT INTO deliveries (book_id, file_id, user_id, address, status)
             VALUES (?, ?, ?, 'reader@example.com', ?)",
        )
        .bind(book_id)
        .bind(file_id)
        .bind(user_id)
        .bind(status)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    }

    let (status, stats) = get_json(&test_app, "/api/profile/stats", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        stats,
        json!({ "shelf": 1, "authors": 1, "liked": 1, "booksSent": 1 })
    );
    let (status, other_stats) = get_json(&test_app, "/api/profile/stats", &other_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        other_stats,
        json!({ "shelf": 0, "authors": 0, "liked": 0, "booksSent": 1 })
    );
}

#[tokio::test]
async fn default_target_is_used_when_none_is_chosen() {
    let (test_app, _library_dir, cookie, book_id, file_id) = app_with_book().await;
    let smtp = start_smtp().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    // the first target a user creates becomes the default
    let (status, first) = post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "first@kindle.example" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(first["isDefault"], true);

    let (_, second) = post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "second@kindle.example" }),
    )
    .await;
    assert_eq!(second["isDefault"], false);
    let second_id = second["id"].as_i64().unwrap();

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/delivery-targets/{second_id}/default"))
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let targets: Value = serde_json::from_slice(&body).unwrap();
    let defaults: Vec<&Value> = targets
        .as_array()
        .unwrap()
        .iter()
        .filter(|target| target["isDefault"] == true)
        .collect();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0]["address"], "second@kindle.example");

    let (status, delivery) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(delivery["address"], "second@kindle.example");

    let messages = smtp.messages.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("second@kindle.example"));
}

#[tokio::test]
async fn disabled_targets_cannot_be_chosen_explicitly() {
    let (test_app, _library_dir, cookie, book_id, file_id) = app_with_book().await;
    let smtp = start_smtp().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    let (_, target) = post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "reader@kindle.example" }),
    )
    .await;
    let target_id = target["id"].as_i64().unwrap();

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/delivery-targets/{target_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie)
                .body(Body::from(r#"{"enabled":false}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let (status, _) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &cookie,
        json!({ "targetId": target_id }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let messages = smtp.messages.lock().unwrap();
    assert_eq!(messages.len(), 0);
}

#[tokio::test]
async fn delivery_requires_a_target_and_smtp_configuration() {
    let (test_app, _library_dir, cookie, book_id, file_id) = app_with_book().await;
    let smtp = start_smtp().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    let (status, body) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "unprocessable_entity");

    post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "reader@kindle.example" }),
    )
    .await;

    test_app
        .state
        .settings
        .set("smtp.host", &json!(""))
        .await
        .unwrap();

    let (status, delivery) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(delivery["status"], "FAILED");
    assert!(
        delivery["errorMessage"]
            .as_str()
            .unwrap()
            .contains("SMTP is not configured")
    );
}

#[tokio::test]
async fn oversized_files_are_rejected() {
    let (test_app, _library_dir, cookie, book_id, file_id) = app_with_book().await;
    let smtp = start_smtp().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "reader@kindle.example" }),
    )
    .await;

    sqlx::query("UPDATE book_files SET size = ? WHERE id = ?")
        .bind(100 * 1024 * 1024i64)
        .bind(file_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    let (status, body) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["message"].as_str().unwrap().contains("delivery limit"));
}

#[tokio::test]
async fn failed_deliveries_can_be_retried() {
    let (test_app, _library_dir, cookie, book_id, file_id) = app_with_book().await;

    // First point SMTP at a closed port so the send fails fast.
    configure_smtp(&test_app, "127.0.0.1", 1).await;

    post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "reader@kindle.example" }),
    )
    .await;

    let (status, delivery) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(delivery["status"], "FAILED");
    let delivery_id = delivery["id"].as_i64().unwrap();

    let smtp = start_smtp().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    let (status, retried) = post_json(
        &test_app,
        &format!("/api/deliveries/{delivery_id}/retry"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(retried["status"], "SENT");
    assert_eq!(smtp.messages.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn creating_a_target_names_it_after_the_device_type() {
    let (test_app, _library_dir, cookie, _book_id, _file_id) = app_with_book().await;

    let (status, target) = post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "reader@pbsync.example", "deviceType": "pocketbook" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(target["type"], "pocketbook");
    assert_eq!(target["name"], "PocketBook");

    let (_, named) = post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "second@pbsync.example", "deviceType": "pocketbook", "name": "Bedside" }),
    )
    .await;
    assert_eq!(named["name"], "Bedside");
}

#[tokio::test]
async fn delivery_targets_are_isolated_per_user() {
    let (test_app, _library_dir, cookie, _book_id, _file_id) = app_with_book().await;

    let (_, target) = post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "reader@kindle.example" }),
    )
    .await;
    let target_id = target["id"].as_i64().unwrap();

    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let bob_cookie = common::login(&test_app, "bob", "password123").await;

    let (_, bob_targets) = get_json(&test_app, "/api/delivery-targets", &bob_cookie).await;
    assert_eq!(bob_targets.as_array().unwrap().len(), 0);

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/delivery-targets/{target_id}"))
                .header(header::COOKIE, &bob_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delivery_history_is_user_scoped_with_an_admin_wide_flag() {
    let (test_app, _library_dir, cookie, book_id, file_id) = app_with_book().await;
    let smtp = start_smtp().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "admin@kindle.example" }),
    )
    .await;
    let (status, _) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let bob_cookie = common::login(&test_app, "bob", "password123").await;
    post_json(
        &test_app,
        "/api/delivery-targets",
        &bob_cookie,
        json!({ "address": "bob@kindle.example" }),
    )
    .await;
    let (status, _) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/deliver"),
        &bob_cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (_, mine) = get_json(&test_app, "/api/deliveries", &cookie).await;
    assert_eq!(mine.as_array().unwrap().len(), 1);
    assert_eq!(mine[0]["address"], "admin@kindle.example");

    let (_, all) = get_json(&test_app, "/api/deliveries?all=true", &cookie).await;
    assert_eq!(all.as_array().unwrap().len(), 2);

    let (_, bob_all) = get_json(&test_app, "/api/deliveries?all=true", &bob_cookie).await;
    assert_eq!(bob_all.as_array().unwrap().len(), 1);
    assert_eq!(bob_all[0]["address"], "bob@kindle.example");
}

#[tokio::test]
async fn default_reader_reports_the_household_fallback() {
    let (test_app, _library_dir, cookie, _book_id, _file_id) = app_with_book().await;

    let (status, body) = get_json(&test_app, "/api/delivery-targets/default", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["source"], "none");
    assert!(body["address"].is_null());
    assert!(body["senderAddress"].is_null());
    assert_eq!(
        body["amazonUrl"],
        "https://www.amazon.com/hz/mycd/myx#/home/settings/pdoc"
    );

    test_app
        .state
        .settings
        .set(
            bokhylle_server::settings::KINDLE_ADDRESS,
            &serde_json::json!("household@kindle.example"),
        )
        .await
        .unwrap();

    let (_, body) = get_json(&test_app, "/api/delivery-targets/default", &cookie).await;
    assert_eq!(body["source"], "household");
    assert_eq!(body["address"], "household@kindle.example");

    post_json(
        &test_app,
        "/api/delivery-targets",
        &cookie,
        json!({ "address": "personal@kindle.example" }),
    )
    .await;

    let (_, body) = get_json(&test_app, "/api/delivery-targets/default", &cookie).await;
    assert_eq!(body["source"], "personal");
    assert_eq!(body["address"], "personal@kindle.example");
}

#[tokio::test]
async fn acquisition_keeps_its_explicit_reader_without_changing_another_requester() {
    let (app, _library, cookie, book_id, _file_id) = app_with_book().await;
    let alice = app
        .state
        .auth
        .verify_login("reader", "password123")
        .await
        .unwrap()
        .unwrap();
    let bob = app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let (status, target) = post_json(
        &app,
        "/api/delivery-targets",
        &cookie,
        json!({"address": "chosen@kindle.example"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let target_id = target["id"].as_i64().unwrap();
    let destination = bokhylle_server::services::delivery::acquisition_destination(
        &app.state,
        &alice,
        true,
        Some(target_id),
    )
    .await
    .unwrap();
    let (acquisition, duplicate) = bokhylle_server::services::delivery::create_acquisition(
        &app.state,
        &alice,
        book_id,
        Some("epub".into()),
        vec!["en".into()],
        true,
        true,
        destination,
    )
    .await
    .unwrap();
    assert!(!duplicate);
    let (shared, duplicate) = bokhylle_server::acquisition::create_with_languages(
        &app.state.db,
        book_id,
        Some(bob.id),
        Some("epub".into()),
        vec!["en".into()],
        false,
        false,
    )
    .await
    .unwrap();
    assert!(duplicate);
    assert_eq!(shared.id, acquisition.id);
    let (status, new_default) = post_json(
        &app,
        "/api/delivery-targets",
        &cookie,
        json!({"address": "new-default@kindle.example"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = post_json(
        &app,
        &format!("/api/delivery-targets/{}/default", new_default["id"]),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    sqlx::query("UPDATE delivery_targets SET address = 'edited@kindle.example' WHERE id = ?")
        .bind(target_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let requests: Vec<(i64, bool, Option<i64>, Option<String>)> = sqlx::query_as("SELECT user_id, deliver_on_ready, delivery_target_id, delivery_address FROM acquisition_requests WHERE acquisition_id = ? ORDER BY user_id").bind(&acquisition.id).fetch_all(&app.state.db).await.unwrap();
    assert_eq!(
        requests,
        vec![
            (
                alice.id,
                true,
                Some(target_id),
                Some("chosen@kindle.example".into())
            ),
            (bob.id, false, None, None)
        ]
    );
}

#[tokio::test]
async fn smtp_connection_test_requires_admin_and_works() {
    let (test_app, _library_dir, _admin_cookie, _book_id, _file_id) = app_with_book().await;
    let smtp = start_smtp().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    test_app
        .state
        .auth
        .create_user("carol", "password123", Role::User)
        .await
        .unwrap();
    let user_cookie = common::login(&test_app, "carol", "password123").await;

    let (status, _) = post_json(
        &test_app,
        "/api/admin/integrations/smtp/test",
        &user_cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    test_app
        .state
        .auth
        .create_user("root", "password123", Role::Admin)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "root", "password123").await;

    let (status, body) = post_json(
        &test_app,
        "/api/admin/integrations/smtp/test",
        &admin_cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn notification_emails_are_opt_in_per_user() {
    let smtp = start_smtp().await;
    let test_app = common::test_app().await;
    configure_smtp(&test_app, "127.0.0.1", smtp.addr.port()).await;

    test_app
        .state
        .auth
        .create_user("emma", "password123", Role::User)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("alex", "password123", Role::User)
        .await
        .unwrap();

    let emma_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'emma'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let alex_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alex'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    sqlx::query(
        "UPDATE users SET notification_email = 'emma@example.com', email_notifications = 1
         WHERE id = ?",
    )
    .bind(emma_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    bokhylle_server::notifications::create(
        &test_app.state.db,
        Some(emma_id),
        "ready",
        "Added to your library: Project Hail Mary",
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let messages = smtp.messages.lock().unwrap().clone();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("Added to your library: Project Hail Mary"));
    assert!(messages[0].contains("emma@example.com"));
    drop(messages);

    // Alex has no notification email and never opted in.
    bokhylle_server::notifications::create(
        &test_app.state.db,
        Some(alex_id),
        "ready",
        "Added to your library: The Expanse",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(smtp.messages.lock().unwrap().len(), 1);

    // Kinds outside READY/FAILED/NEEDS_SELECTION never email.
    bokhylle_server::notifications::create(
        &test_app.state.db,
        Some(emma_id),
        "sent",
        "Sent to your reader",
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(smtp.messages.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn startup_recovery_fails_interrupted_pending_deliveries() {
    let (test_app, _library_dir, _cookie, book_id, file_id) = app_with_book().await;

    let pending_id: i64 = sqlx::query_scalar(
        "INSERT INTO deliveries (book_id, file_id, user_id, address, status)
         VALUES (?, ?, NULL, 'reader@example.com', 'PENDING')
         RETURNING id",
    )
    .bind(book_id)
    .bind(file_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    let sent_id: i64 = sqlx::query_scalar(
        "INSERT INTO deliveries (book_id, file_id, user_id, address, status)
         VALUES (?, ?, NULL, 'reader@example.com', 'SENT')
         RETURNING id",
    )
    .bind(book_id)
    .bind(file_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();

    let recovered = bokhylle_server::delivery::recover(&test_app.state)
        .await
        .unwrap();
    assert_eq!(recovered, 1, "only the interrupted send is recovered");

    let (status, message): (String, Option<String>) =
        sqlx::query_as("SELECT status, error_message FROM deliveries WHERE id = ?")
            .bind(pending_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(status, "FAILED");
    assert!(
        message.as_deref().unwrap_or("").contains("interrupted"),
        "the failure must say it was interrupted: {message:?}"
    );

    let sent_status: String = sqlx::query_scalar("SELECT status FROM deliveries WHERE id = ?")
        .bind(sent_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(sent_status, "SENT", "completed sends are untouched");
}
