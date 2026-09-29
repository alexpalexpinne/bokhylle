use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;

use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;

mod common;

async fn configure_child_reader(test_app: &common::TestApp, smtp_port: u16) {
    for (key, value) in [
        (
            bokhylle_server::settings::KINDLE_ADDRESS,
            json!("child@kindle.example"),
        ),
        ("smtp.host", json!("127.0.0.1")),
        ("smtp.port", json!(smtp_port)),
        ("smtp.tls", json!("none")),
        ("smtp.from", json!("library@example.com")),
    ] {
        test_app.state.settings.set(key, &value).await.unwrap();
    }
}

async fn start_smtp() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let messages = Arc::new(Mutex::new(Vec::new()));
    let received = messages.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let received = received.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = tokio::io::split(socket);
                let mut lines = BufReader::new(reader).lines();
                let mut in_data = false;
                let mut body = String::new();
                writer.write_all(b"220 fake ESMTP\r\n").await.ok();
                while let Ok(Some(line)) = lines.next_line().await {
                    if in_data {
                        if line == "." {
                            received.lock().unwrap().push(body.clone());
                            body.clear();
                            in_data = false;
                            writer.write_all(b"250 Ok\r\n").await.ok();
                        } else {
                            body.push_str(&line);
                            body.push('\n');
                        }
                    } else if line.starts_with("EHLO") || line.starts_with("HELO") {
                        writer.write_all(b"250-fake\r\n250 OK\r\n").await.ok();
                    } else if line == "DATA" {
                        writer.write_all(b"354 Send data\r\n").await.ok();
                        in_data = true;
                    } else if line == "QUIT" {
                        writer.write_all(b"221 Bye\r\n").await.ok();
                        break;
                    } else {
                        writer.write_all(b"250 OK\r\n").await.ok();
                    }
                }
            });
        }
    });
    (port, messages)
}

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

async fn app() -> common::TestApp {
    let provider = Arc::new(FakeMetadataProvider::new(vec![
        MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLREQUESTW".to_string(),
            title: "Requested Book".to_string(),
            authors: vec!["Request Author".to_string()],
            language: Some("en".to_string()),
            year: Some(2024),
            ..Default::default()
        },
        MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLREQUEST2W".to_string(),
            title: "Second Request Book".to_string(),
            authors: vec!["Request Author".to_string()],
            language: Some("en".to_string()),
            year: Some(2024),
            ..Default::default()
        },
    ]));
    let test_app = common::test_app_with_metadata(provider).await;
    test_app
        .state
        .auth
        .create_user("parent", "password123", Role::Admin)
        .await
        .unwrap();
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
    for id in [emma_id, alex_id] {
        sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
            .bind(id)
            .execute(&test_app.state.db)
            .await
            .unwrap();
    }
    test_app
}

