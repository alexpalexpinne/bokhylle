use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use bokhylle_acquisition::state::AcquisitionStatus;
use bokhylle_server::auth::Role;
use serde_json::{Value, json};
use tower::ServiceExt;

mod common;

async fn request(
    app: &common::TestApp,
    method: &str,
    uri: &str,
    cookie: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn fixture_server() -> (String, tokio::task::JoinHandle<()>) {
    let fixture = tempfile::tempdir().unwrap();
    let book = bokhylle_library::fixtures::generate_library(fixture.path(), 2)
        .unwrap()
        .into_iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains("Project Hail Mary"))
        })
        .unwrap();
    let bytes = Bytes::from(std::fs::read(book).unwrap());
    let catalogue = r#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dcterms="http://purl.org/dc/terms/">
        <title>Fictional Books</title>
        <entry><id>urn:book:hail-mary</id><title>Project Hail Mary</title>
          <author><name>Andy Weir</name></author><dcterms:language>en</dcterms:language>
          <link rel="http://opds-spec.org/acquisition/open-access"
                type="application/epub+zip" href="/book.epub"/>
        </entry></feed>"#;
    let router = Router::new()
        .route("/catalogue", get(move || async move { catalogue }))
        .route(
            "/book.epub",
            get(move || {
                let bytes = bytes.clone();
                async move { ([(header::CONTENT_TYPE, "application/epub+zip")], bytes) }
            }),
        )
        .route(
            "/redirect",
            get(|| async {
                (
                    StatusCode::FOUND,
                    [(header::LOCATION, "http://127.0.0.1:9/book.epub")],
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (origin, task)
}

#[tokio::test]
async fn opds_acquisition_uses_http_pipeline_and_keeps_url_private() {
    let (origin, server) = fixture_server().await;
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&app, "admin", "password123").await;

    let (status, source) = request(
        &app,
        "POST",
        "/api/catalogues",
        &cookie,
        json!({"name":"Fictional Books","url":format!("{origin}/catalogue")}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let source_id = source["id"].as_str().unwrap();

    let (status, feed) = request(
        &app,
        "GET",
        &format!("/api/catalogues/{source_id}/feed"),
        &cookie,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(feed["entries"][0]["title"], "Project Hail Mary");
    assert_eq!(feed["entries"][0]["files"][0]["format"], "epub");
    assert!(feed["entries"][0]["files"][0].get("url").is_none());

    let (status, acquisition) = request(
        &app,
        "POST",
        &format!("/api/catalogues/{source_id}/acquisitions"),
        &cookie,
        json!({
            "pageUrl": feed["pageUrl"],
            "entryId": "urn:book:hail-mary",
            "fileIndex": 0,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = acquisition["id"].as_str().unwrap();
    let mut final_status = String::new();
    for _ in 0..200 {
        let row: String = sqlx::query_scalar("SELECT status FROM acquisitions WHERE id = ?")
            .bind(id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        final_status = row;
        if matches!(
            final_status.as_str(),
            "READY" | "NEEDS_REVIEW" | "IMPORT_FAILED" | "DOWNLOAD_FAILED"
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(final_status, "READY");
    let file_count: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(file_count, 1);
    let event_details: Vec<Option<String>> =
        sqlx::query_scalar("SELECT detail FROM acquisition_events WHERE acquisition_id = ?")
            .bind(id)
            .fetch_all(&app.state.db)
            .await
            .unwrap();
    assert!(
        event_details
            .iter()
            .flatten()
            .all(|detail| !detail.contains("book.epub"))
    );
    server.abort();
}

#[tokio::test]
async fn untrusted_private_urls_and_cross_origin_redirects_are_rejected() {
    let (origin, server) = fixture_server().await;
    let direct =
        bokhylle_server::remote_http::response(&format!("{origin}/book.epub"), None, None, 1024)
            .await;
    assert!(direct.is_err());
    let redirected = bokhylle_server::remote_http::response(
        &format!("{origin}/redirect"),
        Some(&origin),
        None,
        1024,
    )
    .await;
    assert!(redirected.is_err());
    server.abort();
}

#[tokio::test]
async fn downloading_http_acquisition_resumes_after_recovery() {
    let (origin, server) = fixture_server().await;
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let book_id: i64 = sqlx::query_scalar(
        "INSERT INTO books (title, normalized_title) VALUES ('Project Hail Mary', 'project hail mary') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let author_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('Andy Weir', 'andy weir') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(author_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (acquisition, duplicate) = bokhylle_server::acquisition::create_http_with_languages(
        &app.state.db,
        book_id,
        user.id,
        Some("epub".to_string()),
        vec!["en".to_string()],
        false,
        &format!("{origin}/book.epub"),
        "epub",
        "opds",
        "Fictional Books",
        "urn:book:hail-mary",
        Some(&origin),
    )
    .await
    .unwrap();
    assert!(!duplicate);
    for status in [
        AcquisitionStatus::Searching,
        AcquisitionStatus::Evaluating,
        AcquisitionStatus::Queued,
        AcquisitionStatus::Downloading,
    ] {
        bokhylle_server::acquisition::transition(&app.state.db, &acquisition.id, status, None)
            .await
            .unwrap();
    }
    bokhylle_server::acquisition_pipeline::recover(&app.state).await;
    let mut final_status = String::new();
    for _ in 0..200 {
        final_status = sqlx::query_scalar("SELECT status FROM acquisitions WHERE id = ?")
            .bind(&acquisition.id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
        if matches!(
            final_status.as_str(),
            "READY" | "NEEDS_REVIEW" | "IMPORT_FAILED" | "DOWNLOAD_FAILED"
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(final_status, "READY");
    let staged: Option<String> =
        sqlx::query_scalar("SELECT content_path FROM acquisitions WHERE id = ?")
            .bind(&acquisition.id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    let staged = staged.expect("downloaded artifact path");
    for _ in 0..100 {
        if !std::path::Path::new(&staged).exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(!std::path::Path::new(&staged).exists());
    server.abort();
}

#[tokio::test]
async fn child_cannot_browse_and_restricted_adult_cannot_submit_direct_url() {
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
    let child_cookie = common::login(&app, "child", "password123").await;
    let (status, _) = request(&app, "GET", "/api/catalogues", &child_cookie, Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let adult = app
        .state
        .auth
        .create_user("adult", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET can_acquire = 0 WHERE id = ?")
        .bind(adult.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let adult_cookie = common::login(&app, "adult", "password123").await;
    let (status, _) = request(
        &app,
        "POST",
        "/api/books/1/acquisitions/http",
        &adult_cookie,
        json!({"url":"https://example.org/book.epub"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
