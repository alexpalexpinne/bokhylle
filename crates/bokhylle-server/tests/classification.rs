use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bokhylle_server::auth::Role;
use serde_json::{Value, json};
use tower::ServiceExt;

mod common;

async fn request(
    app: &common::TestApp,
    method: &str,
    uri: &str,
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie);
    }
    let payload = if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .router
        .clone()
        .oneshot(builder.body(payload).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn add_book(app: &common::TestApp, title: &str, file_name: &str, format: &str) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO books (title, normalized_title) VALUES (?, ?) RETURNING id",
    )
    .bind(title)
    .bind(title.to_lowercase())
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let edition_id: i64 =
        sqlx::query_scalar("INSERT INTO editions (book_id, title) VALUES (?, ?) RETURNING id")
            .bind(id)
            .bind(title)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, ?, ?, 10, ?)",
    )
    .bind(edition_id)
    .bind(format!("/fictional/{id}-{file_name}"))
    .bind(format)
    .bind(format!("fictional-{id}"))
    .execute(&app.state.db)
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn review_is_admin_only_and_batch_is_atomic() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let admin = common::login(&app, "admin", "password123").await;
    let reader = common::login(&app, "reader", "password123").await;
    let one = add_book(&app, "Imaginary Saga Vol. 2", "saga-vol-2.cbz", "cbz").await;
    let two = add_book(&app, "Ordinary Novel", "novel.epub", "epub").await;
    let uri = "/api/admin/books/classification-review";

    assert_eq!(
        request(&app, "GET", uri, "", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "GET", uri, &reader, None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            uri,
            &reader,
            Some(json!({"decisions": [{"bookId": one, "action": "dismiss"}]}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    let (status, pending) = request(&app, "GET", uri, &admin, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending["total"], 2);
    let saga = pending["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == one)
        .unwrap();
    assert_eq!(saga["suggestion"]["publicationKind"], "comic");
    assert_eq!(saga["suggestion"]["seriesName"], "Imaginary Saga");
    assert_eq!(saga["suggestion"]["seriesNumber"], "2");

    let bad = json!({"decisions": [
        {"bookId": one, "action": "apply", "publicationKind": "comic", "newSeriesName": "Imaginary Saga", "seriesNumber": "2"},
        {"bookId": two, "action": "apply", "publicationKind": "manga", "seriesId": 999999}
    ]});
    assert_eq!(
        request(&app, "POST", uri, &admin, Some(bad)).await.0,
        StatusCode::BAD_REQUEST
    );
    let kind: String = sqlx::query_scalar("SELECT publication_kind FROM books WHERE id = ?")
        .bind(one)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(kind, "unknown");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM series WHERE name = 'Imaginary Saga'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(count, 0);

    let good = json!({"decisions": [
        {"bookId": one, "action": "apply", "publicationKind": "manga", "newSeriesName": "Imaginary Saga", "seriesNumber": "2", "seriesSortOrder": 2.5, "readingDirection": "rtl"},
        {"bookId": two, "action": "dismiss"}
    ]});
    assert_eq!(
        request(&app, "POST", uri, &admin, Some(good)).await.0,
        StatusCode::OK
    );
    let (_, pending) = request(&app, "GET", uri, &admin, None).await;
    assert_eq!(pending["total"], 0);
    let (_, all) = request(&app, "GET", &format!("{uri}?status=all"), &admin, None).await;
    assert_eq!(all["total"], 2);
    let saga = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == one)
        .unwrap();
    assert_eq!(saga["publicationKind"], "manga");
    assert_eq!(saga["seriesName"], "Imaginary Saga");
    assert_eq!(saga["seriesSortOrder"], 2.5);
    assert_eq!(saga["readingDirection"], "rtl");

    // Provider text may change without relinking the local series.
    sqlx::query("UPDATE books SET series = 'Wrong Provider Series' WHERE id = ?")
        .bind(one)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (_, all) = request(&app, "GET", &format!("{uri}?status=all"), &admin, None).await;
    let saga = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == one)
        .unwrap();
    assert_eq!(saga["seriesName"], "Imaginary Saga");
    assert_eq!(saga["publicationKind"], "manga");
    assert!(saga["reviewedAt"].is_number());
}

#[tokio::test]
async fn accepted_corrections_survive_a_library_rescan() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();
    let app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    app.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin = common::login(&app, "admin", "password123").await;
    let scan = bokhylle_server::library::index_library(&app.state)
        .await
        .unwrap();
    assert!(scan.indexed > 0);
    let book_id: i64 = sqlx::query_scalar("SELECT b.id FROM books b JOIN editions e ON e.book_id = b.id JOIN book_files f ON f.edition_id = e.id LIMIT 1")
        .fetch_one(&app.state.db).await.unwrap();
    let uri = "/api/admin/books/classification-review";
    assert_eq!(request(&app, "POST", uri, &admin, Some(json!({"decisions": [{
        "bookId": book_id, "action": "apply", "publicationKind": "manga",
        "newSeriesName": "Fictional Fixture Series", "seriesNumber": "1.5", "readingDirection": "rtl"
    }]}))).await.0, StatusCode::OK);
    bokhylle_server::library::index_library(&app.state)
        .await
        .unwrap();
    let row: (String, String, Option<String>, Option<f64>, i64, i64) = sqlx::query_as(
        "SELECT b.publication_kind, s.name, b.series_number, b.series_sort_order,
                b.series_link_locked, b.classification_reviewed_at
         FROM books b JOIN series s ON s.id = b.series_id WHERE b.id = ?",
    )
    .bind(book_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(row.0, "manga");
    assert_eq!(row.1, "Fictional Fixture Series");
    assert_eq!(row.2.as_deref(), Some("1.5"));
    assert_eq!(row.3, Some(1.5));
    assert_eq!(row.4, 1);
    assert!(row.5 > 0);

    // Clearing a reviewed volume is intentional; embedded metadata must not
    // repopulate it on the next scan.
    assert_eq!(
        request(
            &app,
            "POST",
            uri,
            &admin,
            Some(json!({"decisions": [{
                "bookId": book_id, "action": "apply", "publicationKind": "manga",
                "seriesId": sqlx::query_scalar::<_, i64>("SELECT series_id FROM books WHERE id = ?")
                    .bind(book_id).fetch_one(&app.state.db).await.unwrap(),
                "seriesNumber": null
            }]})),
        )
        .await
        .0,
        StatusCode::OK
    );
    bokhylle_server::library::index_library(&app.state)
        .await
        .unwrap();
    let volume: Option<String> = sqlx::query_scalar("SELECT series_number FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert!(volume.is_none());
}

#[tokio::test]
async fn four_hundred_mixed_files_can_be_filtered_and_reviewed_together() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    let admin = common::login(&app, "admin", "password123").await;
    let mut book_ids = Vec::new();
    for index in 1..=320 {
        book_ids.push(
            add_book(
                &app,
                &format!("Fictional Novel {index}"),
                &format!("novel-{index}.epub"),
                "epub",
            )
            .await,
        );
    }
    let mut comic_ids = Vec::new();
    for index in 1..=80 {
        comic_ids.push(
            add_book(
                &app,
                &format!("Imaginary Comic Vol. {index}"),
                &format!("comic-{index}.cbz"),
                "cbz",
            )
            .await,
        );
    }
    let uri = "/api/admin/books/classification-review";
    let (status, first) = request(&app, "GET", uri, &admin, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["total"], 400);
    assert_eq!(first["items"].as_array().unwrap().len(), 25);
    assert_eq!(first["counts"]["book"], 320);
    assert_eq!(first["counts"]["comic"], 80);
    assert_eq!(first["counts"]["simple"], 320);
    assert_eq!(first["counts"]["needsReview"], 80);

    let (status, books) = request(
        &app,
        "GET",
        &format!("{uri}?kind=book&attention=simple&pageSize=500"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(books["total"], 320);
    assert_eq!(books["items"].as_array().unwrap().len(), 320);
    let (status, comics) = request(
        &app,
        "GET",
        &format!("{uri}?kind=comic&attention=review&pageSize=500"),
        &admin,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(comics["total"], 80);
    assert_eq!(comics["items"].as_array().unwrap().len(), 80);

    let decisions: Vec<Value> = book_ids
        .iter()
        .map(|id| json!({"bookId": id, "action": "apply", "publicationKind": "book"}))
        .collect();
    assert_eq!(
        request(
            &app,
            "POST",
            uri,
            &admin,
            Some(json!({"decisions": decisions}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, pending) = request(&app, "GET", uri, &admin, None).await;
    assert_eq!(pending["total"], 80);
    assert_eq!(pending["counts"]["book"], 0);
    assert_eq!(pending["counts"]["comic"], 80);

    // A preview made before another review must fail as one transaction.
    let stale = json!({"decisions": [
        {"bookId": comic_ids[0], "action": "apply", "publicationKind": "comic", "onlyIfPending": true},
        {"bookId": book_ids[0], "action": "apply", "publicationKind": "manga", "onlyIfPending": true}
    ]});
    assert_eq!(
        request(&app, "POST", uri, &admin, Some(stale)).await.0,
        StatusCode::CONFLICT
    );
    let comic_kind: String = sqlx::query_scalar("SELECT publication_kind FROM books WHERE id = ?")
        .bind(comic_ids[0])
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(comic_kind, "unknown");
}
