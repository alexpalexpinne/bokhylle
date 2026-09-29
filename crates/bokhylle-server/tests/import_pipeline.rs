use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_acquisition::model::ReleaseCandidate;
use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;

mod common;

const MAGNET: &str = "magnet:?xt=urn:btih:dddddddddddddddddddddddddddddddddddddddd";

fn hail_mary() -> MetadataResult {
    MetadataResult {
        provider: "fake".to_string(),
        provider_key: "/works/OL1W".to_string(),
        title: "Project Hail Mary".to_string(),
        authors: vec!["Andy Weir".to_string()],
        year: Some(2021),
        language: Some("en".to_string()),
        isbn13: Some("9780593135204".to_string()),
        subjects: vec!["Science fiction".to_string(), "Space opera".to_string()],
        ..Default::default()
    }
}

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

struct Fixtures {
    hail_mary: std::path::PathBuf,
    other: std::path::PathBuf,
}

fn fixture_books() -> (tempfile::TempDir, Fixtures) {
    let dir = tempfile::tempdir().unwrap();
    let written = bokhylle_library::fixtures::generate_library(dir.path(), 2).unwrap();

    let hail_mary = written
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains("Project Hail Mary"))
        })
        .unwrap()
        .clone();
    let other = written
        .iter()
        .find(|path| path != &&hail_mary && path.extension().is_some_and(|ext| ext == "epub"))
        .unwrap()
        .clone();

    (dir, Fixtures { hail_mary, other })
}

