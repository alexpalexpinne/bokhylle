use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use bokhylle_server::{
    AppState, app,
    auth::Role,
    backup,
    server::{
        self,
        releases::{ReleaseChecker, UpdateState},
    },
    settings,
};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};
mod common;

async fn request(
    test: &common::TestApp,
    path: &str,
    cookie: Option<&str>,
    method: &str,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = test
        .router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn server_administration_is_only_available_to_admins() {
    let test = common::test_app().await;
    test.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    test.state
        .auth
        .create_user("adult", "password123", Role::User)
        .await
        .unwrap();
    let child = test
        .state
        .auth
        .create_user("child", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(child.id)
        .execute(&test.state.db)
        .await
        .unwrap();
    let admin = common::login(&test, "admin", "password123").await;
    let adult = common::login(&test, "adult", "password123").await;
    let child = common::login(&test, "child", "password123").await;
    for (endpoint, method) in [
        ("/api/admin/server", "GET"),
        ("/api/admin/server/restart", "GET"),
        ("/api/admin/server/diagnostics", "GET"),
        ("/api/admin/server/updates", "GET"),
        ("/api/admin/server/updates", "POST"),
        ("/api/admin/maintenance/backups", "GET"),
    ] {
        assert_eq!(
            request(&test, endpoint, None, method).await.0,
            StatusCode::UNAUTHORIZED
        );
        for cookie in [&adult, &child] {
            assert_eq!(
                request(&test, endpoint, Some(cookie), method).await.0,
                StatusCode::FORBIDDEN
            );
        }
        if method == "GET" {
            assert_eq!(
                request(&test, endpoint, Some(&admin), method).await.0,
                StatusCode::OK
            );
        }
    }
    let (_, status) = request(&test, "/api/admin/server", Some(&admin), "GET").await;
    assert_eq!(status["build"]["version"], bokhylle_core::VERSION);
    assert!(status["startedAt"].as_i64().unwrap() > 0);
    assert!(status["databaseOk"].as_bool().unwrap());
    assert_eq!(status["restartRequired"], json!([]));
    #[cfg(unix)]
    assert_eq!(
        status["storage"].as_array().unwrap().len(),
        1,
        "paths on one filesystem share one capacity report"
    );
}

#[tokio::test]
async fn restart_tracking_only_marks_effective_startup_changes_and_clears_on_revert() {
    let test = common::test_app().await;
    test.state
        .settings
        .set(settings::SCAN_INTERVAL_HOURS, &json!(3))
        .await
        .unwrap();
    test.state
        .settings
        .set(settings::BACKUP_KEEP, &json!(2))
        .await
        .unwrap();
    assert!(
        test.state
            .server
            .pending_restart(&test.state.settings)
            .await
            .unwrap()
            .is_empty()
    );
    test.state
        .settings
        .set(settings::METADATA_PROVIDER, &json!("google_books"))
        .await
        .unwrap();
    test.state
        .settings
        .set(
            settings::GOOGLE_BOOKS_API_KEY,
            &json!("never-export-this-key"),
        )
        .await
        .unwrap();
    let pending = test
        .state
        .server
        .pending_restart(&test.state.settings)
        .await
        .unwrap();
    assert_eq!(pending.len(), 2);
    assert!(
        !serde_json::to_string(&pending)
            .unwrap()
            .contains("never-export-this-key")
    );
    test.state
        .settings
        .set(settings::METADATA_PROVIDER, &json!("automatic"))
        .await
        .unwrap();
    test.state
        .settings
        .set(settings::GOOGLE_BOOKS_API_KEY, &json!(""))
        .await
        .unwrap();
    assert!(
        test.state
            .server
            .pending_restart(&test.state.settings)
            .await
            .unwrap()
            .is_empty()
    );
    test.state
        .settings
        .set(settings::LIBRARY_ROOT, &json!("/new-library"))
        .await
        .unwrap();
    assert_eq!(
        test.state
            .server
            .pending_restart(&test.state.settings)
            .await
            .unwrap()[0]
            .key,
        settings::LIBRARY_ROOT
    );
    // A new runtime captures the newly effective configuration.
    let runtime = server::ServerRuntime::capture(&test.state.settings, &test.state.paths)
        .await
        .unwrap();
    assert!(
        runtime
            .pending_restart(&test.state.settings)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn restart_tracking_respects_environment_overrides() {
    const CHILD: &str = "BOKHYLLE_TEST_SERVER_ENV_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let dir = tempfile::tempdir().unwrap();
        // Environment changes are isolated in a child process, never raced with other tests.
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "restart_tracking_respects_environment_overrides",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .env("BOKHYLLE_LIBRARY_DIR", dir.path())
            .env("BOKHYLLE_GOOGLE_BOOKS_API_KEY", "environment-key")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "isolated environment test failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let test = common::test_app().await;
    test.state
        .settings
        .set(settings::LIBRARY_ROOT, &json!("/stored-but-masked"))
        .await
        .unwrap();
    test.state
        .settings
        .set(
            settings::GOOGLE_BOOKS_API_KEY,
            &json!("stored-but-masked-key"),
        )
        .await
        .unwrap();
    assert!(
        test.state
            .server
            .pending_restart(&test.state.settings)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn backup_outcomes_survive_restarts_and_retry_without_losing_a_valid_snapshot() {
    let test = common::test_app().await;
    let start = 1_700_000_000;
    backup::scheduler_tick_at(&test.state, start).await.unwrap();
    let status = backup::status_at(&test.state, start).await.unwrap();
    assert_eq!(status.outcome, "succeeded");
    assert_eq!(status.last_success.as_ref().unwrap().created_at, start);
    assert_eq!(status.next_scheduled_at, Some(start + 24 * 3600));
    let config = test.state.paths.config_dir.clone();
    let library = test.state.paths.library_root.clone();
    let backups = config.join("backups");
    let saved = config.join("saved-backups");
    std::fs::rename(&backups, &saved).unwrap();
    std::fs::write(&backups, b"not a directory").unwrap();
    let failure = start + 24 * 3600;
    assert!(
        backup::scheduler_tick_at(&test.state, failure)
            .await
            .is_err()
    );
    let status = backup::status_at(&test.state, failure).await.unwrap();
    assert_eq!(status.outcome, "failed");
    assert_eq!(status.last_failure.as_ref().unwrap().at, failure);
    assert_eq!(status.last_success.as_ref().unwrap().created_at, start);
    assert!(status.inventory_error.is_some());
    assert_eq!(status.next_scheduled_at, Some(failure + 300));
    test.state.db.close().await;
    let restarted = common::test_app_from_existing_config(config, library).await;
    let status = backup::status_at(&restarted.state, failure).await.unwrap();
    assert_eq!(status.outcome, "failed");
    assert_eq!(status.last_success.as_ref().unwrap().created_at, start);
    std::fs::remove_file(&backups).unwrap();
    std::fs::rename(saved, backups).unwrap();
    backup::scheduler_tick_at(&restarted.state, failure + 299)
        .await
        .unwrap();
    assert_eq!(
        backup::status_at(&restarted.state, failure + 299)
            .await
            .unwrap()
            .outcome,
        "failed"
    );
    backup::scheduler_tick_at(&restarted.state, failure + 300)
        .await
        .unwrap();
    assert_eq!(
        backup::status_at(&restarted.state, failure + 300)
            .await
            .unwrap()
            .outcome,
        "succeeded"
    );
    restarted
        .state
        .settings
        .set(settings::BACKUP_INTERVAL_HOURS, &json!(0))
        .await
        .unwrap();
    assert_eq!(
        backup::status_at(&restarted.state, failure + 300)
            .await
            .unwrap()
            .next_scheduled_at,
        None
    );
}

#[tokio::test]
async fn interrupted_backup_attempts_become_visible_failures() {
    let test = common::test_app().await;
    sqlx::query(
        "UPDATE server_backup_state SET outcome = 'running', last_attempt_at = 1000 WHERE id = 1",
    )
    .execute(&test.state.db)
    .await
    .unwrap();
    backup::recover(&test.state).await.unwrap();
    let status = backup::status_at(&test.state, server::now()).await.unwrap();
    assert_eq!(status.outcome, "failed");
    assert!(
        status
            .last_failure
            .unwrap()
            .summary
            .unwrap()
            .contains("server stopped")
    );
}

async fn release_state(test: &common::TestApp, mock: &MockServer) -> AppState {
    let mut state = test.state.clone();
    let mut runtime = server::ServerRuntime::capture(&state.settings, &state.paths)
        .await
        .unwrap();
    runtime.releases = Arc::new(ReleaseChecker::with_endpoint(format!(
        "{}/latest",
        mock.uri()
    )));
    state.server = Arc::new(runtime);
    state
}

#[tokio::test]
async fn release_checks_are_cached_bounded_and_survive_offline_restarts() {
    let test = common::test_app().await;
    let mock = MockServer::start().await;
    Mock::given(method("GET")).and(path("/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tag_name":"v0.10.0", "draft":false,"prerelease":false,"html_url":"https://evil.test/?secret=never-use-this"})))
        .expect(1).mount(&mock).await;
    let state = release_state(&test, &mock).await;
    let (one, two) = tokio::join!(
        state.server.releases.check_at(&state, true, 1000),
        state.server.releases.check_at(&state, true, 1000)
    );
    assert!(matches!(one.unwrap().state, UpdateState::UpdateAvailable));
    let status = two.unwrap();
    assert_eq!(
        status.release_url.as_deref(),
        Some("https://github.com/alexpalexpinne/bokhylle/releases/tag/v0.10.0")
    );
    mock.verify().await;
    mock.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503).set_body_string("private-provider-error"))
        .expect(1)
        .mount(&mock)
        .await;
    let status = state
        .server
        .releases
        .check_at(&state, true, 1061)
        .await
        .unwrap();
    assert!(matches!(status.state, UpdateState::Unavailable));
    assert_eq!(status.latest_version.as_deref(), Some("0.10.0"));
    assert_eq!(status.last_success_at, Some(1000));
    assert_eq!(status.checked_at, Some(1061));
    assert!(
        !serde_json::to_string(&status)
            .unwrap()
            .contains("private-provider-error")
    );
    let rebuilt = release_state(&test, &mock).await;
    assert!(matches!(
        rebuilt
            .server
            .releases
            .status(&rebuilt)
            .await
            .unwrap()
            .state,
        UpdateState::Unavailable
    ));
    let anonymous = app(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/server/updates")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn release_awareness_rejects_prereleases_malformed_and_oversized_responses() {
    let test = common::test_app().await;
    let mock = MockServer::start().await;
    let state = release_state(&test, &mock).await;
    let cases = [
        (
            json!({"tag_name":"v0.1.0", "draft":false,"prerelease":false}),
            0,
        ),
        (
            json!({"tag_name":"v0.0.9", "draft":false,"prerelease":false}),
            1,
        ),
        (
            json!({"tag_name":"v0.2.0-beta", "draft":false,"prerelease":true}),
            2,
        ),
        (
            json!({"tag_name":"../../private-path", "draft":false,"prerelease":false}),
            2,
        ),
    ];
    for (index, (body, expected)) in cases.into_iter().enumerate() {
        mock.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&mock)
            .await;
        let status = state
            .server
            .releases
            .check_at(&state, true, 1000 + index as i64 * 61)
            .await
            .unwrap();
        assert!(match expected {
            0 => matches!(status.state, UpdateState::UpToDate),
            1 => matches!(status.state, UpdateState::NewerBuild),
            _ => matches!(status.state, UpdateState::Unavailable),
        });
    }
    mock.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 128 * 1024 + 1]))
        .mount(&mock)
        .await;
    assert!(matches!(
        state
            .server
            .releases
            .check_at(&state, true, 2000)
            .await
            .unwrap()
            .state,
        UpdateState::Unavailable
    ));
    mock.reset().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock)
        .await;
    assert!(matches!(
        state
            .server
            .releases
            .check_at(&state, true, 2061)
            .await
            .unwrap()
            .state,
        UpdateState::NoRelease
    ));
}

