use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bokhylle_server::auth::Role;
use serde_json::{Value, json};
use tower::ServiceExt;

mod common;

async fn get(app: &common::TestApp, cookie: &str, uri: &str) -> (StatusCode, Value) {
    let response = app
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
    (status, serde_json::from_slice(&body).unwrap())
}

async fn put(app: &common::TestApp, cookie: &str, uri: &str, body: Value) -> StatusCode {
    app.router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(uri)
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

async fn add_book(
    app: &common::TestApp,
    title: &str,
    series: Option<&str>,
    number: Option<&str>,
    kind: &str,
) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO books (title, normalized_title, series, series_number, publication_kind)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(title)
    .bind(title.to_lowercase())
    .bind(series)
    .bind(number)
    .bind(kind)
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
    sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, ?, 'cbz', 10, ?)")
        .bind(edition_id).bind(format!("/fictional/{id}.cbz"))
        .bind(format!("fictional-sha-{id}"))
        .execute(&app.state.db).await.unwrap();
    id
}

#[tokio::test]
async fn series_progress_requires_explicit_completion_and_a_consecutive_volume() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    app.state
        .auth
        .create_user("other", "password123", Role::User)
        .await
        .unwrap();
    let reader = common::login(&app, "reader", "password123").await;
    let other = common::login(&app, "other", "password123").await;
    let reader_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'reader'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let one = add_book(&app, "Moonward 1", Some("Moonward"), Some("1"), "comic").await;
    let two = add_book(&app, "Moonward 2", Some("Moonward"), Some("2"), "comic").await;
    let four = add_book(&app, "Moonward 4", Some("Moonward"), Some("4"), "comic").await;
    let series_id: i64 = sqlx::query_scalar("SELECT series_id FROM books WHERE id = ?")
        .bind(one)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let uri = format!("/api/series/{series_id}?mine=false");
    let file_id: i64 = sqlx::query_scalar(
        "SELECT f.id FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = ?",
    )
    .bind(one)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO browser_reading_positions
         (user_id, book_file_id, sha256, format, locator, percentage, completed, revision)
         VALUES (?, ?, ?, 'cbz', '2', 0.2, 0, 1)",
    )
    .bind(reader_id)
    .bind(file_id)
    .bind(format!("fictional-sha-{one}"))
    .execute(&app.state.db)
    .await
    .unwrap();
    let (_, started) = get(&app, &reader, &uri).await;
    assert_eq!(started["reading"]["current"]["bookId"], one);
    assert!(started["reading"]["nextBookId"].is_null());
    let (_, before_finish) = get(&app, &reader, "/api/books/continue").await;
    assert_eq!(before_finish.as_array().unwrap().len(), 1);
    assert_eq!(
        put(
            &app,
            &reader,
            &format!("/api/books/{one}/completion"),
            json!({"completed": true})
        )
        .await,
        StatusCode::OK
    );
    let (_, finished_one) = get(&app, &reader, &uri).await;
    assert_eq!(finished_one["reading"]["nextBookId"], two);
    assert_eq!(finished_one["reading"]["finishedBookIds"], json!([one]));
    assert!(finished_one["reading"]["current"].is_null());
    let (_, after_finish) = get(&app, &reader, "/api/books/continue").await;
    assert!(after_finish.as_array().unwrap().is_empty());
    let (_, other_progress) = get(&app, &other, &uri).await;
    assert_eq!(other_progress["reading"]["finishedBookIds"], json!([]));
    assert!(other_progress["reading"]["nextBookId"].is_null());

    // Replacing a file does not erase the book-level finish decision.
    sqlx::query("UPDATE book_files SET sha256 = 'replacement' WHERE id = ?")
        .bind(file_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (_, completion) = get(&app, &reader, &format!("/api/books/{one}/completion")).await;
    assert_eq!(completion["completed"], true);

    assert_eq!(
        put(
            &app,
            &reader,
            &format!("/api/books/{two}/completion"),
            json!({"completed": true})
        )
        .await,
        StatusCode::OK
    );
    let (_, gap) = get(&app, &reader, &uri).await;
    assert_eq!(gap["reading"]["missingNextVolume"], 3);
    assert!(gap["reading"]["nextBookId"].is_null());
    let three = add_book(&app, "Moonward 3", Some("Moonward"), Some("3"), "comic").await;
    let (_, filled) = get(&app, &reader, &uri).await;
    assert_eq!(filled["reading"]["nextBookId"], three);
    add_book(
        &app,
        "Moonward 3 alternate",
        Some("Moonward"),
        Some("3"),
        "comic",
    )
    .await;
    let (_, ambiguous) = get(&app, &reader, &uri).await;
    assert!(ambiguous["reading"]["nextBookId"].is_null());
    assert!(ambiguous["reading"]["missingNextVolume"].is_null());
    assert_eq!(
        put(
            &app,
            &reader,
            &format!("/api/books/{two}/completion"),
            json!({"completed": false})
        )
        .await,
        StatusCode::OK
    );
    let (_, reverted) = get(&app, &reader, &uri).await;
    assert_eq!(reverted["reading"]["nextBookId"], two);
    assert_eq!(reverted["reading"]["finishedBookIds"], json!([one]));
    let special = add_book(&app, "Moonward 1.5", Some("Moonward"), Some("1.5"), "comic").await;
    assert_eq!(
        put(
            &app,
            &reader,
            &format!("/api/books/{special}/completion"),
            json!({"completed": true})
        )
        .await,
        StatusCode::OK
    );
    let (_, after_special) = get(&app, &reader, &uri).await;
    assert_eq!(after_special["reading"]["nextBookId"], two);
    assert_ne!(four, three);
}

