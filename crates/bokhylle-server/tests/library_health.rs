mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

async fn get(
    app: &common::TestApp,
    uri: &str,
    cookie: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .router
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

async fn insert_book(
    app: &common::TestApp,
    title: &str,
    file_path: &str,
    cover_path: Option<&str>,
    description: Option<&str>,
    language: Option<&str>,
) -> i64 {
    let metadata = bokhylle_metadata::MetadataResult {
        provider: "openlibrary".to_string(),
        provider_key: format!("/works/{title}"),
        title: title.to_string(),
        authors: vec!["Health Author".to_string()],
        ..Default::default()
    };
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &metadata,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE books SET cover_path = ?, description = ?, language = ? WHERE id = ?")
        .bind(cover_path)
        .bind(description)
        .bind(language)
        .bind(book_id)
        .execute(&app.state.db)
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
    .bind(file_path)
    .bind(format!("sha-{book_id}"))
    .execute(&app.state.db)
    .await
    .unwrap();
    book_id
}

async fn titles(app: &common::TestApp, uri: &str, cookie: &str) -> Vec<String> {
    let (status, value) = get(app, uri, Some(cookie)).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {value}");
    let items = value["items"].as_array().unwrap().clone();
    items
        .into_iter()
        .map(|item| item["title"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn library_health_counts_gaps_and_the_library_filters_them() {
    let temp = tempfile::tempdir().unwrap();
    let present = temp.path().join("present.epub");
    std::fs::write(&present, vec![1u8; 2048]).unwrap();

    let app = common::test_app_with_library_root(temp.path().to_path_buf()).await;
    app.state
        .auth
        .create_user(
            "health-admin",
            "password123",
            bokhylle_server::auth::Role::Admin,
        )
        .await
        .unwrap();
    app.state
        .auth
        .create_user(
            "health-member",
            "password123",
            bokhylle_server::auth::Role::User,
        )
        .await
        .unwrap();

    let long_description = "A description that is comfortably longer than one hundred and twenty characters so the library counts it as a usable blurb for this book.".to_string();
    insert_book(
        &app,
        "Complete Book",
        present.to_str().unwrap(),
        Some("/tmp/cover.jpg"),
        Some(&long_description),
        Some("en"),
    )
    .await;
    insert_book(
        &app,
        "Gap Book",
        temp.path().join("gone.epub").to_str().unwrap(),
        None,
        Some("Too short."),
        None,
    )
    .await;

    let admin = common::login(&app, "health-admin", "password123").await;
    let member = common::login(&app, "health-member", "password123").await;

    let (status, _) = get(&app, "/api/admin/library-health", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = get(&app, "/api/admin/library-health", Some(&member)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, health) = get(&app, "/api/admin/library-health", Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["books"], 2);
    assert_eq!(health["files"], 2);
    assert_eq!(health["missingCovers"], 1);
    assert_eq!(health["missingDescriptions"], 1);
    assert_eq!(health["missingLanguages"], 1);
    assert_eq!(health["missingFiles"], 1);
    let samples = health["missingFileSamples"].as_array().unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0]["title"], "Gap Book");
    assert!(samples[0]["path"].as_str().unwrap().ends_with("gone.epub"));

    for missing in ["cover", "description", "language"] {
        let items = titles(&app, &format!("/api/books?missing={missing}"), &admin).await;
        assert_eq!(items, vec!["Gap Book".to_string()], "missing={missing}");
    }

    let items = titles(&app, "/api/books?missing=bogus", &admin).await;
    assert_eq!(items.len(), 2, "an unknown missing value is ignored");

    let (status, results) = get(&app, "/api/books/search?q=book&missing=cover", Some(&admin)).await;
    assert_eq!(status, StatusCode::OK);
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "Gap Book");
    assert!(
        titles(&app, "/api/books?missing=cover", &member)
            .await
            .len()
            == 1,
        "the gap view is ordinary library browsing, not admin-only"
    );
}