#[tokio::test]
async fn diagnostics_export_counts_and_configuration_without_private_fields() {
    let test = common::test_app().await;
    test.state
        .auth
        .create_user("private-person", "password123", Role::Admin)
        .await
        .unwrap();
    test.state
        .settings
        .set(settings::SMTP_PASSWORD, &json!("private-password"))
        .await
        .unwrap();
    test.state
        .settings
        .set(
            settings::PROWLARR_URL,
            &json!("http://private-host.test/?apikey=private-key"),
        )
        .await
        .unwrap();
    test.state
        .settings
        .set(settings::GOOGLE_BOOKS_API_KEY, &json!("private-google-key"))
        .await
        .unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO books (title, normalized_title) VALUES ('Private Book', 'private book') RETURNING id").fetch_one(&test.state.db).await.unwrap();
    let edition: i64 = sqlx::query_scalar(
        "INSERT INTO editions (book_id, title) VALUES (?, 'Private Book') RETURNING id",
    )
    .bind(id)
    .fetch_one(&test.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, '/private-household/missing.epub', 'epub', 1, 'private-sha')").bind(edition).execute(&test.state.db).await.unwrap();
    let capture = bokhylle_server::observability::LogCapture::new();
    tracing::warn!(
        url = "https://private-url.test",
        token = "private-token",
        "private-error-message"
    );
    assert!(capture.logs().contains("private-token"));
    let cookie = common::login(&test, "private-person", "password123").await;
    let (status, body) =
        request(&test, "/api/admin/server/diagnostics", Some(&cookie), "GET").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["library"]["missingFiles"], 1);
    assert_eq!(body["integrations"][0]["configured"], true);
    let text = body.to_string();
    for private in [
        "private-password",
        "private-key",
        "private-host",
        "private-google-key",
        "private-person",
        "Private Book",
        "private-household",
        "private-url",
        "private-token",
        "private-error-message",
    ] {
        assert!(!text.contains(private), "diagnostics leaked {private}");
    }
    assert!(!text.contains(test.state.paths.config_dir.to_str().unwrap()));
}
