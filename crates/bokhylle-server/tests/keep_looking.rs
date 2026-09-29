use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_acquisition::model::ReleaseCandidate;
use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_metadata::MetadataResult;
use bokhylle_server::auth::Role;

mod common;

const MAGNET: &str = "magnet:?xt=urn:btih:abcdef0123456789abcdef0123456789abcdef01";

fn release(title: &str, size: i64, seeders: i64) -> ReleaseCandidate {
    ReleaseCandidate {
        source: None,
        method: None,
        id: title.to_string(),
        title: title.to_string(),
        indexer: Some("fake-indexer".to_string()),
        size_bytes: size,
        seeders: Some(seeders),
        leechers: Some(0),
        download_url: Some("http://indexer.test/download/1".to_string()),
        magnet_url: Some(MAGNET.to_string()),
        info_url: None,
        detected_title: None,
        detected_author: None,
        detected_format: None,
        detected_language: None,
        detected_volume: None,
        is_collection: false,
        is_audiobook: false,
        is_comic: false,
    }
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

async fn failing_acquisition(
    indexer: Arc<FakeIndexerProvider>,
) -> (common::TestApp, String, String) {
    let test_app = common::test_app_with_providers(
        tempfile::tempdir().unwrap().path().to_path_buf(),
        indexer,
        Arc::new(FakeDownloadProvider::default()),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLKEEPW".to_string(),
            title: "Keep Looking Book".to_string(),
            authors: vec!["Keep Author".to_string()],
            language: Some("en".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (status, created) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let acquisition_id = created["id"].as_str().unwrap().to_string();

    // Wait for the failed state the pipeline reaches with a failing indexer,
    // including the retry the failure scheduler attaches to it.
    for _ in 0..200 {
        let (_, view) = get_json(
            &test_app,
            &format!("/api/acquisitions/{acquisition_id}"),
            &cookie,
        )
        .await;
        if view["status"] == "DOWNLOAD_FAILED" && view["keepLooking"] == true {
            return (test_app, cookie, acquisition_id);
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("acquisition never failed");
}

#[tokio::test]
async fn a_transient_failure_schedules_and_retries() {
    let indexer = Arc::new(FakeIndexerProvider::failing());
    let (test_app, cookie, acquisition_id) = failing_acquisition(indexer.clone()).await;

    let (status, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["status"], "DOWNLOAD_FAILED");
    assert_eq!(
        view["keepLooking"], true,
        "a transient failure keeps the intent alive: {view}"
    );
    assert!(view["nextRetryAt"].as_i64().is_some());

    // Make the retry due and let the indexer answer this time.
    sqlx::query("UPDATE acquisitions SET next_retry_at = unixepoch() - 1 WHERE id = ?")
        .bind(&acquisition_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    indexer.set_candidates(vec![release(
        "Keep.Looking.Book.Retail.EN.EPUB",
        3_000_000,
        12,
    )]);

    let retried = bokhylle_server::keep_looking::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(retried, 1, "the due retry runs the pipeline again");

    let mut queued = None;
    for _ in 0..200 {
        let (_, view) = get_json(
            &test_app,
            &format!("/api/acquisitions/{acquisition_id}"),
            &cookie,
        )
        .await;
        if view["status"] != "DOWNLOAD_FAILED" && view["status"] != "REQUESTED" {
            queued = Some(view);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let view = queued.expect("the retry must leave the failed state");
    assert_eq!(view["retryAttempts"], 1);
    assert_eq!(
        view["keepLooking"], false,
        "no retry is scheduled while the book is progressing: {view}"
    );
}

#[tokio::test]
async fn stopping_and_intrinsic_failures_do_not_retry() {
    let indexer = Arc::new(FakeIndexerProvider::failing());
    let (test_app, cookie, acquisition_id) = failing_acquisition(indexer.clone()).await;

    let (status, view) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/keep-looking"),
        &cookie,
        json!({ "enabled": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["keepLooking"], false);
    assert!(view["nextRetryAt"].is_null());

    let retried = bokhylle_server::keep_looking::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(retried, 0, "a stopped request is not retried");

    // Re-enabling schedules an immediate attempt for a retryable failure.
    let (status, view) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/keep-looking"),
        &cookie,
        json!({ "enabled": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["keepLooking"], true);
    assert!(view["nextRetryAt"].as_i64().is_some());

    // Intrinsic import failures are not retried automatically.
    sqlx::query(
        "UPDATE acquisitions
         SET status = 'IMPORT_FAILED', error_code = 'corrupt_archive',
             next_retry_at = NULL, retry_stopped = 0
         WHERE id = ?",
    )
    .bind(&acquisition_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();
    bokhylle_server::keep_looking::schedule_after_failure(&test_app.state.db, &acquisition_id)
        .await
        .unwrap();
    let next: Option<i64> =
        sqlx::query_scalar("SELECT next_retry_at FROM acquisitions WHERE id = ?")
            .bind(&acquisition_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert!(next.is_none(), "corrupt archives must not loop");
}
