use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bokhylle_server::auth::Role;
use md5::Digest;
use serde_json::{Value, json};
use std::io::Write;
use tower::ServiceExt;

mod common;

async fn request(
    app: &common::TestApp,
    method: &str,
    uri: &str,
    cookie: &str,
    body: Option<Value>,
    range: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie);
    }
    if let Some(range) = range {
        builder = builder.header(header::RANGE, range);
    }
    let body = if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    app.router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn seed_file(app: &common::TestApp, path: &str, format: &str, sha: &str) -> (i64, i64) {
    let book_id: i64 = sqlx::query_scalar(
        "INSERT INTO books (title, normalized_title) VALUES ('Reader Book', 'reader book') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let edition_id: i64 = sqlx::query_scalar(
        "INSERT INTO editions (book_id, title) VALUES (?, 'Reader Book') RETURNING id",
    )
    .bind(book_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let file_id: i64 = sqlx::query_scalar(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, ?, 20, ?) RETURNING id",
    )
    .bind(edition_id)
    .bind(path)
    .bind(format)
    .bind(sha)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    (book_id, file_id)
}

fn content_url(book_id: i64, file_id: i64) -> String {
    format!("/api/books/{book_id}/files/{file_id}/content")
}

fn position_url(book_id: i64, file_id: i64) -> String {
    format!("/api/books/{book_id}/files/{file_id}/position")
}

fn direction_url(book_id: i64, file_id: i64) -> String {
    format!("/api/books/{book_id}/files/{file_id}/direction")
}

