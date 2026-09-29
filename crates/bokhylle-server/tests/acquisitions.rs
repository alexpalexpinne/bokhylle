use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_acquisition::state::AcquisitionStatus;
use bokhylle_server::auth::Role;

mod common;

async fn app_with_book() -> (common::TestApp, tempfile::TempDir, String, i64) {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();

    let test_app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
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
        let response = get_json(&test_app, "/api/library/scan/status", &cookie).await;
        if response.1["running"] == false && response.1["summary"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let (_, books) = get_json(&test_app, "/api/books", &cookie).await;
    let book_id = books["items"][0]["id"].as_i64().unwrap();

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

#[tokio::test]
async fn create_returns_requested_and_protects_duplicates() {
    let (test_app, _library_dir, cookie, book_id) = app_with_book().await;

    let (status, created) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({ "preferredFormat": "epub", "preferredLanguage": "en" }),
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(created["status"], "REQUESTED");
    assert_eq!(created["duplicate"], false);
    let acquisition_id = created["id"].as_str().unwrap().to_string();

    let (status, duplicate) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({ "preferredFormat": "pdf" }),
    )
    .await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(duplicate["id"], acquisition_id);

    let (status, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["status"], "REQUESTED");
    assert_eq!(view["bookId"], book_id);
    assert_eq!(view["preferredFormat"], "epub");

    let (status, list) = get_json(&test_app, "/api/acquisitions", &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(!list[0]["bookTitle"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn language_variants_do_not_share_a_duplicate() {
    let (test_app, _library_dir, _cookie, book_id) = app_with_book().await;

    // A finished English acquisition must not satisfy a Swedish request.
    sqlx::query(
        "INSERT INTO acquisitions
            (id, book_id, status, preferred_language, acquisition_languages, language_key)
         VALUES ('lang-en', ?, 'READY', 'en', '[\"en\"]', 'en')",
    )
    .bind(book_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    let (swedish, duplicate) = bokhylle_server::acquisition::create_with_languages(
        &test_app.state.db,
        book_id,
        None,
        Some("epub".to_string()),
        vec!["sv".to_string()],
        false,
        false,
    )
    .await
    .unwrap();
    assert!(!duplicate, "a different language variant is new work");
    assert_ne!(swedish.id, "lang-en");
    assert_eq!(swedish.preferred_language.as_deref(), Some("sv"));
    assert_eq!(swedish.acquisition_languages.as_deref(), Some("[\"sv\"]"));

    // The same variant joins its in-flight acquisition, and an intersecting
    // policy joins it too (it may acquire an acceptable language).
    let (again, duplicate) = bokhylle_server::acquisition::create_with_languages(
        &test_app.state.db,
        book_id,
        None,
        Some("epub".to_string()),
        vec!["sv".to_string()],
        false,
        false,
    )
    .await
    .unwrap();
    assert!(duplicate);
    assert_eq!(again.id, swedish.id);
    let (joined, duplicate) = bokhylle_server::acquisition::create_with_languages(
        &test_app.state.db,
        book_id,
        None,
        Some("epub".to_string()),
        vec!["sv".to_string(), "en".to_string()],
        false,
        false,
    )
    .await
    .unwrap();
    assert!(duplicate);
    assert_eq!(joined.id, swedish.id);

    // A READY row with the same key no longer blocks a fresh acquisition.
    let (fresh, duplicate) = bokhylle_server::acquisition::create_with_languages(
        &test_app.state.db,
        book_id,
        None,
        Some("epub".to_string()),
        vec!["en".to_string()],
        false,
        false,
    )
    .await
    .unwrap();
    assert!(!duplicate, "READY is not a duplicate for new intent");

    // The pipeline evaluates the frozen intent, not the profile.
    let expected =
        bokhylle_server::acquisition_pipeline::load_expected_book(&test_app.state, book_id, &again)
            .await
            .unwrap();
    assert_eq!(expected.languages, vec!["sv".to_string()]);
    let expected =
        bokhylle_server::acquisition_pipeline::load_expected_book(&test_app.state, book_id, &fresh)
            .await
            .unwrap();
    assert_eq!(expected.languages, vec!["en".to_string()]);
}

#[tokio::test]
async fn candidate_lists_from_before_format_tier_still_load() {
    let (test_app, _library_dir, cookie, book_id) = app_with_book().await;
    let (_, created) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({}),
    )
    .await;
    let acquisition_id = created["id"].as_str().unwrap().to_string();

    // An evaluation persisted before `formatTier` and `languageIndex` existed.
    let legacy = json!({
        "candidates": [{
            "candidate": {
                "id": "legacy-1",
                "title": "Legacy Release EPUB",
                "indexer": "Legacy",
                "sizeBytes": 1000,
                "seeders": 5,
                "leechers": 0,
                "detectedFormat": "epub"
            },
            "score": 10,
            "confidence": 0.9,
            "scoreReasons": [{ "weight": 10, "reason": "title match" }],
            "rejectionReasons": []
        }],
        "queries": ["legacy"]
    });
    sqlx::query(
        "INSERT INTO acquisition_events (acquisition_id, event, detail)
         VALUES (?, 'acquisition.candidates.evaluated', ?)",
    )
    .bind(&acquisition_id)
    .bind(serde_json::to_string(&legacy).unwrap())
    .execute(&test_app.state.db)
    .await
    .unwrap();

    let (status, candidates) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/candidates"),
        &cookie,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "legacy candidate lists must load: {candidates}"
    );
    let items = candidates.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["releaseName"], "Legacy Release EPUB");
    assert_eq!(items[0]["format"], "epub");

    let (status, diagnostics) = get_json(
        &test_app,
        &format!("/api/admin/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "diagnostics must load: {diagnostics}"
    );
    assert_eq!(diagnostics["candidates"].as_array().unwrap().len(), 1);

    // The Activity controls must keep working on the same acquisition.
    let (status, view) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/keep-looking"),
        &cookie,
        json!({ "enabled": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "keep looking must work: {view}");
}

#[tokio::test]
async fn create_validates_book_and_authentication() {
    let (test_app, _library_dir, cookie, _book_id) = app_with_book().await;

    let (status, _) = post_json(
        &test_app,
        "/api/books/424242/acquisitions",
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/books/1/acquisitions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cancel_is_idempotent_and_records_events() {
    let (test_app, _library_dir, cookie, book_id) = app_with_book().await;

    let (_, created) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({}),
    )
    .await;
    let acquisition_id = created["id"].as_str().unwrap().to_string();

    let (status, cancelled) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/cancel"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "CANCELLED");

    let (status, again) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/cancel"),
        &cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["status"], "CANCELLED");

    let events = bokhylle_server::acquisition::events(&test_app.state.db, &acquisition_id)
        .await
        .unwrap();
    assert_eq!(
        events.len(),
        2,
        "created + one transition; repeat cancel is a no-op"
    );
    assert_eq!(events[0].0, "acquisition.created");
    assert_eq!(events[1].0, "acquisition.status.changed");
}

#[tokio::test]
async fn only_the_requester_or_an_admin_can_cancel_or_select() {
    let (test_app, _library_dir, admin_cookie, book_id) = app_with_book().await;

    test_app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();
    let alice_cookie = common::login(&test_app, "alice", "password123").await;
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let bob_cookie = common::login(&test_app, "bob", "password123").await;

    let (_, created) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &alice_cookie,
        json!({}),
    )
    .await;
    let acquisition_id = created["id"].as_str().unwrap().to_string();

    let (status, _) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/cancel"),
        &bob_cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/select"),
        &bob_cookie,
        json!({ "index": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // the owner gets past the permission gate (and the status check applies)
    let (status, _) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/select"),
        &alice_cookie,
        json!({ "index": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, cancelled) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/cancel"),
        &alice_cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "CANCELLED");

    // admins can manage any acquisition
    let (_, created) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &alice_cookie,
        json!({}),
    )
    .await;
    let acquisition_id = created["id"].as_str().unwrap().to_string();
    let (status, _) = post_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}/cancel"),
        &admin_cookie,
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn user_preferences_are_used_as_acquisition_defaults() {
    let (test_app, _library_dir, cookie, book_id) = app_with_book().await;

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
        .update_profile(
            user.id,
            None,
            Some("pdf".to_string()),
            Some("sv".to_string()),
            None,
        )
        .await
        .unwrap();

    let (_, created) = post_json(
        &test_app,
        &format!("/api/books/{book_id}/acquisitions"),
        &cookie,
        json!({}),
    )
    .await;
    let acquisition_id = created["id"].as_str().unwrap();
    let (_, view) = get_json(
        &test_app,
        &format!("/api/acquisitions/{acquisition_id}"),
        &cookie,
    )
    .await;
    assert_eq!(view["preferredFormat"], "pdf");
    assert_eq!(view["preferredLanguage"], "sv");
}

#[tokio::test]
async fn invalid_transitions_are_rejected_but_valid_ones_apply() {
    let (test_app, _library_dir, _cookie, book_id) = app_with_book().await;

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

    let error = bokhylle_server::acquisition::transition(
        &test_app.state.db,
        &acquisition.id,
        AcquisitionStatus::Downloading,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(error.status(), StatusCode::CONFLICT);

    let searching = bokhylle_server::acquisition::transition(
        &test_app.state.db,
        &acquisition.id,
        AcquisitionStatus::Searching,
        None,
    )
    .await
    .unwrap();
    assert_eq!(searching.status, "SEARCHING");

    let error = bokhylle_server::acquisition::transition(
        &test_app.state.db,
        &acquisition.id,
        AcquisitionStatus::Downloaded,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(error.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn acquisitions_survive_a_restart() {
    let (test_app, _library_dir, _cookie, book_id) = app_with_book().await;

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

    let database_path = test_app.state.paths.config_dir.join("bokhylle.db");
    let reopened = bokhylle_server::db::init(&database_path).await.unwrap();
    let recovered = bokhylle_server::acquisition::get(&reopened, &acquisition.id)
        .await
        .unwrap()
        .expect("acquisition should be persisted");

    assert_eq!(recovered.status, "REQUESTED");
    assert_eq!(recovered.book_id, book_id);
}

#[tokio::test]
async fn concurrent_transitions_only_one_wins() {
    let (test_app, _library_dir, _cookie, book_id) = app_with_book().await;

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

    let first = bokhylle_server::acquisition::transition(
        &test_app.state.db,
        &acquisition.id,
        AcquisitionStatus::Cancelled,
        None,
    );
    let second = bokhylle_server::acquisition::transition(
        &test_app.state.db,
        &acquisition.id,
        AcquisitionStatus::DownloadFailed,
        None,
    );

    // Both are legal from REQUESTED, but neither is legal after the other.
    // That makes the assertion independent of which future reads first.
    let (first, second) = tokio::join!(first, second);
    let successes = [first.is_ok(), second.is_ok()]
        .iter()
        .filter(|ok| **ok)
        .count();
    assert_eq!(successes, 1, "exactly one transition should win the race");

    let events = bokhylle_server::acquisition::events(&test_app.state.db, &acquisition.id)
        .await
        .unwrap();
    assert_eq!(
        events.len(),
        2,
        "only the winning transition may write an event"
    );
}

#[tokio::test]
async fn duplicates_are_detected_across_all_protected_states() {
    let (test_app, _library_dir, _cookie, book_id) = app_with_book().await;

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

    sqlx::query("UPDATE acquisitions SET status = 'SEARCHING' WHERE id = ?")
        .bind(&acquisition.id)
        .execute(&test_app.state.db)
        .await
        .unwrap();

    let (existing, duplicate) = bokhylle_server::acquisition::create(
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
    assert!(duplicate);
    assert_eq!(existing.id, acquisition.id);

    // READY is intentionally not protected: a finished acquisition in one
    // language must not satisfy a request in another.
    sqlx::query("UPDATE acquisitions SET status = 'READY' WHERE id = ?")
        .bind(&acquisition.id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let (_fresh, duplicate) = bokhylle_server::acquisition::create(
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
    assert!(!duplicate, "a READY acquisition is not a duplicate");
}

#[tokio::test]
async fn database_rejects_second_active_acquisition_for_a_book() {
    let (test_app, _library_dir, _cookie, book_id) = app_with_book().await;

    let (_acquisition, _) = bokhylle_server::acquisition::create(
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

    let error = sqlx::query(
        "INSERT INTO acquisitions (id, book_id, status) VALUES ('second', ?, 'SEARCHING')",
    )
    .bind(book_id)
    .execute(&test_app.state.db)
    .await
    .unwrap_err();

    match error {
        sqlx::Error::Database(database_error) => {
            assert!(database_error.is_unique_violation());
        }
        other => panic!("expected a unique violation, got {other}"),
    }
}
