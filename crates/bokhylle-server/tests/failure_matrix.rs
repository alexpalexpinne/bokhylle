use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_acquisition::model::ReleaseCandidate;
use bokhylle_acquisition::provider::{DownloadProvider, IndexerProvider};
use bokhylle_acquisition::state::AcquisitionStatus;
use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_server::auth::Role;

mod common;

const MAGNET: &str = "magnet:?xt=urn:btih:cccccccccccccccccccccccccccccccccccccccc";

fn strong_candidate() -> ReleaseCandidate {
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

async fn app_with_book(
    indexer: Arc<dyn IndexerProvider>,
    downloader: Arc<dyn DownloadProvider>,
) -> (common::TestApp, tempfile::TempDir, String, i64) {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let test_app =
        common::test_app_with_providers(library_dir.path().to_path_buf(), indexer, downloader)
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

    (test_app, library_dir, cookie, book_id)
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

async fn create_acquisition(test_app: &common::TestApp, cookie: &str, book_id: i64) -> String {
    let (status, created) = post_json(
        test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = created["id"].as_str().unwrap().to_string();
    let _ = created["duplicate"];
    id
}

async fn wait_for_status(
    test_app: &common::TestApp,
    cookie: &str,
    acquisition_id: &str,
    wanted: &[&str],
) -> Value {
    let mut last = Value::Null;

    for _ in 0..400 {
        let (_, view) = get_json(
            test_app,
            &format!("/api/acquisitions/{acquisition_id}"),
            cookie,
        )
        .await;
        last = view.clone();

        if let Some(status) = view["status"].as_str()
            && wanted.contains(&status)
        {
            return view;
        }

        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    panic!("acquisition did not reach {wanted:?}; last state: {last}");
}

#[tokio::test]
async fn prowlarr_unavailable_fails_search_recoverably() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) =
        app_with_book(Arc::new(FakeIndexerProvider::failing()), downloader).await;

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["DOWNLOAD_FAILED"]).await;
    assert_eq!(view["errorCode"], "search_failed");
    assert_eq!(view["status"], "DOWNLOAD_FAILED");
}

#[tokio::test]
async fn zero_candidates_produce_no_release_found() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) =
        app_with_book(Arc::new(FakeIndexerProvider::default()), downloader).await;

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["NO_RELEASE_FOUND"]).await;
    assert_eq!(view["status"], "NO_RELEASE_FOUND");

    let mut notified = false;
    for _ in 0..100 {
        let (_, list) = get_json(&test_app, "/api/notifications", &cookie).await;
        if list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "failed")
        {
            notified = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(notified, "failure notification was not recorded");
}

#[tokio::test]
async fn qbittorrent_unavailable_when_queueing_fails_download() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    downloader.set_failing(true);

    let (test_app, _library_dir, cookie, book_id) = app_with_book(
        Arc::new(FakeIndexerProvider::with_candidates(vec![
            strong_candidate(),
        ])),
        downloader,
    )
    .await;

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["DOWNLOAD_FAILED"]).await;
    assert_eq!(view["errorCode"], "download_failed");
}

#[tokio::test]
async fn qbittorrent_outage_during_tracking_is_transient() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) = app_with_book(
        Arc::new(FakeIndexerProvider::with_candidates(vec![
            strong_candidate(),
        ])),
        downloader.clone(),
    )
    .await;

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;

    downloader.set_failing(true);
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

    downloader.set_failing(false);
    downloader.set_progress(0.5);
    let report = bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(report.downloading, 1);
}

#[tokio::test]
async fn stalled_torrent_keeps_downloading_state() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) = app_with_book(
        Arc::new(FakeIndexerProvider::with_candidates(vec![
            strong_candidate(),
        ])),
        downloader.clone(),
    )
    .await;

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;

    downloader.set_progress(0.35);
    downloader.set_state("stalledDL");
    let report = bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    assert_eq!(report.downloading, 1);
    assert_eq!(report.failed, 0);

    let (_, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(view["status"], "DOWNLOADING");
    assert_eq!(view["progress"], 35.0);
}

#[tokio::test]
async fn restart_during_download_persists_and_reconciles() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) = app_with_book(
        Arc::new(FakeIndexerProvider::with_candidates(vec![
            strong_candidate(),
        ])),
        downloader.clone(),
    )
    .await;

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;

    downloader.set_progress(0.5);
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    let database_path = test_app.state.paths.config_dir.join("bokhylle.db");
    let reopened = bokhylle_server::db::init(&database_path).await.unwrap();
    let recovered = bokhylle_server::acquisition::get(&reopened, &acquisition_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.status, "DOWNLOADING");
    assert_eq!(recovered.progress, 50.0);

    downloader.set_progress(1.0);
    downloader.set_state("uploading");
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    let (_, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    // the import pipeline runs concurrently after the tracker hand-off
    let status = view["status"].as_str().unwrap_or_default();
    assert!(
        ["DOWNLOADED", "READY", "IMPORT_FAILED"].contains(&status),
        "expected download hand-off, got {status}"
    );
}

#[tokio::test]
async fn restart_mid_search_resumes_and_queues() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) = app_with_book(
        Arc::new(FakeIndexerProvider::with_candidates(vec![
            strong_candidate(),
        ])),
        downloader.clone(),
    )
    .await;

    let (acquisition, _) = bokhylle_server::acquisition::create(
        &test_app.state.db,
        book_id,
        None,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();
    bokhylle_server::acquisition::transition(
        &test_app.state.db,
        &acquisition.id,
        AcquisitionStatus::Searching,
        None,
    )
    .await
    .unwrap();

    bokhylle_server::acquisition_pipeline::recover(&test_app.state).await;

    let view = wait_for_status(&test_app, &cookie, &acquisition.id, &["QUEUED"]).await;
    assert_eq!(view["status"], "QUEUED");
    assert_eq!(downloader.added().len(), 1);
}

#[tokio::test]
async fn user_cancellation_removes_owned_download() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) = app_with_book(
        Arc::new(FakeIndexerProvider::with_candidates(vec![
            strong_candidate(),
        ])),
        downloader.clone(),
    )
    .await;

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;

    let (status, cancelled) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/cancel"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "CANCELLED");
    assert_eq!(
        downloader.canceled(),
        vec!["cccccccccccccccccccccccccccccccccccccccc".to_string()]
    );

    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    let (_, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(view["status"], "CANCELLED");
}

#[tokio::test]
async fn duplicate_add_does_not_start_a_second_workflow() {
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) = app_with_book(
        Arc::new(FakeIndexerProvider::with_candidates(vec![
            strong_candidate(),
        ])),
        downloader.clone(),
    )
    .await;

    let first = create_acquisition(&test_app, &cookie, book_id).await;

    let (status, duplicate) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["id"], first);

    wait_for_status(&test_app, &cookie, &first, &["QUEUED"]).await;

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM acquisitions")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(downloader.added().len(), 1);
}
