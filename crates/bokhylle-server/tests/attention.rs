use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;

use bokhylle_metadata::MetadataResult;
use bokhylle_server::auth::Role;

mod common;

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

async fn seed_book(test_app: &common::TestApp, key: &str, title: &str) -> i64 {
    bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: key.to_string(),
            title: title.to_string(),
            authors: vec!["Attention Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn attention_lists_only_human_decisions() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let admin_cookie = common::login(&test_app, "admin", "password123").await;
    let bob_cookie = common::login(&test_app, "bob", "password123").await;

    let review_book = seed_book(&test_app, "/works/OLATT1W", "Needs Review Book").await;
    let failed_book = seed_book(&test_app, "/works/OLATT2W", "Failed Import Book").await;
    let ready_book = seed_book(&test_app, "/works/OLATT3W", "Ready Book").await;
    let downloading_book = seed_book(&test_app, "/works/OLATT4W", "Downloading Book").await;
    let path_book = seed_book(&test_app, "/works/OLATT5W", "Missing Path Book").await;
    let retrying_book = seed_book(&test_app, "/works/OLATT6W", "Retrying Book").await;

    for (id, book_id, status, error_code, retry_stopped, next_retry_at) in [
        ("att-review", review_book, "NEEDS_REVIEW", None, 0, None),
        (
            "att-failed",
            failed_book,
            "IMPORT_FAILED",
            Some("corrupt_archive"),
            0,
            None,
        ),
        ("att-ready", ready_book, "READY", None, 0, None),
        ("att-down", downloading_book, "DOWNLOADING", None, 0, None),
        (
            "att-path",
            path_book,
            "IMPORT_FAILED",
            Some("content_missing"),
            1,
            None,
        ),
        (
            "att-retrying",
            retrying_book,
            "IMPORT_FAILED",
            Some("content_missing"),
            0,
            Some(1_900_000_000_i64),
        ),
        ("att-cancel", path_book, "CANCELLED", None, 0, None),
    ] {
        let cancel_pending = i64::from(id == "att-cancel");
        sqlx::query(
            "INSERT INTO acquisitions
                (id, book_id, status, error_code, retry_stopped, next_retry_at, cancel_pending)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(book_id)
        .bind(status)
        .bind(error_code)
        .bind(retry_stopped)
        .bind(next_retry_at)
        .bind(cancel_pending)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    }

    let (status, body) = get_json(&test_app, "/api/admin/attention", &admin_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["count"], 4,
        "only known human actions belong here: {body}"
    );
    let items = body["items"].as_array().unwrap();
    let kinds: Vec<&str> = items
        .iter()
        .map(|item| item["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"review"));
    assert!(kinds.contains(&"failed_import"));
    assert!(kinds.contains(&"path"));
    assert!(kinds.contains(&"cancel_failed"));
    assert!(
        items.iter().all(|item| item["title"] != "Downloading Book"
            && item["title"] != "Ready Book"
            && item["title"] != "Retrying Book"),
        "progress, finished and auto-retrying items stay out: {body}"
    );
    assert_eq!(
        items
            .iter()
            .find(|item| item["kind"] == "failed_import")
            .unwrap()["errorCode"],
        "corrupt_archive"
    );
    assert_eq!(
        items.iter().find(|item| item["kind"] == "path").unwrap()["errorCode"],
        "content_missing"
    );

    let (status, _) = get_json(&test_app, "/api/admin/attention", &bob_cookie).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