#[tokio::test]
async fn children_request_and_adults_approve_or_decline() {
    let test_app = app().await;
    let (smtp_port, _messages) = start_smtp().await;
    configure_child_reader(&test_app, smtp_port).await;
    let parent = common::login(&test_app, "parent", "password123").await;
    let emma = common::login(&test_app, "emma", "password123").await;
    let alex = common::login(&test_app, "alex", "password123").await;

    // The request catalogue is metadata-only: no ownership, status or
    // availability fields.
    let (status, results) = request(
        &test_app,
        "GET",
        "/api/requests/search?q=requested&type=any",
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let hit = results["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["title"] == "Requested Book")
        .expect("provider result");
    for leaked in ["ownedBookId", "ownedFileId", "onShelf", "status"] {
        assert!(
            hit.get(leaked).is_none(),
            "the request catalogue must not expose {leaked}: {hit}"
        );
    }

    let (status, created) = request(
        &test_app,
        "POST",
        "/api/requests",
        &emma,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let request_id = created["request"]["id"].as_i64().unwrap();
    assert_eq!(created["request"]["status"], "requested");
    assert_eq!(created["request"]["phase"], "requested");
    assert_eq!(created["request"]["requester"], "emma");

    // Asking twice returns the pending request instead of duplicating it.
    let (status, duplicate) = request(
        &test_app,
        "POST",
        "/api/requests",
        &emma,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["request"]["id"], request_id);

    // A child cannot decide, and cannot see another child's requests.
    let (status, _) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{request_id}/approve"),
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, alex_requests) = request(&test_app, "GET", "/api/requests", &alex, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        alex_requests["items"].as_array().unwrap().is_empty(),
        "children only see their own requests: {alex_requests}"
    );

    // The notifications payload carries the pending-request count that the
    // navigation uses to decide whether Requests is worth showing.
    let (status, parent_notifications) =
        request(&test_app, "GET", "/api/notifications", &parent, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(parent_notifications["pendingRequests"], 1);
    assert_eq!(
        parent_notifications["pendingRequestItems"][0]["requester"], "emma",
        "the approval menu needs who asked: {parent_notifications}"
    );
    assert_eq!(
        parent_notifications["pendingRequestItems"][0]["title"],
        "Requested Book"
    );
    let (status, emma_notifications) =
        request(&test_app, "GET", "/api/notifications", &emma, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(emma_notifications["pendingRequests"], 1);
    assert_eq!(
        emma_notifications["pendingRequestItems"][0]["requester"],
        "emma"
    );

    // The adult sees it and approves; the normal acquisition takes over.
    let (status, listed) = request(&test_app, "GET", "/api/requests", &parent, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == request_id),
        "the household inbox lists pending requests: {listed}"
    );

    let (status, approved) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{request_id}/approve"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(approved["request"]["status"], "approved");
    assert_eq!(approved["request"]["phase"], "looking");
    let acquisition_id = approved["request"]["acquisitionId"].as_str().unwrap();

    let (status, parent_notifications) =
        request(&test_app, "GET", "/api/notifications", &parent, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        parent_notifications["pendingRequests"], 0,
        "a decided request is no longer pending work"
    );
    assert!(
        parent_notifications["pendingRequestItems"]
            .as_array()
            .unwrap()
            .is_empty(),
        "a decided request leaves the approval menu: {parent_notifications}"
    );

    let (requester, source, deliver_on_ready): (i64, String, i64) = sqlx::query_as(
        "SELECT ar.user_id, ar.source, ar.deliver_on_ready FROM acquisition_requests ar WHERE ar.acquisition_id = ?",
    )
    .bind(acquisition_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(requester, 2, "the child is the requester");
    assert_eq!(source, "book_request");
    assert_eq!(deliver_on_ready, 1, "approval must send the acquired book");

    let on_shelf: i64 =
        sqlx::query_scalar("SELECT count(*) FROM user_books WHERE user_id = 2 AND on_shelf = 1")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(on_shelf, 1, "approval puts the book on the child's shelf");

    let (status, _) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{request_id}/approve"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // The requester is notified, and so were the adults.
    let kid_notifications: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM notifications WHERE user_id = 2 ORDER BY id")
            .fetch_all(&test_app.state.db)
            .await
            .unwrap();
    assert!(
        kid_notifications.contains(&"approved".to_string()),
        "{kid_notifications:?}"
    );
    let adult_notifications: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM notifications WHERE user_id = 1 ORDER BY id")
            .fetch_all(&test_app.state.db)
            .await
            .unwrap();
    assert!(
        adult_notifications.contains(&"request".to_string()),
        "{adult_notifications:?}"
    );

    // A second child's request can be declined without creating work.
    let (_, alex_created) = request(
        &test_app,
        "POST",
        "/api/requests",
        &alex,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" })),
    )
    .await;
    let alex_request = alex_created["request"]["id"].as_i64().unwrap();
    let (status, declined) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{alex_request}/decline"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(declined["request"]["phase"], "declined");
    let acquisitions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM acquisitions WHERE book_id = 1")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(acquisitions, 1, "a declined request creates no acquisition");
}

#[tokio::test]
async fn child_approval_waits_for_reader_setup() {
    let test_app = app().await;
    let parent = common::login(&test_app, "parent", "password123").await;
    let emma = common::login(&test_app, "emma", "password123").await;
    let (_, created) = request(
        &test_app,
        "POST",
        "/api/requests",
        &emma,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" })),
    )
    .await;
    let request_id = created["request"]["id"].as_i64().unwrap();

    let (status, error) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{request_id}/approve"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error["message"].as_str().unwrap().contains("reader"));

    test_app
        .state
        .settings
        .set(
            bokhylle_server::settings::KINDLE_ADDRESS,
            &json!("child@kindle.example"),
        )
        .await
        .unwrap();
    let (status, error) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{request_id}/approve"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error["message"].as_str().unwrap().contains("SMTP"));

    let pending: String = sqlx::query_scalar("SELECT status FROM book_requests WHERE id = ?")
        .bind(request_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(
        pending, "requested",
        "missing delivery setup cannot consume approval"
    );
}

#[tokio::test]
async fn a_child_without_request_access_keeps_their_existing_requests() {
    let test_app = app().await;
    let parent = common::login(&test_app, "parent", "password123").await;
    let emma = common::login(&test_app, "emma", "password123").await;
    let emma_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'emma'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    // A pending request exists before access is switched off.
    let (status, created) = request(
        &test_app,
        "POST",
        "/api/requests",
        &emma,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let request_id = created["request"]["id"].as_i64().unwrap();

    let (status, updated) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{emma_id}"),
        &parent,
        Some(json!({ "canRequest": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["canRequest"], false);

    let (status, _) = request(
        &test_app,
        "GET",
        "/api/requests/search?q=requested&type=any",
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the catalogue is refused");
    let (status, _) = request(
        &test_app,
        "POST",
        "/api/requests",
        &emma,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUEST2W" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "new requests are refused");

    // Their own requests remain visible, and an adult can still decide them.
    let (status, listed) = request(&test_app, "GET", "/api/requests", &emma, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == request_id),
        "existing requests survive the switch: {listed}"
    );
    let (status, _) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{request_id}/decline"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "adults can still decide");

    // The profile reflects the switch, and it can be turned back on.
    let (status, me) = request(&test_app, "GET", "/api/auth/me", &emma, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["canRequest"], false);
    let (status, updated) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{emma_id}"),
        &parent,
        Some(json!({ "canRequest": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["canRequest"], true);
    let (status, results) = request(
        &test_app,
        "GET",
        "/api/requests/search?q=requested&type=any",
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!results["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn child_discover_and_request_permissions_are_independent() {
    let test_app = app().await;
    let parent = common::login(&test_app, "parent", "password123").await;
    let emma = common::login(&test_app, "emma", "password123").await;
    let emma_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'emma'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    let (status, me) = request(&test_app, "GET", "/api/auth/me", &emma, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["user"]["canDiscover"], false);
    assert_eq!(me["user"]["canRequest"], true);
    let (status, _) = request(
        &test_app,
        "GET",
        "/api/requests/book?provider=fake&providerKey=%2Fworks%2FOLREQUESTW",
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, updated) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{emma_id}"),
        &parent,
        Some(json!({ "canDiscover": true, "canRequest": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["canDiscover"], true);
    assert_eq!(updated["canRequest"], false);

    let (status, results) = request(
        &test_app,
        "GET",
        "/api/requests/search?q=requested",
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!results["items"].as_array().unwrap().is_empty());
    let (status, detail) = request(
        &test_app,
        "GET",
        "/api/requests/book?provider=fake&providerKey=%2Fworks%2FOLREQUESTW",
        &emma,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["title"], "Requested Book");
    for leaked in [
        "ownedBookId",
        "ownedFileId",
        "onShelf",
        "status",
        "releases",
    ] {
        assert!(
            detail.get(leaked).is_none(),
            "detail leaked {leaked}: {detail}"
        );
    }

    let (status, _) = request(
        &test_app,
        "POST",
        "/api/requests",
        &emma,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for path in [
        "/api/discover/search?q=requested",
        "/api/discover/releases?providerKey=x",
    ] {
        let (status, _) = request(&test_app, "GET", path, &emma, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }

    let (status, _) = request(
        &test_app,
        "PUT",
        &format!("/api/admin/users/{emma_id}"),
        &parent,
        Some(json!({ "canDiscover": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for path in [
        "/api/requests/search?q=requested",
        "/api/requests/book?provider=fake&providerKey=%2Fworks%2FOLREQUESTW",
    ] {
        let (status, _) = request(&test_app, "GET", path, &emma, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn administrator_approval_reuses_a_household_copy_and_manages_the_rest() {
    let test_app = app().await;
    let (smtp_port, messages) = start_smtp().await;
    configure_child_reader(&test_app, smtp_port).await;
    let emma = common::login(&test_app, "emma", "password123").await;
    let alex = common::login(&test_app, "alex", "password123").await;
    // A second administrator handles the request; an ordinary adult cannot.
    let mum = test_app
        .state
        .auth
        .create_user("mum", "password123", Role::Admin)
        .await
        .unwrap();
    let mum_cookie = common::login(&test_app, "mum", "password123").await;
    let outsider = test_app
        .state
        .auth
        .create_user("outsider", "password123", Role::User)
        .await
        .unwrap();
    let _ = outsider;
    let outsider_cookie = common::login(&test_app, "outsider", "password123").await;

    // The household already owns the first book, in a language the child reads.
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLREQUESTW".to_string(),
            title: "Requested Book".to_string(),
            authors: vec!["Request Author".to_string()],
            language: Some("en".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE editions SET language = 'en' WHERE book_id = ?")
        .bind(book_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
        .bind(book_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let copy_dir = tempfile::tempdir().unwrap();
    let copy_path = copy_dir.path().join("request-copy.epub");
    std::fs::write(&copy_path, b"fixture book").unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, 'epub', 12, 'request-copy')",
    )
    .bind(edition_id)
    .bind(copy_path.to_str().unwrap())
    .execute(&test_app.state.db)
    .await
    .unwrap();

    let (_, created) = request(
        &test_app,
        "POST",
        "/api/requests",
        &emma,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" })),
    )
    .await;
    let request_id = created["request"]["id"].as_i64().unwrap();

    let (status, outsider_requests) =
        request(&test_app, "GET", "/api/requests", &outsider_cookie, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(outsider_requests["items"].as_array().unwrap().is_empty());
    let (status, outsider_notifications) = request(
        &test_app,
        "GET",
        "/api/notifications",
        &outsider_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(outsider_notifications["pendingRequests"], 0);
    for decision in ["approve", "decline"] {
        let (status, _) = request(
            &test_app,
            "POST",
            &format!("/api/requests/{request_id}/{decision}"),
            &outsider_cookie,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    let (status, approved) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{request_id}/approve"),
        &mum_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    assert_eq!(approved["request"]["phase"], "ready");
    assert!(
        approved["request"]["acquisitionId"].is_null(),
        "an owned copy settles the request without an acquisition: {approved}"
    );
    let acquisitions: i64 = sqlx::query_scalar("SELECT count(*) FROM acquisitions")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(acquisitions, 0, "no download work for an owned copy");
    let (on_shelf, source): (i64, String) =
        sqlx::query_as("SELECT on_shelf, source FROM user_books WHERE user_id = 2 AND book_id = ?")
            .bind(book_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(on_shelf, 1, "the child's shelf receives the owned copy");
    assert_eq!(source, "book_request");
    assert_eq!(approved["request"]["deliveryStatus"], "SENT");
    let deliveries: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT user_id, address, status FROM deliveries WHERE book_id = ?")
            .bind(book_id)
            .fetch_all(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(
        deliveries,
        vec![(2, "child@kindle.example".into(), "SENT".into())]
    );
    assert_eq!(
        messages.lock().unwrap().len(),
        1,
        "the owned copy was emailed"
    );
    let ready_notified: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications WHERE user_id = 2 AND kind = 'sent'",
    )
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(ready_notified, 1, "the child is told it was sent");

    // A book the household does not own still goes through the pipeline, and
    // the approving adult becomes its manager.
    let (_, second) = request(
        &test_app,
        "POST",
        "/api/requests",
        &alex,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUEST2W" })),
    )
    .await;
    let second_request = second["request"]["id"].as_i64().unwrap();
    let (status, approved) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{second_request}/approve"),
        &mum_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let acquisition_id = approved["request"]["acquisitionId"]
        .as_str()
        .unwrap()
        .to_string();
    let deliver_on_ready: i64 = sqlx::query_scalar(
        "SELECT deliver_on_ready FROM acquisition_requests WHERE acquisition_id = ? AND user_id = 3",
    )
    .bind(&acquisition_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(
        deliver_on_ready, 1,
        "the missing copy will be sent after import"
    );

    let (status, view) = request(
        &test_app,
        "GET",
        &format!("/api/acquisitions/{acquisition_id}"),
        &mum_cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the approver can open it: {view}");
    assert_eq!(
        view["managedByMe"], true,
        "the view marks the approver as its manager: {view}"
    );
    let (status, listed) = request(&test_app, "GET", "/api/acquisitions", &mum_cookie, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == acquisition_id.as_str()),
        "the approver's activity includes the managed acquisition: {listed}"
    );
    let (status, _) = request(
        &test_app,
        "POST",
        &format!("/api/acquisitions/{acquisition_id}/keep-looking"),
        &mum_cookie,
        Some(json!({ "enabled": true })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the manager can stop or resume retries"
    );
    let (status, _) = request(
        &test_app,
        "GET",
        &format!("/api/acquisitions/{acquisition_id}"),
        &outsider_cookie,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "another adult still cannot manage it"
    );
    let _ = mum;
}

#[tokio::test]
async fn reader_can_ask_admin_to_add_a_shared_book_without_seeing_child_requests() {
    let test_app = app().await;
    test_app
        .state
        .auth
        .create_user("guest", "password123", Role::User)
        .await
        .unwrap();
    let guest_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'guest'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET can_acquire = 0 WHERE id = ?")
        .bind(guest_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let guest = common::login(&test_app, "guest", "password123").await;
    let child = common::login(&test_app, "emma", "password123").await;
    let parent = common::login(&test_app, "parent", "password123").await;

    let (_, child_request) = request(
        &test_app,
        "POST",
        "/api/requests",
        &child,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" })),
    )
    .await;
    let child_id = child_request["request"]["id"].as_i64().unwrap();
    let (status, own_request) = request(
        &test_app,
        "POST",
        "/api/requests",
        &guest,
        Some(json!({ "provider": "fake", "providerKey": "/works/OLREQUEST2W" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let own_id = own_request["request"]["id"].as_i64().unwrap();

    let (status, visible) = request(&test_app, "GET", "/api/requests", &guest, None).await;
    assert_eq!(status, StatusCode::OK);
    let items = visible["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], own_id);
    let (status, notifications) =
        request(&test_app, "GET", "/api/notifications", &guest, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(notifications["pendingRequests"], 1);
    assert_eq!(notifications["pendingRequestItems"][0]["id"], own_id);
    let (status, _) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{child_id}/approve"),
        &guest,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, approved) = request(
        &test_app,
        "POST",
        &format!("/api/requests/{own_id}/approve"),
        &parent,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{approved}");
    let book_id = approved["request"]["bookId"].as_i64().unwrap();
    assert!(
        bokhylle_server::user_books::contains(&test_app.state.db, guest_id, book_id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn concurrent_requests_return_one_pending_row() {
    let test_app = app().await;
    let emma = common::login(&test_app, "emma", "password123").await;
    let body =
        serde_json::to_vec(&json!({ "provider": "fake", "providerKey": "/works/OLREQUESTW" }))
            .unwrap();

    let mut handles = Vec::new();
    for _ in 0..5 {
        let router = test_app.router.clone();
        let cookie = emma.clone();
        let body = body.clone();
        handles.push(tokio::spawn(async move {
            let response = router
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/requests")
                        .header(header::COOKIE, cookie)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            (status, serde_json::from_slice::<Value>(&bytes).unwrap())
        }));
    }

    let mut ids = Vec::new();
    for handle in handles {
        let (status, value) = handle.await.unwrap();
        assert!(
            status.is_success(),
            "a concurrent duplicate must not error: {status} {value}"
        );
        ids.push(value["request"]["id"].as_i64().unwrap());
    }
    assert!(
        ids.windows(2).all(|pair| pair[0] == pair[1]),
        "every caller sees the canonical request: {ids:?}"
    );
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM book_requests WHERE status = 'requested'")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(rows, 1);
}
