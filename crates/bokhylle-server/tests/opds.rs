mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bokhylle_metadata::MetadataResult;
use tower::ServiceExt;

fn base64(input: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[tokio::test]
async fn opds_feed_advertises_the_real_file_type_and_challenges_anonymous_requests() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let (_, token) = bokhylle_server::reader_tokens::create(&app.state.db, user.id, "test")
        .await
        .unwrap();

    let metadata = MetadataResult {
        provider: "openlibrary".to_string(),
        provider_key: "/works/OLPDFW".to_string(),
        title: "PDF Only".to_string(),
        authors: vec!["Some Author".to_string()],
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
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, '/tmp/opds.pdf', 'pdf', 10, 'opds-pdf')",
    )
    .bind(edition_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let auth = format!("Basic {}", base64(&format!("any:{token}")));
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/opds/all")
                .header(header::AUTHORIZATION, auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        body.contains("type=\"application/pdf\""),
        "feed must advertise the real file type"
    );
    assert!(
        !body.contains("1970-01-01"),
        "feed entries must carry real timestamps"
    );

    // The Basic scheme is case-insensitive.
    let lowercase = format!("basic {}", base64(&format!("any:{token}")));
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/opds/all")
                .header(header::AUTHORIZATION, lowercase)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let anonymous = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/opds/all")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert!(anonymous.headers().contains_key(header::WWW_AUTHENTICATE));
}

async fn get(app: &common::TestApp, uri: &str, token: &str) -> (StatusCode, String) {
    let auth = format!("Basic {}", base64(&format!("any:{token}")));
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::AUTHORIZATION, auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    (status, body)
}

async fn add_book(app: &common::TestApp, title: &str, author: &str, path: &str) -> (i64, i64) {
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
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, 'epub', 10, ?)",
    )
    .bind(edition_id)
    .bind(path)
    .bind(format!("sha-{book_id}"))
    .execute(&app.state.db)
    .await
    .unwrap();
    let author_id: i64 = sqlx::query_scalar("SELECT author_id FROM book_authors WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    (book_id, author_id)
}

#[tokio::test]
async fn opds_root_advertises_recent_and_author_feeds() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user(
            "opds-user",
            "password123",
            bokhylle_server::auth::Role::User,
        )
        .await
        .unwrap();
    let (_, token) = bokhylle_server::reader_tokens::create(&app.state.db, user.id, "test")
        .await
        .unwrap();
    let (_, author_id) = add_book(
        &app,
        "Author Book",
        "Feeder Author",
        "/tmp/opds-author.epub",
    )
    .await;

    let (status, root) = get(&app, "/opds", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(root.contains("/opds/recent"));
    assert!(root.contains("/opds/authors"));

    let (status, recent) = get(&app, "/opds/recent", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(recent.contains("Author Book"));

    let (status, authors) = get(&app, "/opds/authors", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(authors.contains("Feeder Author"));
    assert!(authors.contains(&format!("/opds/authors/{author_id}")));

    let (status, feed) = get(&app, &format!("/opds/authors/{author_id}"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(feed.contains("Author Book"));

    let (status, _) = get(&app, "/opds/authors/99999", &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn opds_author_feed_for_a_child_lists_only_their_shelf() {
    let app = common::test_app().await;
    let child = app
        .state
        .auth
        .create_user_with_profile(
            "opds-kid",
            "password123",
            bokhylle_server::auth::Role::User,
            "password",
            "child",
        )
        .await
        .unwrap();
    let (_, token) = bokhylle_server::reader_tokens::create(&app.state.db, child.id, "test")
        .await
        .unwrap();
    let (book_id, author_id) = add_book(&app, "Kid Book", "Kid Feeder", "/tmp/opds-kid.epub").await;

    let (status, authors) = get(&app, "/opds/authors", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !authors.contains("Kid Feeder"),
        "an off-shelf author must not be listed"
    );

    let (status, _) = get(&app, &format!("/opds/authors/{author_id}"), &token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    bokhylle_server::user_books::add(&app.state.db, child.id, book_id, "request")
        .await
        .unwrap();

    let (status, authors) = get(&app, "/opds/authors", &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(authors.contains("Kid Feeder"));

    let (status, feed) = get(&app, &format!("/opds/authors/{author_id}"), &token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(feed.contains("Kid Book"));
}