async fn import_app(
    content: &std::path::Path,
) -> (
    common::TestApp,
    tempfile::TempDir,
    tempfile::TempDir,
    String,
    Arc<FakeDownloadProvider>,
) {
    let library_dir = tempfile::tempdir().unwrap();

    let downloader = Arc::new(FakeDownloadProvider::default());
    downloader.set_content_path(content);
    downloader.set_progress(1.0);
    downloader.set_state("uploading");

    // Imports are only trusted from the downloads directory, so the fixture
    // directory is the app's downloads root.
    let test_app = common::test_app_full_with_downloads(
        library_dir.path().to_path_buf(),
        content.to_path_buf(),
        Arc::new(FakeMetadataProvider::new(vec![hail_mary()])),
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

    let content_dir = tempfile::tempdir().unwrap();
    let _ = content_dir;

    (test_app, library_dir, content_dir, cookie, downloader)
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

async fn start_acquisition(test_app: &common::TestApp, cookie: &str) -> String {
    let (status, created) = post_json(
        test_app,
        "/api/discover/acquisitions",
        cookie,
        json!({ "provider": "fake", "providerKey": "/works/OL1W" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    created["id"].as_str().unwrap().to_string()
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
async fn delivery_intent_fires_when_the_import_is_ready() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("content.epub");
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (test_app, _library_dir, _keep, cookie, _downloader) = import_app(content_dir.path()).await;
    let (status, created) = post_json(
        &test_app,
        "/api/discover/acquisitions",
        &cookie,
        json!({ "provider": "fake", "providerKey": "/works/OL1W", "sendToReader": true }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let acquisition_id = created["id"].as_str().unwrap().to_string();

    let queued = wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    assert_eq!(
        queued["deliverOnReady"], true,
        "intent must be stored: {queued}"
    );
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    let view = wait_for_status(
        &test_app,
        &cookie,
        &acquisition_id,
        &["READY", "NEEDS_REVIEW", "DOWNLOAD_FAILED"],
    )
    .await;
    assert_eq!(view["status"], "READY", "view: {view}");

    // No SMTP or reader is configured, so delivery fails before a record is
    // created; the intent must survive so a retry can still send.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM acquisition_requests
         WHERE acquisition_id = ? AND deliver_on_ready = 1",
    )
    .bind(&acquisition_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(
        pending, 1,
        "a pre-record delivery failure must preserve the intent"
    );

    // the failure is still visible as an event, so poll for it.
    let mut delivered = false;
    for _ in 0..500 {
        let events = bokhylle_server::acquisition::events(&test_app.state.db, &acquisition_id)
            .await
            .unwrap();
        if events
            .iter()
            .any(|(name, _, _)| name == "acquisition.delivery.failed")
        {
            delivered = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(delivered, "delivery attempt was not recorded");
}

#[tokio::test]
async fn second_requesters_delivery_intent_is_preserved() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("content.epub");
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (test_app, _library_dir, _keep, cookie, _downloader) = import_app(content_dir.path()).await;

    // the first household member requests the book plainly
    let acquisition_id = start_acquisition(&test_app, &cookie).await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;

    // a second member asks for the same book with a send-to-reader intent
    test_app
        .state
        .auth
        .create_user("emma", "246810", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let emma_cookie = common::login(&test_app, "emma", "246810").await;
    let (status, duplicate) = post_json(
        &test_app,
        "/api/discover/acquisitions",
        &emma_cookie,
        json!({ "provider": "fake", "providerKey": "/works/OL1W", "sendToReader": true }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(
        duplicate["duplicate"], true,
        "one shared download job: {duplicate}"
    );
    assert_eq!(duplicate["id"], acquisition_id);

    // both members are registered on the shared acquisition, Emma with intent
    let requests: Vec<(String, i64)> = sqlx::query_as(
        "SELECT u.username, r.deliver_on_ready
         FROM acquisition_requests r
         JOIN users u ON u.id = r.user_id
         WHERE r.acquisition_id = ?
         ORDER BY u.username",
    )
    .bind(&acquisition_id)
    .fetch_all(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(requests.len(), 2, "requests: {requests:?}");
    assert_eq!(requests[0].0, "emma");
    assert_eq!(requests[0].1, 1);
    assert_eq!(requests[1].0, "reader");
    assert_eq!(requests[1].1, 0);

    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    let view = wait_for_status(
        &test_app,
        &cookie,
        &acquisition_id,
        &["READY", "NEEDS_REVIEW", "DOWNLOAD_FAILED"],
    )
    .await;
    assert_eq!(view["status"], "READY", "view: {view}");

    // Emma has no reader configured, so delivery fails before a record
    // exists and her intent must survive for a later retry.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let emma_pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM acquisition_requests
         WHERE acquisition_id = ? AND deliver_on_ready = 1",
    )
    .bind(&acquisition_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(emma_pending, 1, "Emma's intent must be preserved");

    // both members are notified about the ready book; the notifications are
    // written right after the delivery attempt, so poll for them.
    let mut emma_notifications = Value::Null;
    for _ in 0..500 {
        let (_, current) = get_json(&test_app, "/api/notifications", &emma_cookie).await;
        emma_notifications = current;
        if emma_notifications["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "ready")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let kinds: Vec<&str> = emma_notifications["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"ready"), "emma: {emma_notifications}");
    assert!(
        kinds.contains(&"failed"),
        "the delivery attempt is reported to Emma: {emma_notifications}"
    );

    let (_, owner_notifications) = get_json(&test_app, "/api/notifications", &cookie).await;
    let owner_kinds: Vec<&str> = owner_notifications["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["kind"].as_str())
        .collect();
    assert!(
        owner_kinds.contains(&"ready"),
        "owner: {owner_notifications}"
    );
}

#[tokio::test]
async fn completed_download_is_imported_into_the_library() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("content.epub");
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (test_app, library_dir, _keep, cookie, _downloader) = import_app(content_dir.path()).await;
    test_app
        .state
        .settings
        .set("imports.strategy", &json!("move"))
        .await
        .unwrap();
    let acquisition_id = start_acquisition(&test_app, &cookie).await;

    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    let view = wait_for_status(
        &test_app,
        &cookie,
        &acquisition_id,
        &["READY", "NEEDS_REVIEW", "DOWNLOAD_FAILED"],
    )
    .await;
    assert_eq!(view["status"], "READY", "view: {view}");

    let files: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT path, source_path FROM book_files")
            .fetch_all(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(files.len(), 1);
    let (path, source_path) = &files[0];
    let canonical = std::path::Path::new(path);
    assert!(canonical.starts_with(library_dir.path()));
    assert!(canonical.exists());
    assert!(!source.exists(), "source should be moved, not copied");
    assert_eq!(
        source_path.as_deref(),
        Some(source.to_string_lossy().as_ref())
    );

    let (_, library) = get_json(&test_app, "/api/books", &cookie).await;
    assert_eq!(library["total"], 1);
}

#[tokio::test]
async fn needs_review_flow_allows_choosing_a_file() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("mystery.epub");
    std::fs::copy(&fixtures.other, &source).unwrap();

    let (test_app, _library_dir, _keep, cookie, _downloader) = import_app(content_dir.path()).await;
    test_app
        .state
        .settings
        .set("imports.strategy", &json!("move"))
        .await
        .unwrap();
    let acquisition_id = start_acquisition(&test_app, &cookie).await;

    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["NEEDS_REVIEW"]).await;
    assert_eq!(view["errorCode"], "low_confidence");

    let (status, review) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/review"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let candidates = review["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    let path = candidates[0]["path"].as_str().unwrap().to_string();

    let (status, _resolved) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/review/resolve"),
        &cookie,
        json!({ "action": "choose", "path": path }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;
    assert_eq!(view["status"], "READY");

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(!source.exists());
}

#[tokio::test]
async fn needs_review_can_be_ignored_without_deleting_source_data() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("mystery.epub");
    std::fs::copy(&fixtures.other, &source).unwrap();

    let (test_app, _library_dir, _keep, cookie, _downloader) = import_app(content_dir.path()).await;
    let acquisition_id = start_acquisition(&test_app, &cookie).await;

    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    wait_for_status(&test_app, &cookie, &acquisition_id, &["NEEDS_REVIEW"]).await;

    let (status, _resolved) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/review/resolve"),
        &cookie,
        json!({ "action": "ignore" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["CANCELLED"]).await;
    assert_eq!(view["status"], "CANCELLED");
    assert!(source.exists(), "review must never delete source data");

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn zipped_downloads_are_imported() {
    use std::io::Write;

    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let zip_path = content_dir.path().join("download.zip");

    let file = std::fs::File::create(&zip_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file(
            "Project Hail Mary.epub",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer
        .write_all(&std::fs::read(&fixtures.hail_mary).unwrap())
        .unwrap();
    writer.finish().unwrap();

    let (test_app, _library_dir, _keep, cookie, _downloader) = import_app(content_dir.path()).await;
    let acquisition_id = start_acquisition(&test_app, &cookie).await;

    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;
    assert_eq!(view["status"], "READY");

    let (path,): (String,) = sqlx::query_as("SELECT path FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert!(path.ends_with(".epub"));
}

#[tokio::test]
async fn review_endpoints_require_admin() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    std::fs::copy(&fixtures.other, content_dir.path().join("mystery.epub")).unwrap();

    let (test_app, _library_dir, _keep, cookie, _downloader) = import_app(content_dir.path()).await;
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let user_cookie = common::login(&test_app, "bob", "password123").await;

    let acquisition_id = start_acquisition(&test_app, &cookie).await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    wait_for_status(&test_app, &cookie, &acquisition_id, &["NEEDS_REVIEW"]).await;

    let (status, _) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/review"),
        &user_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn hardlink_strategy_keeps_the_seeding_file() {
    use std::os::unix::fs::MetadataExt;

    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("content.epub");
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (test_app, _library_dir, _keep, cookie, downloader) = import_app(content_dir.path()).await;
    let acquisition_id = start_acquisition(&test_app, &cookie).await;

    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    downloader.set_progress(1.0);
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;

    assert!(
        source.exists(),
        "hardlink strategy must not remove the torrent file"
    );

    let (path,): (String,) = sqlx::query_as("SELECT path FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let canonical = std::path::Path::new(&path);
    assert!(canonical.exists());

    let source_meta = std::fs::metadata(&source).unwrap();
    let canonical_meta = std::fs::metadata(canonical).unwrap();
    assert_eq!(
        source_meta.ino(),
        canonical_meta.ino(),
        "expected a hardlink"
    );
    assert!(canonical_meta.nlink() >= 2);
}

#[tokio::test]
async fn copy_strategy_keeps_the_source_as_a_real_copy() {
    use std::os::unix::fs::MetadataExt;

    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("content.epub");
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (test_app, _library_dir, _keep, cookie, downloader) = import_app(content_dir.path()).await;
    test_app
        .state
        .settings
        .set("imports.strategy", &json!("copy"))
        .await
        .unwrap();
    let acquisition_id = start_acquisition(&test_app, &cookie).await;

    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    downloader.set_progress(1.0);
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;

    assert!(source.exists(), "copy strategy keeps the source");

    let (path,): (String,) = sqlx::query_as("SELECT path FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let canonical = std::path::Path::new(&path);
    let source_meta = std::fs::metadata(&source).unwrap();
    let canonical_meta = std::fs::metadata(canonical).unwrap();
    assert_ne!(source_meta.ino(), canonical_meta.ino());
    assert_eq!(source_meta.len(), canonical_meta.len());
}

#[tokio::test]
async fn needs_review_retry_reidentifies_after_the_file_changes() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("mystery.epub");
    std::fs::copy(&fixtures.other, &source).unwrap();

    let (test_app, _library_dir, _keep, cookie, downloader) = import_app(content_dir.path()).await;
    let acquisition_id = start_acquisition(&test_app, &cookie).await;

    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    downloader.set_progress(1.0);
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    wait_for_status(&test_app, &cookie, &acquisition_id, &["NEEDS_REVIEW"]).await;

    std::fs::remove_file(&source).unwrap();
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (status, _resolved) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/review/resolve"),
        &cookie,
        json!({ "action": "retry" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;
    assert_eq!(view["status"], "READY");
}

#[tokio::test]
async fn importing_state_is_resumed_idempotently() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    std::fs::copy(&fixtures.hail_mary, content_dir.path().join("content.epub")).unwrap();

    let (test_app, _library_dir, _keep, cookie, downloader) = import_app(content_dir.path()).await;
    let acquisition_id = start_acquisition(&test_app, &cookie).await;

    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    downloader.set_progress(1.0);
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;

    sqlx::query("UPDATE acquisitions SET status = 'IMPORTING' WHERE id = ?")
        .bind(&acquisition_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    bokhylle_server::import_pipeline::run(&test_app.state, &acquisition_id)
        .await
        .unwrap();

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;
    assert_eq!(view["status"], "READY");

    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(files, 1, "resume must not import the file twice");
}

#[tokio::test]
async fn missing_content_fails_the_import_clearly() {
    let content_dir = tempfile::tempdir().unwrap();
    let missing = content_dir.path().join("gone");

    let downloader = Arc::new(FakeDownloadProvider::default());
    downloader.set_content_path(&missing);
    downloader.set_progress(1.0);
    downloader.set_state("uploading");

    let library_dir = tempfile::tempdir().unwrap();
    let test_app = common::test_app_full(
        library_dir.path().to_path_buf(),
        Arc::new(FakeMetadataProvider::new(vec![hail_mary()])),
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

    let acquisition_id = start_acquisition(&test_app, &cookie).await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["IMPORT_FAILED"]).await;
    assert_eq!(view["errorCode"], "content_missing");
}

async fn import_once(test_app: &common::TestApp) -> (String, String, i64) {
    let cookie = common::login(test_app, "reader", "password123").await;
    let acquisition_id = start_acquisition(test_app, &cookie).await;

    wait_for_status(test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    bokhylle_server::acquisition_tracker::tick(&test_app.state)
        .await
        .unwrap();
    wait_for_status(test_app, &cookie, &acquisition_id, &["READY"]).await;

    let (path, size): (String, i64) = sqlx::query_as("SELECT path, size FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    (acquisition_id, path, size)
}

async fn simulate_crash_after_placement(
    test_app: &common::TestApp,
    acquisition_id: &str,
    path: &str,
    size: i64,
    source_path: &str,
) {
    let digest = {
        let mut file = std::fs::File::open(path).unwrap();
        use std::io::Read;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        // reuse the server's hashing by shelling through the library helper
        bokhylle_server::library::hash_file(std::path::Path::new(path)).unwrap()
    };

    sqlx::query("DELETE FROM book_files")
        .execute(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE acquisitions SET status = 'IMPORTING' WHERE id = ?")
        .bind(acquisition_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO pending_imports
            (acquisition_id, book_id, target_path, sha256, size, format, source_path)
         VALUES (?, (SELECT book_id FROM acquisitions WHERE id = ?), ?, ?, ?, 'epub', ?)",
    )
    .bind(acquisition_id)
    .bind(acquisition_id)
    .bind(path)
    .bind(&digest)
    .bind(size)
    .bind(source_path)
    .execute(&test_app.state.db)
    .await
    .unwrap();
}

#[tokio::test]
async fn crash_between_placement_and_db_is_finalized_on_resume() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("content.epub");
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (test_app, _library_dir, _keep, _cookie, _downloader) =
        import_app(content_dir.path()).await;
    let (acquisition_id, path, size) = import_once(&test_app).await;

    simulate_crash_after_placement(
        &test_app,
        &acquisition_id,
        &path,
        size,
        &source.to_string_lossy(),
    )
    .await;

    bokhylle_server::import_pipeline::run(&test_app.state, &acquisition_id)
        .await
        .unwrap();

    let cookie = common::login(&test_app, "reader", "password123").await;
    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;
    assert_eq!(view["status"], "READY");

    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(
        files, 1,
        "the placed file must be finalized, not duplicated"
    );

    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_imports")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(pending, 0, "the journal entry must be cleared");
}

#[tokio::test]
async fn stale_pending_entries_are_cleared_and_reimported() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("content.epub");
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (test_app, _library_dir, _keep, _cookie, _downloader) =
        import_app(content_dir.path()).await;
    let (acquisition_id, path, size) = import_once(&test_app).await;

    simulate_crash_after_placement(
        &test_app,
        &acquisition_id,
        &path,
        size,
        &source.to_string_lossy(),
    )
    .await;

    std::fs::remove_file(&path).unwrap();

    bokhylle_server::import_pipeline::run(&test_app.state, &acquisition_id)
        .await
        .unwrap();

    let cookie = common::login(&test_app, "reader", "password123").await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;

    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(files, 1);

    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_imports")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(pending, 0);
}

#[tokio::test]
async fn resuming_an_import_that_already_has_the_file_reuses_it() {
    let (_fixture_dir, fixtures) = fixture_books();
    let content_dir = tempfile::tempdir().unwrap();
    let source = content_dir.path().join("content.epub");
    std::fs::copy(&fixtures.hail_mary, &source).unwrap();

    let (test_app, _library_dir, _keep, _cookie, _downloader) =
        import_app(content_dir.path()).await;
    let (acquisition_id, _path, _size) = import_once(&test_app).await;

    // As if the process died after recording the file but before READY.
    sqlx::query("UPDATE acquisitions SET status = 'IMPORTING' WHERE id = ?")
        .bind(&acquisition_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    bokhylle_server::import_pipeline::run(&test_app.state, &acquisition_id)
        .await
        .unwrap();

    let cookie = common::login(&test_app, "reader", "password123").await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["READY"]).await;

    let detail: Option<String> = sqlx::query_scalar(
        "SELECT detail FROM acquisition_events
         WHERE acquisition_id = ? AND event = 'import.duplicate'",
    )
    .bind(&acquisition_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    let detail: Value = serde_json::from_str(&detail.unwrap()).unwrap();
    assert!(
        detail["fileId"].as_i64().is_some(),
        "the existing file must be resolved for delivery: {detail}"
    );
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(files, 1);
}

#[cfg(unix)]
#[tokio::test]
async fn acquisition_rejects_a_library_parent_symlink() {
    let (_fixtures, books) = fixture_books();
    let content = tempfile::tempdir().unwrap();
    std::fs::copy(&books.hail_mary, content.path().join("content.epub")).unwrap();
    let (app, _library, _keep, cookie, _downloader) = import_app(content.path()).await;
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        outside.path(),
        app.state.paths.library_root.join("Andy Weir"),
    )
    .unwrap();
    let id = start_acquisition(&app, &cookie).await;
    wait_for_status(&app, &cookie, &id, &["QUEUED"]).await;
    bokhylle_server::acquisition_tracker::tick(&app.state)
        .await
        .unwrap();
    wait_for_status(&app, &cookie, &id, &["IMPORT_FAILED"]).await;
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM book_files")
            .fetch_one(&app.state.db)
            .await
            .unwrap(),
        0
    );
}

#[cfg(unix)]
#[tokio::test]
async fn recovery_rejects_a_journal_target_outside_the_library() {
    let (_fixtures, books) = fixture_books();
    let content = tempfile::tempdir().unwrap();
    let source = content.path().join("content.epub");
    std::fs::copy(&books.hail_mary, &source).unwrap();
    let (app, _library, _keep, _cookie, _downloader) = import_app(content.path()).await;
    let (id, _path, size) = import_once(&app).await;
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("outside.epub");
    std::fs::copy(&source, &target).unwrap();
    simulate_crash_after_placement(
        &app,
        &id,
        &target.to_string_lossy(),
        size,
        &source.to_string_lossy(),
    )
    .await;
    let error = bokhylle_server::import_pipeline::run(&app.state, &id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("outside the library"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM book_files")
            .fetch_one(&app.state.db)
            .await
            .unwrap(),
        0
    );
    assert!(target.exists());
}