#[tokio::test]
async fn reading_direction_has_a_shared_default_and_shelf_scoped_profile_overrides() {
    let library = tempfile::tempdir().unwrap();
    let epub = library.path().join("Fictional Book.epub");
    std::fs::write(&epub, b"fictional-epub-bytes").unwrap();
    let app = common::test_app_with_library_root(library.path().to_path_buf()).await;
    for (name, role) in [
        ("parent", Role::Admin),
        ("second", Role::User),
        ("kid", Role::User),
    ] {
        app.state
            .auth
            .create_user(name, "password123", role)
            .await
            .unwrap();
    }
    let parent = common::login(&app, "parent", "password123").await;
    let second = common::login(&app, "second", "password123").await;
    let kid = common::login(&app, "kid", "password123").await;
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(kid_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (book_id, file_id) = seed_file(&app, epub.to_str().unwrap(), "epub", "sha").await;
    let direction = direction_url(book_id, file_id);
    let position = position_url(book_id, file_id);

    assert_eq!(
        request(
            &app,
            "PUT",
            &direction,
            &kid,
            Some(json!({"direction": "rtl"})),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    bokhylle_server::user_books::add(&app.state.db, kid_id, book_id, "parent_assigned")
        .await
        .unwrap();
    let changed = request(
        &app,
        "PUT",
        &direction,
        &kid,
        Some(json!({"direction": "rtl"})),
        None,
    )
    .await;
    assert_eq!(changed.status(), StatusCode::OK);
    assert_eq!(json_body(changed).await["directionOverride"], "rtl");
    assert_eq!(
        json_body(request(&app, "GET", &position, &kid, None, None).await).await["direction"],
        json!({"bookDirection": null, "seriesDirection": null, "directionOverride": "rtl"})
    );
    assert_eq!(
        json_body(request(&app, "GET", &position, &second, None, None).await).await["direction"],
        json!({"bookDirection": null, "seriesDirection": null, "directionOverride": null})
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &direction,
            &kid,
            Some(json!({"direction": "up"})),
            None
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let admin_url = format!("/api/admin/books/{book_id}");
    assert_eq!(
        request(
            &app,
            "PUT",
            &admin_url,
            &parent,
            Some(json!({"readingDirection": "rtl"})),
            None,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        json_body(request(&app, "GET", &position, &second, None, None).await).await["direction"],
        json!({"bookDirection": "rtl", "seriesDirection": null, "directionOverride": null})
    );
    assert_eq!(
        json_body(request(&app, "GET", &position, &kid, None, None).await).await["direction"],
        json!({"bookDirection": "rtl", "seriesDirection": null, "directionOverride": "rtl"})
    );
    let reset = request(
        &app,
        "PUT",
        &direction,
        &kid,
        Some(json!({"direction": null})),
        None,
    )
    .await;
    assert_eq!(json_body(reset).await["directionOverride"], Value::Null);
    let series_id: i64 = sqlx::query_scalar(
        "INSERT INTO series (name, default_reading_direction) VALUES ('Imaginary Cycle', 'rtl') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let inherited = request(
        &app,
        "PUT",
        &admin_url,
        &parent,
        Some(json!({"readingDirection": null, "seriesId": series_id})),
        None,
    )
    .await;
    assert_eq!(inherited.status(), StatusCode::OK);
    assert_eq!(
        json_body(request(&app, "GET", &position, &second, None, None).await).await["direction"],
        json!({"bookDirection": null, "seriesDirection": "rtl", "directionOverride": null})
    );
    sqlx::query("DELETE FROM user_books WHERE user_id = ? AND book_id = ?")
        .bind(kid_id)
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "PUT",
            &direction,
            &kid,
            Some(json!({"direction": "ltr"})),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

fn update(sha: &str, locator: &str, percentage: f64, revision: i64) -> Value {
    json!({
        "sha256": sha,
        "locator": locator,
        "percentage": percentage,
        "completed": false,
        "expectedRevision": revision,
    })
}

#[tokio::test]
async fn explicit_book_completion_survives_passive_saves_and_rejects_stale_tabs() {
    let library = tempfile::tempdir().unwrap();
    let epub = library.path().join("Fictional Finish.epub");
    std::fs::write(&epub, b"fictional-epub-bytes").unwrap();
    let app = common::test_app_with_library_root(library.path().to_path_buf()).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let (book_id, file_id) = seed_file(&app, epub.to_str().unwrap(), "epub", "sha-one").await;
    let position = position_url(book_id, file_id);
    let completion = format!("/api/books/{book_id}/completion");

    let mut finished = update("sha-one", "epubcfi(/6/2)", 0.9, 0);
    finished["completed"] = json!(true);
    let saved = request(&app, "PUT", &position, &cookie, Some(finished), None).await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(json_body(saved).await["bookCompleted"], true);
    let passive = request(
        &app,
        "PUT",
        &position,
        &cookie,
        Some(update("sha-one", "epubcfi(/6/4)", 0.4, 1)),
        None,
    )
    .await;
    assert_eq!(passive.status(), StatusCode::OK);
    assert_eq!(json_body(passive).await["bookCompleted"], true);

    let reset = request(
        &app,
        "PUT",
        &completion,
        &cookie,
        Some(json!({"completed": false})),
        None,
    )
    .await;
    assert_eq!(reset.status(), StatusCode::OK);
    assert_eq!(json_body(reset).await["completed"], false);
    assert_eq!(
        request(
            &app,
            "PUT",
            &position,
            &cookie,
            Some(json!({"sha256": "sha-one", "locator": "epubcfi(/6/6)", "percentage": 0.9, "completed": true, "expectedRevision": 2})),
            None,
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let latest = json_body(request(&app, "GET", &position, &cookie, None, None).await).await;
    assert_eq!(latest["bookCompleted"], false);
    assert_eq!(latest["position"]["completed"], false);
    assert_eq!(latest["position"]["revision"], 3);
}

fn write_comic(path: &std::path::Path) {
    let mut archive = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for name in ["page10.png", "page2.png", "page1.png"] {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"\x89PNG\r\n\x1a\nfictional").unwrap();
    }
    archive.finish().unwrap();
}

#[tokio::test]
async fn comic_pages_and_positions_are_child_shelf_scoped() {
    let library = tempfile::tempdir().unwrap();
    let cbz = library.path().join("Fictional Comic.cbz");
    write_comic(&cbz);
    let app = common::test_app_with_library_root(library.path().to_path_buf()).await;
    app.state
        .auth
        .create_user("kid", "password123", Role::User)
        .await
        .unwrap();
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(kid_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let kid = common::login(&app, "kid", "password123").await;
    let (book_id, file_id) = seed_file(&app, cbz.to_str().unwrap(), "cbz", "comic-sha").await;
    let manifest = format!("/api/books/{book_id}/files/{file_id}/pages");
    let page = format!("{manifest}/2");
    let position = position_url(book_id, file_id);
    for url in [&manifest, &page, &position] {
        assert_eq!(
            request(&app, "GET", url, &kid, None, None).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    bokhylle_server::user_books::add(&app.state.db, kid_id, book_id, "parent_assigned")
        .await
        .unwrap();
    let response = request(&app, "GET", &manifest, &kid, None, None).await;
    assert_eq!(json_body(response).await["pages"], 3);
    let response = request(&app, "GET", &page, &kid, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &position,
            &kid,
            Some(update("comic-sha", "2", 0.66, 0)),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", &position, &kid, None, None)
            .await
            .status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM user_books WHERE user_id = ? AND book_id = ?")
        .bind(kid_id)
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    for url in [&manifest, &page, &position] {
        assert_eq!(
            request(&app, "GET", url, &kid, None, None).await.status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn content_is_inline_ranged_and_private_for_epub_and_pdf() {
    let library = tempfile::tempdir().unwrap();
    let epub = library.path().join("sample.epub");
    std::fs::write(&epub, b"fictional-epub-bytes").unwrap();
    let pdf = library.path().join("sample.pdf");
    std::fs::write(&pdf, b"fictional-pdf-bytes").unwrap();
    let app = common::test_app_with_library_root(library.path().to_path_buf()).await;
    app.state
        .auth
        .create_user("adult", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "adult", "password123").await;
    let (book_id, file_id) = seed_file(&app, epub.to_str().unwrap(), "epub", "sha-one").await;
    let (pdf_book, pdf_file) = seed_file(&app, pdf.to_str().unwrap(), "pdf", "sha-two").await;

    let response = request(
        &app,
        "GET",
        &content_url(book_id, file_id),
        &cookie,
        None,
        Some("bytes=0-8"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/epub+zip"
    );
    assert_eq!(response.headers()[header::CONTENT_DISPOSITION], "inline");
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 0-8/20");
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(
        to_bytes(response.into_body(), usize::MAX).await.unwrap(),
        "fictional".as_bytes()
    );

    let pdf_response = request(
        &app,
        "GET",
        &content_url(pdf_book, pdf_file),
        &cookie,
        None,
        Some("bytes=0-8"),
    )
    .await;
    assert_eq!(pdf_response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        pdf_response.headers()[header::CONTENT_TYPE],
        "application/pdf"
    );
    assert_eq!(
        pdf_response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    assert_eq!(
        pdf_response.headers()[header::CONTENT_RANGE],
        "bytes 0-8/19"
    );
    assert_eq!(
        request(&app, "GET", &content_url(book_id, file_id), "", None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    std::fs::remove_file(epub).unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            &content_url(book_id, file_id),
            &cookie,
            None,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn pdf_pages_are_file_bound_and_validated() {
    let library = tempfile::tempdir().unwrap();
    let pdf = library.path().join("sample.pdf");
    std::fs::write(&pdf, b"fictional-pdf-bytes").unwrap();
    let app = common::test_app_with_library_root(library.path().to_path_buf()).await;
    app.state
        .auth
        .create_user("adult", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "adult", "password123").await;
    let (book_id, file_id) = seed_file(&app, pdf.to_str().unwrap(), "pdf", "pdf-sha").await;
    let url = position_url(book_id, file_id);
    let initial = json_body(request(&app, "GET", &url, &cookie, None, None).await).await;
    assert_eq!(initial["format"], "pdf");
    for locator in ["0", "-1", "1.5", "epubcfi(/6/2)", "01"] {
        assert_eq!(
            request(
                &app,
                "PUT",
                &url,
                &cookie,
                Some(update("pdf-sha", locator, 0.5, 0)),
                None
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let saved = request(
        &app,
        "PUT",
        &url,
        &cookie,
        Some(update("pdf-sha", "2", 0.5, 0)),
        None,
    )
    .await;
    assert_eq!(json_body(saved).await["position"]["locator"], "2");
    let document = bokhylle_server::partial_md5(&pdf).unwrap();
    let synced: (String, String, i64) =
        sqlx::query_as("SELECT locator, source, revision FROM reading_progress WHERE document = ?")
            .bind(&document)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(synced, ("2".into(), "bokhylle".into(), 1));
    let adult_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'adult'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let (_, token) = bokhylle_server::reader_tokens::create(&app.state.db, adult_id, "KOReader")
        .await
        .unwrap();
    let key = hex::encode(md5::Md5::digest(token.as_bytes()));
    let pulled = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/syncs/progress/{document}"))
                .header("x-auth-user", "adult")
                .header("x-auth-key", &key)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(pulled).await["progress"], "2");
    let pushed = app.router.clone().oneshot(
        Request::builder().method("PUT").uri("/syncs/progress")
            .header("x-auth-user", "adult").header("x-auth-key", &key)
            .body(Body::from(json!({
                "document": document, "progress": "3", "percentage": 0.75, "device": "KOReader",
            }).to_string())).unwrap(),
    ).await.unwrap();
    assert_eq!(pushed.status(), StatusCode::OK);
    let state = json_body(request(&app, "GET", &url, &cookie, None, None).await).await;
    assert_eq!(state["external"]["locator"], "3");
    assert_eq!(state["external"]["revision"], 2);
    assert_eq!(
        request(
            &app,
            "PUT",
            &url,
            &cookie,
            Some(update("pdf-sha", "4", 0.9, 1)),
            None
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE book_files SET sha256 = 'new-pdf-sha' WHERE id = ?")
        .bind(file_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        json_body(request(&app, "GET", &url, &cookie, None, None).await).await["position"],
        Value::Null
    );
}

#[tokio::test]
async fn positions_are_profile_scoped_and_revision_checked() {
    let library = tempfile::tempdir().unwrap();
    let epub = library.path().join("sample.epub");
    std::fs::write(&epub, b"fictional-epub-bytes").unwrap();
    let app = common::test_app_with_library_root(library.path().to_path_buf()).await;
    for name in ["first", "second"] {
        app.state
            .auth
            .create_user(name, "password123", Role::User)
            .await
            .unwrap();
    }
    let first = common::login(&app, "first", "password123").await;
    let second = common::login(&app, "second", "password123").await;
    let (book_id, file_id) = seed_file(&app, epub.to_str().unwrap(), "epub", "sha-one").await;
    let url = position_url(book_id, file_id);

    let initial = request(&app, "GET", &url, &first, None, None).await;
    assert_eq!(initial.status(), StatusCode::OK);
    assert_eq!(json_body(initial).await["position"], Value::Null);
    let saved = request(
        &app,
        "PUT",
        &url,
        &first,
        Some(update("sha-one", "epubcfi(/6/2)", 0.25, 0)),
        None,
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let saved = json_body(saved).await;
    assert_eq!(saved["position"]["revision"], 1);
    assert_eq!(saved["position"]["locator"], "epubcfi(/6/2)");
    assert_eq!(
        json_body(request(&app, "GET", &url, &second, None, None).await).await["position"],
        Value::Null
    );

    let stale = request(
        &app,
        "PUT",
        &url,
        &first,
        Some(update("sha-one", "epubcfi(/6/4)", 0.5, 0)),
        None,
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let saved = request(
        &app,
        "PUT",
        &url,
        &first,
        Some(update("sha-one", "epubcfi(/6/4)", 0.5, 1)),
        None,
    )
    .await;
    assert_eq!(json_body(saved).await["position"]["revision"], 2);
    assert_eq!(
        request(
            &app,
            "PUT",
            &url,
            &first,
            Some(update("sha-one", "epubcfi(/6/6)", 0.75, 1)),
            None,
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    for invalid in [
        update("sha-one", "", 0.5, 2),
        update("sha-one", "epubcfi(/6/6)", 1.5, 2),
        update("sha-one", &"x".repeat(4097), 0.5, 2),
    ] {
        assert_eq!(
            request(&app, "PUT", &url, &first, Some(invalid), None)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }

    // A rescan can replace bytes at the same file id. The old CFI is never
    // returned, and the new identity starts with an expected revision of zero.
    sqlx::query("UPDATE book_files SET sha256 = 'sha-new' WHERE id = ?")
        .bind(file_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        json_body(request(&app, "GET", &url, &first, None, None).await).await["position"],
        Value::Null
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &url,
            &first,
            Some(update("sha-one", "epubcfi(/6/8)", 0.8, 2)),
            None,
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let saved = request(
        &app,
        "PUT",
        &url,
        &first,
        Some(update("sha-new", "epubcfi(/6/2)", 0.1, 0)),
        None,
    )
    .await;
    assert_eq!(json_body(saved).await["position"]["revision"], 3);
}

#[tokio::test]
async fn child_can_read_assigned_epub_until_shelf_access_is_removed() {
    let library = tempfile::tempdir().unwrap();
    let epub = library.path().join("sample.epub");
    std::fs::write(&epub, b"fictional-epub-bytes").unwrap();
    let app = common::test_app_with_library_root(library.path().to_path_buf()).await;
    app.state
        .auth
        .create_user("kid", "password123", Role::User)
        .await
        .unwrap();
    let kid_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'kid'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(kid_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let kid = common::login(&app, "kid", "password123").await;
    let (book_id, file_id) = seed_file(&app, epub.to_str().unwrap(), "epub", "sha-one").await;
    let content = content_url(book_id, file_id);
    let position = position_url(book_id, file_id);

    for url in [&content, &position] {
        assert_eq!(
            request(&app, "GET", url, &kid, None, None).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            &position,
            &kid,
            Some(update("sha-one", "epubcfi(/6/2)", 0.1, 0)),
            None,
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    bokhylle_server::user_books::add(&app.state.db, kid_id, book_id, "parent_assigned")
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", &content, &kid, None, None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", &position, &kid, None, None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &position,
            &kid,
            Some(update("sha-one", "epubcfi(/6/2)", 0.1, 0)),
            None,
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/api/books/{book_id}/files/{file_id}/download"),
            &kid,
            None,
            None,
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    sqlx::query("DELETE FROM user_books WHERE user_id = ? AND book_id = ?")
        .bind(kid_id)
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    for url in [&content, &position] {
        assert_eq!(
            request(&app, "GET", url, &kid, None, None).await.status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn continue_reading_merges_sources_without_guessing_a_browser_position() {
    let library = tempfile::tempdir().unwrap();
    let epub = library.path().join("sample.epub");
    std::fs::write(&epub, b"fictional-epub-bytes").unwrap();
    let app = common::test_app_with_library_root(library.path().to_path_buf()).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'reader'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let (book_id, file_id) = seed_file(&app, epub.to_str().unwrap(), "epub", "sha-one").await;
    let saved = request(
        &app,
        "PUT",
        &position_url(book_id, file_id),
        &cookie,
        Some(update("sha-one", "epubcfi(/6/2)", 0.35, 0)),
        None,
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);

    sqlx::query(
        "INSERT INTO reading_progress
         (user_id, document, book_id, book_file_id, percentage, locator, updated_at)
         VALUES (?, 'koreader-doc', ?, ?, 0.42, '/koreader/xpointer', unixepoch() + 10)",
    )
    .bind(user_id)
    .bind(book_id)
    .bind(file_id)
    .execute(&app.state.db)
    .await
    .unwrap();
    let response = request(&app, "GET", "/api/books/continue", &cookie, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let items = json_body(response).await;
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["source"], "koreader");
    assert_eq!(items[0]["percentage"], 0.42);
    assert_eq!(items[0]["browserFileId"], file_id);
    assert_eq!(items[0]["browserPercentage"], 0.35);
    assert_eq!(items[0]["epubFileId"], file_id);

    let detail = json_body(
        request(
            &app,
            "GET",
            &format!("/api/books/{book_id}"),
            &cookie,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(detail["browserFileId"], file_id);

    // A changed file never advertises the old browser position, while the
    // independently synced KOReader percentage remains visible.
    sqlx::query("UPDATE book_files SET sha256 = 'sha-new' WHERE id = ?")
        .bind(file_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let items =
        json_body(request(&app, "GET", "/api/books/continue", &cookie, None, None).await).await;
    assert_eq!(items[0]["source"], "koreader");
    assert_eq!(items[0]["browserFileId"], Value::Null);
    assert_eq!(items[0]["browserPercentage"], Value::Null);
    assert_eq!(items[0]["epubFileId"], file_id);
}
