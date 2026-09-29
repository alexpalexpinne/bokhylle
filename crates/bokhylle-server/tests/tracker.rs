use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;

use bokhylle_acquisition::model::ReleaseCandidate;
use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_server::auth::Role;

mod common;

const MAGNET: &str = "magnet:?xt=urn:btih:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn candidate() -> ReleaseCandidate {
    ReleaseCandidate {
        source: None,
        method: None,
        id: "release-1".to_string(),
        title: "Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB".to_string(),
        indexer: Some("fake-indexer".to_string()),
        size_bytes: 3_000_000,
        seeders: Some(12),
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

async fn queued_acquisition(
    downloader: Arc<FakeDownloadProvider>,
) -> (common::TestApp, tempfile::TempDir, String, String) {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let test_app = common::test_app_with_providers(
        library_dir.path().to_path_buf(),
        Arc::new(FakeIndexerProvider::with_candidates(vec![candidate()])),
        downloader.clone(),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let status = test_app
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
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::ACCEPTED);
    for _ in 0..200 {
        let (_, status) = get_json(&test_app, "/api/library/scan/status", &cookie).await;
        if status["running"] == false && status["summary"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let (_, books) = get_json(&test_app, "/api/books?pageSize=50", &cookie).await;
    let book_id = books["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["title"] == "Project Hail Mary")
        .expect("fixture book")["id"]
        .as_i64()
        .unwrap();

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/books/{book_id}/acquisitions"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, &cookie)
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let mut acquisition_id = String::new();
    for _ in 0..400 {
        let (_, list) = get_json(&test_app, "/api/acquisitions", &cookie).await;
        if let Some(first) = list.as_array().and_then(|list| list.first())
            && first["status"] == "QUEUED"
        {
            acquisition_id = first["id"].as_str().unwrap().to_string();
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(
        !acquisition_id.is_empty(),
        "acquisition did not reach QUEUED"
    );

    (test_app, library_dir, cookie, acquisition_id)
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

#[tokio::test]
async fn tick_tracks_progress_and_completes_downloads() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, acquisition_id) =
        queued_acquisition(downloader.clone()).await;

    downloader.set_progress(0.42);
    let report = bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(report.checked, 1);
    assert_eq!(report.downloading, 1);

    let (_, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(view["status"], "DOWNLOADING");
    assert_eq!(view["progress"], 42.0);
    assert_eq!(view["downloadSpeed"], 1024);

    downloader.set_progress(1.0);
    downloader.set_state("uploading");
    let report = bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(report.completed, 1);

    let (_, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    // the tracker hands off to the import pipeline, which runs concurrently;
    // any of these states proves the download completed.
    let status = view["status"].as_str().unwrap_or_default();
    assert!(
        ["DOWNLOADED", "READY", "IMPORT_FAILED"].contains(&status),
        "tracker must hand off to import, got {status}"
    );
    assert_eq!(view["progress"], 100.0);
}

#[tokio::test]
async fn tick_reports_failure_when_download_disappears() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, acquisition_id) =
        queued_acquisition(downloader.clone()).await;

    downloader.set_missing(true);
    sqlx::query("UPDATE acquisitions SET created_at = 0 WHERE id = ?")
        .bind(&acquisition_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    let report = bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(report.failed, 1);

    let (_, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(view["status"], "DOWNLOAD_FAILED");
    assert_eq!(view["errorCode"], "download_missing");
}

#[tokio::test]
async fn tick_gives_fresh_queued_downloads_a_grace_period() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, acquisition_id) =
        queued_acquisition(downloader.clone()).await;

    downloader.set_missing(true);
    let report = bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(report.checked, 1);
    assert_eq!(report.failed, 0);

    let (_, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(view["status"], "QUEUED");
}

#[tokio::test]
async fn tick_adopts_downloads_missing_a_provider_id() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, _cookie, acquisition_id) =
        queued_acquisition(downloader.clone()).await;

    sqlx::query("UPDATE acquisitions SET provider_download_id = NULL WHERE id = ?")
        .bind(&acquisition_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    let stored = bokhylle_server::acquisition::get(&test_app.state.db, &acquisition_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.provider_download_id.as_deref(), Some("fakehash"));
}

#[tokio::test]
async fn cancelled_downloads_with_pending_cleanup_are_reconciled() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, _cookie, acquisition_id) =
        queued_acquisition(downloader.clone()).await;

    sqlx::query(
        "UPDATE acquisitions
         SET status = 'CANCELLED', cancel_pending = 1, provider_download_id = ?,
             download_provider = 'fake-downloader'
         WHERE id = ?",
    )
    .bind("cleanup-hash")
    .bind(&acquisition_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    // While the client is unreachable the flag survives; once it is back the
    // tracker removes the download and clears the flag.
    downloader.set_failing(true);
    let report = bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(report.cleaned_up, 0);
    let pending: i64 = sqlx::query_scalar("SELECT cancel_pending FROM acquisitions WHERE id = ?")
        .bind(&acquisition_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(pending, 1, "an unreachable client keeps the flag");

    downloader.set_failing(false);
    let report = bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(report.cleaned_up, 1);
    assert!(
        downloader.canceled().contains(&"cleanup-hash".to_string()),
        "the download is removed from the client"
    );
    let pending: i64 = sqlx::query_scalar("SELECT cancel_pending FROM acquisitions WHERE id = ?")
        .bind(&acquisition_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(pending, 0, "the cleanup is recorded as done");
}