#[tokio::test]
async fn comic_series_are_numeric_ordered_and_child_shelf_scoped() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("parent", "password123", Role::Admin)
        .await
        .unwrap();
    app.state
        .auth
        .create_user("child", "password123", Role::User)
        .await
        .unwrap();
    let parent = common::login(&app, "parent", "password123").await;
    let child = common::login(&app, "child", "password123").await;
    let child_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'child'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(child_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    let ten = add_book(
        &app,
        "Imaginary Saga 10",
        Some("Imaginary Saga"),
        Some("10"),
        "manga",
    )
    .await;
    let two = add_book(
        &app,
        "Imaginary Saga 2",
        Some("Imaginary Saga"),
        Some("2"),
        "manga",
    )
    .await;
    let standalone = add_book(&app, "Standalone Comic", None, None, "comic").await;
    let _novel = add_book(&app, "Imaginary Novel", None, None, "book").await;
    let series_id: i64 = sqlx::query_scalar("SELECT series_id FROM books WHERE id = ?")
        .bind(two)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let other_series_id: i64 = sqlx::query_scalar("SELECT series_id FROM books WHERE id = ?")
        .bind(ten)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(series_id, other_series_id);

    let (status, household) = get(&app, &parent, "/api/library/comics?mine=false").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(household["total"], 2);
    assert!(
        household["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tile| tile["type"] == "series" && tile["value"]["volumeCount"] == 2)
    );
    assert!(
        household["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tile| tile["type"] == "book" && tile["value"]["id"] == standalone)
    );

    let (status, detail) = get(
        &app,
        &parent,
        &format!("/api/series/{series_id}?mine=false"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["volumes"][0]["id"], two);
    assert_eq!(detail["volumes"][1]["id"], ten);

    bokhylle_server::user_books::add(&app.state.db, child_id, two, "parent_assigned")
        .await
        .unwrap();
    let (status, child_shelf) = get(&app, &child, "/api/library/comics?mine=false").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(child_shelf["total"], 1);
    assert_eq!(child_shelf["items"][0]["value"]["volumeCount"], 1);
    let (status, child_detail) =
        get(&app, &child, &format!("/api/series/{series_id}?mine=false")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(child_detail["volumes"].as_array().unwrap().len(), 1);
    assert_eq!(child_detail["volumes"][0]["id"], two);
    assert_eq!(
        put(
            &app,
            &child,
            &format!("/api/books/{ten}/completion"),
            json!({"completed": true})
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        put(
            &app,
            &child,
            &format!("/api/books/{two}/completion"),
            json!({"completed": true})
        )
        .await,
        StatusCode::OK
    );
    let (_, child_finished) = get(&app, &child, &format!("/api/series/{series_id}")).await;
    assert_eq!(child_finished["reading"]["finishedBookIds"], json!([two]));

    assert_eq!(
        put(
            &app,
            &parent,
            &format!("/api/admin/series/{series_id}"),
            json!({"name": "Imaginary Journey"})
        )
        .await,
        StatusCode::OK
    );
    let imported: String = sqlx::query_scalar("SELECT series FROM books WHERE id = ?")
        .bind(two)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(imported, "Imaginary Saga");
    let (_, renamed) = get(&app, &parent, &format!("/api/books/{two}")).await;
    assert_eq!(renamed["series"], "Imaginary Journey");
    assert_eq!(renamed["legacySeriesText"], "Imaginary Saga");
    let (status, searched) = get(
        &app,
        &parent,
        "/api/books/search?q=Imaginary%20Journey&mine=false",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        searched
            .as_array()
            .unwrap()
            .iter()
            .any(|book| book["id"] == two)
    );
    let (_, facets) = get(&app, &parent, "/api/books/facets?scope=household").await;
    assert!(
        facets["series"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["value"] == "Imaginary Journey")
    );
    let next = add_book(
        &app,
        "Imaginary Journey 11",
        Some("Imaginary Journey"),
        Some("11"),
        "manga",
    )
    .await;
    let next_series_id: i64 = sqlx::query_scalar("SELECT series_id FROM books WHERE id = ?")
        .bind(next)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(next_series_id, series_id);

    assert_eq!(
        put(
            &app,
            &parent,
            &format!("/api/admin/books/{two}"),
            json!({"seriesId": null, "publicationKind": "comic"})
        )
        .await,
        StatusCode::OK
    );
    let (_, after_unlink) = get(&app, &parent, &format!("/api/books/{two}")).await;
    assert_eq!(after_unlink["seriesId"], Value::Null);
    assert_eq!(after_unlink["series"], Value::Null);
    assert_eq!(after_unlink["legacySeriesText"], "Imaginary Saga");
    let (_, shelf_after_unlink) = get(&app, &parent, "/api/library/comics?mine=false").await;
    assert!(
        shelf_after_unlink["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tile| tile["type"] == "book" && tile["value"]["id"] == two)
    );
    sqlx::query("UPDATE books SET series = 'Imaginary Journey' WHERE id = ?")
        .bind(two)
        .execute(&app.state.db)
        .await
        .unwrap();
    let locked_series_id: Option<i64> =
        sqlx::query_scalar("SELECT series_id FROM books WHERE id = ?")
            .bind(two)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(locked_series_id, None);
}
