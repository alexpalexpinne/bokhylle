use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_acquisition::model::ReleaseCandidate;
use bokhylle_acquisition::provider::{DownloadProvider, IndexerProvider};
use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_server::auth::Role;
use bokhylle_server::providers::StaticProviderFactory;

mod common;

const MAGNET: &str =
    "magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&dn=Project%20Hail%20Mary";

fn release(title: &str, size: i64, seeders: i64, magnet: bool) -> ReleaseCandidate {
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
        magnet_url: magnet.then(|| MAGNET.to_string()),
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

fn strong_candidates() -> Vec<ReleaseCandidate> {
    vec![
        release(
            "Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB",
            3_000_000,
            12,
            true,
        ),
        release("Project Hail Mary PDF", 14_000_000, 5, false),
    ]
}

fn ambiguous_candidates() -> Vec<ReleaseCandidate> {
    vec![
        release("Project.Hail.Mary.2021.epub", 2_200_000, 2, true),
        release("Project.Hail.Mary.2021.epub.release", 2_240_000, 2, true),
    ]
}

fn rejected_candidates() -> Vec<ReleaseCandidate> {
    vec![
        release("Project Hail Mary German EPUB", 2_500_000, 3, true),
        release(
            "Andy Weir Complete Collection EPUB MOBI",
            840_000_000,
            20,
            true,
        ),
        release("Project Hail Mary Audiobook M4B", 300_000_000, 9, true),
    ]
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

    scan_library(&test_app, &cookie).await;
    let book_id = project_hail_mary_id(&test_app, &cookie).await;

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

async fn scan_library(test_app: &common::TestApp, cookie: &str) {
    let status = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/library/scan")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status();
    assert_eq!(status, StatusCode::ACCEPTED);

    for _ in 0..200 {
        let (_, status) = get_json(test_app, "/api/library/scan/status", cookie).await;
        if status["running"] == false && status["summary"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

async fn project_hail_mary_id(test_app: &common::TestApp, cookie: &str) -> i64 {
    let (_, books) = get_json(test_app, "/api/books?pageSize=50", cookie).await;
    books["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["title"] == "Project Hail Mary")
        .expect("Project Hail Mary fixture")["id"]
        .as_i64()
        .unwrap()
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

async fn create_acquisition(test_app: &common::TestApp, cookie: &str, book_id: i64) -> String {
    let (status, created) = post_json(
        test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    created["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn preview_selection_downloads_the_requested_version() {
    let mut candidates = strong_candidates();
    candidates.push(release(
        "Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB.Alternative",
        4_000_000,
        2,
        true,
    ));
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (app, _library, cookie, book_id) = app_with_book(
        Arc::new(FakeIndexerProvider::with_candidates(candidates)),
        downloader.clone(),
    )
    .await;
    let (status, preview) = get_json(
        &app,
        &format!("/api/discover/releases?provider=local&providerKey=local:{book_id}&format=epub"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let choice = preview["releases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|release| {
            release["releaseName"]
                .as_str()
                .unwrap()
                .ends_with("Alternative")
        })
        .unwrap();
    assert!(choice["selectionKey"].is_string());
    assert!(downloader.added().is_empty());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM acquisitions")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 0, "preview must not start an acquisition");

    let (status, created) = post_json(
        &app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({ "preferredFormat": "epub", "releaseKey": choice["selectionKey"] }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let view = wait_for_status(&app, &cookie, created["id"].as_str().unwrap(), &["QUEUED"]).await;
    assert_eq!(view["selectedReleaseName"], choice["releaseName"]);
    assert_eq!(downloader.added().len(), 1);
}

#[tokio::test]
async fn missing_preview_selection_requires_another_choice() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (app, _library, cookie, book_id) = app_with_book(indexer.clone(), downloader.clone()).await;
    let (_, preview) = get_json(
        &app,
        &format!("/api/discover/releases?provider=local&providerKey=local:{book_id}&format=epub"),
        &cookie,
    )
    .await;
    let key = preview["releases"][0]["selectionKey"].as_str().unwrap();
    indexer.set_candidates(vec![release(
        "Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB.Replacement",
        4_000_000,
        12,
        true,
    )]);
    let (status, created) = post_json(
        &app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({ "preferredFormat": "epub", "releaseKey": key }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = created["id"].as_str().unwrap();
    let view = wait_for_status(&app, &cookie, id, &["NEEDS_SELECTION"]).await;
    assert!(downloader.added().is_empty(), "a replacement needs consent");
    assert_eq!(view["errorCode"], "requested_version_unavailable");
    let (status, _) = post_json(
        &app,
        &format!("/api/acquisitions/{id}/select"),
        &cookie,
        json!({ "index": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let view = wait_for_status(&app, &cookie, id, &["QUEUED"]).await;
    assert!(view["errorCode"].is_null());
    assert!(
        view["selectedReleaseName"]
            .as_str()
            .unwrap()
            .ends_with("Replacement")
    );
    assert_eq!(downloader.added().len(), 1);
}

#[tokio::test]
async fn pipeline_queues_high_confidence_release() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());

    let (test_app, _library_dir, cookie, book_id) =
        app_with_book(indexer, downloader.clone()).await;
    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;

    let view = wait_for_status(
        &test_app,
        &cookie,
        &acquisition_id,
        &["QUEUED", "DOWNLOAD_FAILED", "NO_RELEASE_FOUND"],
    )
    .await;
    assert_eq!(view["status"], "QUEUED");
    assert_eq!(
        view["selectedReleaseName"],
        "Andy.Weir.Project.Hail.Mary.Retail.EN.EPUB"
    );
    assert_eq!(view["selectedReleaseFormat"], "epub");
    assert_eq!(view["selectedReleaseSize"], 3_000_000);
    assert_eq!(view["selectedReleaseSeeders"], 12);
    assert_eq!(view["requestedBy"], "reader");
    assert_eq!(view["downloadProvider"], "fake-downloader");

    let added = downloader.added();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0].tag, format!("books-acquisition-{acquisition_id}"));
    assert_eq!(added[0].category, "books-app");
    assert!(added[0].magnet);

    let stored = bokhylle_server::acquisition::get(&test_app.state.db, &acquisition_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.provider_download_id.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );

    let events = bokhylle_server::acquisition::events(&test_app.state.db, &acquisition_id)
        .await
        .unwrap();
    let names: Vec<&str> = events.iter().map(|event| event.0.as_str()).collect();
    assert!(names.contains(&"acquisition.candidates.evaluated"));
    assert!(names.contains(&"acquisition.release.selected"));
    assert!(names.contains(&"acquisition.download.queued"));
}

#[tokio::test]
async fn ask_mode_pauses_for_selection_even_for_confident_matches() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) =
        app_with_book(indexer, downloader.clone()).await;

    let user = test_app
        .state
        .auth
        .verify_login("reader", "password123")
        .await
        .unwrap()
        .unwrap();
    test_app
        .state
        .auth
        .update_profile(user.id, None, None, None, Some("ask".to_string()))
        .await
        .unwrap();

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["NEEDS_SELECTION"]).await;
    assert_eq!(view["status"], "NEEDS_SELECTION");
    assert_eq!(view["askBeforeDownload"], true);
    assert!(downloader.added().is_empty(), "nothing may be queued yet");

    let (status, _) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/select"),
        &cookie,
        json!({ "index": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    assert_eq!(view["status"], "QUEUED");
}

#[tokio::test]
async fn pipeline_requests_selection_when_ambiguous() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(ambiguous_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());

    let (test_app, _library_dir, cookie, book_id) =
        app_with_book(indexer, downloader.clone()).await;
    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["NEEDS_SELECTION"]).await;
    assert_eq!(view["status"], "NEEDS_SELECTION");

    let (status, candidates) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/candidates"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(candidates.as_array().unwrap().len(), 2);
    assert!(candidates[0]["releaseName"].is_string());
    assert!(candidates[0]["seeders"].is_i64());
    assert_eq!(candidates[0]["format"], "epub");

    let (status, technical) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/candidates?technical=true"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(technical[0]["releaseName"].is_string());

    let (status, _selected) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/select"),
        &cookie,
        json!({ "index": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;
    assert_eq!(view["selectedReleaseName"], "Project.Hail.Mary.2021.epub");
    assert_eq!(downloader.added().len(), 1);
}

#[tokio::test]
async fn adults_can_choose_once_without_changing_their_default_or_a_shared_download() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(ambiguous_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (app, _library, _admin_cookie, book_id) = app_with_book(indexer, downloader.clone()).await;
    let alice = app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();
    let bob = app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let alice_cookie = common::login(&app, "alice", "password123").await;
    let bob_cookie = common::login(&app, "bob", "password123").await;
    let (status, created) = post_json(
        &app,
        &format!("/api/books/{book_id}/acquisitions"),
        &alice_cookie,
        json!({"askBeforeDownload":true}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let id = created["id"].as_str().unwrap();
    let view = wait_for_status(&app, &alice_cookie, id, &["NEEDS_SELECTION"]).await;
    assert_eq!(view["askBeforeDownload"], true);
    assert!(downloader.added().is_empty());
    let mode: String = sqlx::query_scalar("SELECT acquisition_mode FROM users WHERE id = ?")
        .bind(alice.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        mode, "automatic",
        "a one-book choice leaves the account default intact"
    );
    let (status, candidates) = get_json(
        &app,
        &format!("/api/acquisitions/{id}/candidates"),
        &alice_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(candidates[1]["releaseName"].is_string());
    assert_eq!(
        get_json(
            &app,
            &format!("/api/acquisitions/{id}/candidates?technical=true"),
            &alice_cookie
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, joined) = post_json(
        &app,
        &format!("/api/books/{book_id}/acquisitions"),
        &bob_cookie,
        json!({"askBeforeDownload":false}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(joined["id"], id);
    assert_eq!(joined["duplicate"], true);
    assert_eq!(
        bokhylle_server::acquisition::get(&app.state.db, id)
            .await
            .unwrap()
            .unwrap()
            .user_id,
        Some(alice.id)
    );
    assert!(
        bokhylle_server::acquisition::get(&app.state.db, id)
            .await
            .unwrap()
            .unwrap()
            .ask_before_download
    );
    assert_eq!(
        post_json(
            &app,
            &format!("/api/acquisitions/{id}/select"),
            &bob_cookie,
            json!({"index":1})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post_json(
            &app,
            &format!("/api/acquisitions/{id}/select"),
            &alice_cookie,
            json!({"index":1})
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
    let view = wait_for_status(&app, &alice_cookie, id, &["QUEUED"]).await;
    assert_eq!(view["selectedReleaseName"], candidates[1]["releaseName"]);
    assert_eq!(downloader.added().len(), 1);
    assert_eq!(
        post_json(
            &app,
            &format!("/api/acquisitions/{id}/select"),
            &alice_cookie,
            json!({"index":0})
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "the chosen version cannot be changed after queueing"
    );
    assert!(
        bokhylle_server::user_books::contains(&app.state.db, bob.id, book_id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn pipeline_reports_no_release_found_when_all_rejected() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(rejected_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());

    let (test_app, _library_dir, cookie, book_id) =
        app_with_book(indexer, downloader.clone()).await;
    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["NO_RELEASE_FOUND"]).await;
    assert_eq!(view["status"], "NO_RELEASE_FOUND");
    assert!(downloader.added().is_empty());
}

#[tokio::test]
async fn pipeline_fails_when_indexer_errors() {
    let indexer = Arc::new(FakeIndexerProvider::failing());
    let downloader = Arc::new(FakeDownloadProvider::default());

    let (test_app, _library_dir, cookie, book_id) = app_with_book(indexer, downloader).await;
    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["DOWNLOAD_FAILED"]).await;
    assert_eq!(view["errorCode"], "search_failed");
}

#[tokio::test]
async fn pipeline_fails_when_integrations_are_missing() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let factory = Arc::new(StaticProviderFactory {
        indexer: None,
        downloader: Some(Arc::new(FakeDownloadProvider::default())),
    });
    let test_app = common::test_app_with_factory(library_dir.path().to_path_buf(), factory).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    scan_library(&test_app, &cookie).await;
    let book_id = project_hail_mary_id(&test_app, &cookie).await;
    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["DOWNLOAD_FAILED"]).await;
    assert_eq!(view["errorCode"], "integrations_not_configured");
}

#[tokio::test]
async fn pipeline_fails_when_downloader_is_missing() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let factory = Arc::new(StaticProviderFactory {
        indexer: Some(indexer),
        downloader: None,
    });
    let test_app = common::test_app_with_factory(library_dir.path().to_path_buf(), factory).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    scan_library(&test_app, &cookie).await;
    let book_id = project_hail_mary_id(&test_app, &cookie).await;
    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;

    let view = wait_for_status(&test_app, &cookie, &acquisition_id, &["DOWNLOAD_FAILED"]).await;
    assert_eq!(view["errorCode"], "integrations_not_configured");
}

#[tokio::test]
async fn recovery_resumes_incomplete_acquisitions() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());

    let (test_app, _library_dir, _cookie, book_id) =
        app_with_book(indexer, downloader.clone()).await;

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

    bokhylle_server::acquisition_pipeline::recover(&test_app.state).await;

    let cookie = common::login(&test_app, "reader", "password123").await;
    let view = wait_for_status(&test_app, &cookie, &acquisition.id, &["QUEUED"]).await;
    assert_eq!(view["status"], "QUEUED");
    assert_eq!(downloader.added().len(), 1);
}

#[tokio::test]
async fn admin_diagnostics_expose_technical_details() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());

    let (test_app, _library_dir, cookie, book_id) = app_with_book(indexer, downloader).await;
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let user_cookie = common::login(&test_app, "bob", "password123").await;

    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;
    wait_for_status(&test_app, &cookie, &acquisition_id, &["QUEUED"]).await;

    let (status, _) = get_json(
        &test_app,
        &format!("/api/admin/acquisitions/{acquisition_id}"),
        &user_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, diagnostics) = get_json(
        &test_app,
        &format!("/api/admin/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(diagnostics["acquisition"]["status"], "QUEUED");
    assert!(diagnostics["acquisition"]["providerDownloadId"].is_string());
    assert!(diagnostics["providerState"]["state"].is_string());
    assert!(
        diagnostics["candidates"][0]["candidate"]["title"]
            .as_str()
            .unwrap()
            .contains("Project.Hail.Mary")
    );
    assert!(
        diagnostics["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["event"] == "acquisition.download.queued")
    );
}

#[tokio::test]
async fn acquisition_reads_require_membership() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, admin_cookie, book_id) = app_with_book(indexer, downloader).await;

    let acquisition_id = create_acquisition(&test_app, &admin_cookie, book_id).await;

    // A second member joins the shared request.
    test_app
        .state
        .auth
        .create_user("member", "password123", Role::User)
        .await
        .unwrap();
    let member_cookie = common::login(&test_app, "member", "password123").await;
    let (status, _) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &member_cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // An unrelated member cannot read it, by id or through candidates.
    test_app
        .state
        .auth
        .create_user("outsider", "password123", Role::User)
        .await
        .unwrap();
    let outsider_cookie = common::login(&test_app, "outsider", "password123").await;
    let (status, _) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &outsider_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/candidates"),
        &outsider_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Requesters can read their shared acquisition.
    let (status, _) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &member_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn acquisition_creation_commits_request_event_and_shelf_together() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) = app_with_book(indexer, downloader).await;
    let acquisition_id = create_acquisition(&test_app, &cookie, book_id).await;

    let requests: i64 =
        sqlx::query_scalar("SELECT count(*) FROM acquisition_requests WHERE acquisition_id = ?")
            .bind(&acquisition_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(requests, 1, "the initial request commits with the job");
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM acquisition_events
         WHERE acquisition_id = ? AND event = 'acquisition.created'",
    )
    .bind(&acquisition_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(events, 1, "the created event commits with the job");
    let shelf: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM user_books ub
         JOIN acquisitions a ON a.book_id = ub.book_id
         WHERE a.id = ? AND ub.on_shelf = 1",
    )
    .bind(&acquisition_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(shelf, 1, "shelf membership commits with the job");
}

#[tokio::test]
async fn a_different_language_starts_its_own_acquisition_and_peers_join() {
    let indexer = Arc::new(FakeIndexerProvider::with_candidates(strong_candidates()));
    let downloader = Arc::new(FakeDownloadProvider::default());
    let (test_app, _library_dir, cookie, book_id) = app_with_book(indexer, downloader).await;

    // The first requester asks for English.
    let (status, created) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({ "preferredLanguage": "en" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let english_id = created["id"].as_str().unwrap().to_string();

    // A Swedish-only member gets their own variant: the English copy does not
    // satisfy their policy.
    test_app
        .state
        .auth
        .create_user("swede", "password123", Role::User)
        .await
        .unwrap();
    let swede_cookie = common::login(&test_app, "swede", "password123").await;
    let (status, swedish) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &swede_cookie,
        json!({ "preferredLanguage": "sv", "sendToReader": true }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(swedish["duplicate"], false);
    let swedish_id = swedish["id"].as_str().unwrap().to_string();
    assert_ne!(swedish_id, english_id);
    let (language, key): (Option<String>, String) =
        sqlx::query_as("SELECT preferred_language, language_key FROM acquisitions WHERE id = ?")
            .bind(&swedish_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(language.as_deref(), Some("sv"));
    assert_eq!(key, "sv");

    // A member who also accepts English joins the English job.
    test_app
        .state
        .auth
        .create_user("mate", "password123", Role::User)
        .await
        .unwrap();
    let mate_cookie = common::login(&test_app, "mate", "password123").await;
    let (status, duplicate) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &mate_cookie,
        json!({ "preferredLanguage": "en", "sendToReader": true }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["id"], english_id);

    let (requests, intent): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM acquisition_requests r WHERE r.acquisition_id = a.id),
                (SELECT COALESCE(MAX(r.deliver_on_ready), 0) FROM acquisition_requests r
                 WHERE r.acquisition_id = a.id)
         FROM acquisitions a WHERE a.id = ?",
    )
    .bind(&english_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(requests, 2, "the English job is shared by its requesters");
    assert_eq!(intent, 1, "the joiner's send intent is preserved");
}
