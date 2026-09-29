use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;

use bokhylle_acquisition::qbittorrent::TorrentInfo;
use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_metadata::testing::FakeMetadataProvider;
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

fn torrent(name: &str, content_path: String) -> TorrentInfo {
    TorrentInfo {
        hash: name.to_string(),
        name: name.to_string(),
        state: "uploading".to_string(),
        progress: 1.0,
        size: 10,
        downloaded: 10,
        save_path: "/downloads".to_string(),
        content_path: Some(content_path),
        category: Some("books-app".to_string()),
        tags: None,
        eta: None,
        dl_speed: None,
    }
}

#[tokio::test]
async fn integration_health_reports_paths_and_hardlinks() {
    let library = tempfile::tempdir().unwrap();
    let downloader = Arc::new(FakeDownloadProvider::default());
    let test_app = common::test_app_full(
        library.path().to_path_buf(),
        Arc::new(FakeMetadataProvider::new(vec![])),
        Arc::new(FakeIndexerProvider::default()),
        downloader.clone(),
    )
    .await;
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

    // One completed download path exists locally, one does not.
    let existing = test_app.state.paths.downloads_dir.join("done.epub");
    std::fs::write(&existing, b"book").unwrap();
    downloader.set_category_torrents(vec![
        torrent("done.epub", existing.to_string_lossy().into_owned()),
        torrent("gone.epub", "/definitely/not/here/gone.epub".to_string()),
    ]);

    let (status, health) =
        get_json(&test_app, "/api/admin/integrations/status", &admin_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["qbittorrent"]["ok"], true);
    assert_eq!(health["qbittorrent"]["pathChecked"], true);
    assert_eq!(health["qbittorrent"]["completed"], 2);
    assert_eq!(
        health["qbittorrent"]["missingPaths"], 1,
        "the missing content path must be reported: {health}"
    );
    assert_eq!(
        health["qbittorrent"]["pathExamples"][0]["name"],
        "gone.epub"
    );
    assert!(
        health["qbittorrent"]["pathMessage"]
            .as_str()
            .unwrap_or_default()
            .contains("volume mapping"),
        "the message must point at the container mapping: {health}"
    );
    assert_eq!(health["library"]["writable"], true);
    assert!(
        health["hardlinks"]["supported"].is_boolean(),
        "the hardlink probe must answer: {health}"
    );

    // A healthy connection with an unlistable category must not report the
    // download path as verified.
    downloader.set_category_failing(true);
    let (status, health) =
        get_json(&test_app, "/api/admin/integrations/status", &admin_cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        health["qbittorrent"]["ok"], true,
        "the connection still works"
    );
    assert_eq!(
        health["qbittorrent"]["pathChecked"], false,
        "a failed listing leaves the path unchecked: {health}"
    );
    assert!(
        health["qbittorrent"]["pathMessage"]
            .as_str()
            .unwrap_or_default()
            .contains("Could not list downloads"),
        "the path message must say the check could not run: {health}"
    );

    let (status, _) = get_json(&test_app, "/api/admin/integrations/status", &bob_cookie).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "integration health is administration-only"
    );
}
