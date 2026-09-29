mod common;

use std::sync::Arc;

use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::discovery::{DiscoveryResult, DiscoveryStatus};

async fn seed_book(state: &bokhylle_server::AppState, key: &str, title: &str) -> (i64, i64) {
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: key.to_string(),
            title: title.to_string(),
            authors: vec!["Automation Author".to_string()],
            language: Some("en".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, 'epub', 10, ?)",
    )
    .bind(edition_id)
    .bind(format!("/tmp/automation-{key}.epub"))
    .bind(format!("automation-digest-{key}"))
    .execute(&state.db)
    .await
    .unwrap();
    let file_id: i64 = sqlx::query_scalar("SELECT id FROM book_files WHERE edition_id = ?")
        .bind(edition_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    (book_id, file_id)
}

async fn automation_fixture() -> (common::TestApp, i64, i64, i64, i64) {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("reader", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let (book_id, file_id) = seed_book(&app.state, "owned-key", "Owned Automation Book").await;
    let author_id: i64 = sqlx::query_scalar("SELECT id FROM authors LIMIT 1")
        .fetch_one(&app.state.db)
        .await
        .unwrap();

    let target_id: i64 = sqlx::query_scalar(
        "INSERT INTO delivery_targets (user_id, type, name, address, connector, is_default)
         VALUES (?, 'kindle', 'Kindle', 'reader@example.com', 'email', 1)
         RETURNING id",
    )
    .bind(user.id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO author_follows (user_id, author_id, auto_acquire, delivery_target_id, baseline_at)
         VALUES (?, ?, 1, ?, unixepoch())",
    )
    .bind(user.id)
    .bind(author_id)
    .bind(target_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    (app, user.id, author_id, book_id, file_id)
}

fn owned_result(book_id: i64, file_id: i64) -> DiscoveryResult {
    DiscoveryResult {
        provider: "fake".to_string(),
        provider_key: "owned-key".to_string(),
        title: "Owned Automation Book".to_string(),
        authors: vec!["Automation Author".to_string()],
        year: Some(2026),
        language: Some("en".to_string()),
        isbn10: None,
        isbn13: None,
        series: None,
        series_number: None,
        cover_id: None,
        status: DiscoveryStatus::InLibrary,
        owned_book_id: Some(book_id),
        owned_file_id: Some(file_id),
        on_shelf: false,
        ..Default::default()
    }
}

#[tokio::test]
async fn owned_match_delivers_once_and_is_idempotent() {
    let (app, user_id, author_id, book_id, file_id) = automation_fixture().await;
    let result = owned_result(book_id, file_id);

    bokhylle_server::updates::handle_automation(
        &app.state,
        author_id,
        "Automation Author",
        &result,
        true,
    )
    .await;
    bokhylle_server::updates::handle_automation(
        &app.state,
        author_id,
        "Automation Author",
        &result,
        true,
    )
    .await;

    let acquisitions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM acquisitions")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(acquisitions, 0, "no acquisition for an owned book");

    let on_shelf: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_books WHERE user_id = ? AND book_id = ? AND on_shelf = 1",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(on_shelf, 1, "the book joins the user's shelf");

    let deliveries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deliveries WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        deliveries, 1,
        "exactly one delivery, even after a second pass"
    );

    // SMTP is not configured here, so the persisted delivery is FAILED and
    // the attempt must say so rather than claiming success.
    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM author_automation_attempts
         WHERE user_id = ? AND result = 'delivery_failed'",
    )
    .bind(user_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(attempts, 1);
}

#[tokio::test]
async fn pre_acquisition_failure_releases_the_claim() {
    let (app, user_id, author_id, book_id, file_id) = automation_fixture().await;
    let result = DiscoveryResult {
        owned_book_id: None,
        owned_file_id: None,
        status: DiscoveryStatus::NotInLibrary,
        ..owned_result(book_id, file_id)
    };

    // The fake provider cannot resolve a metadata key for this discovery, so
    // the run must fail transiently and release its claim.
    bokhylle_server::updates::handle_automation(
        &app.state,
        author_id,
        "Automation Author",
        &result,
        true,
    )
    .await;

    let attempts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM author_automation_attempts WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(attempts, 0, "a transient failure must stay retryable");
}

#[tokio::test]
async fn preferred_format_and_language_order_choose_the_file() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("picky", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE users SET preferred_language = 'sv', preferred_format = 'pdf',
             preferred_languages = ? WHERE id = ?",
    )
    .bind(r#"["sv","en"]"#)
    .bind(user.id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "picky-key".to_string(),
            title: "Picky Book".to_string(),
            authors: vec!["Automation Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let mut files = Vec::new();
    for (language, format) in [("en", "epub"), ("en", "pdf"), ("sv", "epub"), ("sv", "pdf")] {
        let edition_id: i64 = sqlx::query_scalar(
            "INSERT INTO editions (book_id, title, language) VALUES (?, 'Picky Book', ?) RETURNING id",
        )
        .bind(book_id)
        .bind(language)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        let file_id: i64 = sqlx::query_scalar(
            "INSERT INTO book_files (edition_id, path, format, size, sha256)
             VALUES (?, ?, ?, 10, ?) RETURNING id",
        )
        .bind(edition_id)
        .bind(format!("/tmp/picky-{language}.{format}"))
        .bind(format)
        .bind(format!("picky-{language}-{format}"))
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        if language == "sv" && format == "pdf" {
            files.push(file_id);
        }
    }
    let expected = files[0];

    let author_id: i64 = sqlx::query_scalar("SELECT id FROM authors LIMIT 1")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let target_id: i64 = sqlx::query_scalar(
        "INSERT INTO delivery_targets (user_id, type, name, address, connector, is_default)
         VALUES (?, 'kindle', 'Kindle', 'picky@example.com', 'email', 1) RETURNING id",
    )
    .bind(user.id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO author_follows (user_id, author_id, auto_acquire, delivery_target_id, baseline_at)
         VALUES (?, ?, 1, ?, unixepoch())",
    )
    .bind(user.id)
    .bind(author_id)
    .bind(target_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let result = owned_result(book_id, expected);
    bokhylle_server::updates::handle_automation(
        &app.state,
        author_id,
        "Automation Author",
        &result,
        true,
    )
    .await;

    let delivered: i64 = sqlx::query_scalar("SELECT file_id FROM deliveries WHERE user_id = ?")
        .bind(user.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        delivered, expected,
        "Swedish PDF must win for languages [sv,en] and format pdf"
    );
}

#[tokio::test]
async fn author_search_results_must_match_the_followed_author() {
    // An author search can return unrelated works; they must never become
    // discoveries for that author.
    let provider = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "zevin-key".to_string(),
        title: "Tomorrow, and Tomorrow, and Tomorrow".to_string(),
        authors: vec!["Gabrielle Zevin".to_string()],
        year: Some(2022),
        ..Default::default()
    }]));
    let app = common::test_app_with_metadata(provider).await;
    let user = app
        .state
        .auth
        .create_user("follower", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let author_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('Aldous Huxley', 'aldous huxley') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO author_follows (user_id, author_id) VALUES (?, ?)")
        .bind(user.id)
        .bind(author_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    bokhylle_server::updates::refresh_followed_authors(&app.state)
        .await
        .unwrap();

    let discoveries: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM author_discoveries WHERE author_id = ?")
            .bind(author_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        discoveries, 0,
        "unrelated authors must not become discoveries"
    );
}

#[tokio::test]
async fn followed_author_rail_interleaves_authors() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("multi", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();

    let mut author_ids = Vec::new();
    for (author, offset) in [("First Author", 0), ("Second Author", 100)] {
        let author_id: i64 = sqlx::query_scalar(
            "INSERT INTO authors (name, normalized_name) VALUES (?, ?) RETURNING id",
        )
        .bind(author)
        .bind(author.to_lowercase())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO author_follows (user_id, author_id) VALUES (?, ?)")
            .bind(user.id)
            .bind(author_id)
            .execute(&app.state.db)
            .await
            .unwrap();
        for index in 0..3 {
            sqlx::query(
                "INSERT INTO author_discoveries
                     (author_id, provider, provider_key, title, authors, discovered_at)
                 VALUES (?, 'fake', ?, ?, ?, unixepoch() + ?)",
            )
            .bind(author_id)
            .bind(format!("{author}-{index}"))
            .bind(format!("{author} Book {index}"))
            .bind(author)
            .bind(offset - index)
            .execute(&app.state.db)
            .await
            .unwrap();
        }
        author_ids.push(author_id);
    }

    let payload = bokhylle_server::updates::list(&app.state, user.id)
        .await
        .unwrap();
    let payload = serde_json::to_value(payload).unwrap();
    let discoveries = payload["discoveries"].as_array().unwrap();
    let returned: Vec<i64> = discoveries
        .iter()
        .map(|item| item["authorId"].as_i64().unwrap())
        .collect();
    assert!(
        returned.contains(&author_ids[0]) && returned.contains(&author_ids[1]),
        "both followed authors must appear, got {returned:?}"
    );
    for author_id in &author_ids {
        let count = returned.iter().filter(|id| *id == author_id).count();
        assert!(count <= 2, "at most two per author, got {count}");
    }
}

#[tokio::test]
async fn direct_activity_survives_a_historical_acquisition() {
    let (app, user_id, author_id, book_id, file_id) = automation_fixture().await;

    // The book was downloaded through Bokhylle long ago.
    sqlx::query("INSERT INTO acquisitions (id, book_id, status) VALUES ('old-acq', ?, 'READY')")
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    let delivery_id: i64 = sqlx::query_scalar(
        "INSERT INTO deliveries (book_id, file_id, user_id, address, status)
         VALUES (?, ?, ?, 'reader@example.com', 'SENT') RETURNING id",
    )
    .bind(book_id)
    .bind(file_id)
    .bind(user_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO author_automation_attempts
             (user_id, author_id, provider, provider_key, result, book_id, delivery_id)
         VALUES (?, ?, 'fake', 'owned-key', 'delivered', ?, ?)",
    )
    .bind(user_id)
    .bind(author_id)
    .bind(book_id)
    .bind(delivery_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let payload = bokhylle_server::updates::direct_outcomes(&app.state, user_id)
        .await
        .unwrap();
    let payload = serde_json::to_value(payload).unwrap();
    let items = payload["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        1,
        "a later direct delivery must appear despite the old acquisition"
    );
    assert_eq!(items[0]["outcome"], "delivered");
}

#[tokio::test]
async fn acquisition_attempts_are_not_direct_activity() {
    let (app, user_id, author_id, book_id, _file_id) = automation_fixture().await;
    sqlx::query(
        "INSERT INTO author_automation_attempts
             (user_id, author_id, provider, provider_key, result, book_id)
         VALUES (?, ?, 'fake', 'owned-key', 'requested', ?)",
    )
    .bind(user_id)
    .bind(author_id)
    .bind(book_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let payload = bokhylle_server::updates::direct_outcomes(&app.state, user_id)
        .await
        .unwrap();
    let payload = serde_json::to_value(payload).unwrap();
    assert!(payload["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn followed_external_authors_survive_cleanup() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("extern", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let author_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name, olid) VALUES ('External Author', 'external author', 'OL999A') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO author_follows (user_id, author_id) VALUES (?, ?)")
        .bind(user.id)
        .bind(author_id)
        .execute(&app.state.db)
        .await
        .unwrap();

    // Maintenance prunes authors without a local book; a followed external
    // author must be retained.
    bokhylle_server::maintenance::run(&app.state).await.unwrap();

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM authors WHERE id = ?")
        .bind(author_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(remaining, 1, "followed external authors must not be pruned");
}

#[tokio::test]
async fn followed_external_authors_appear_in_author_lists() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("listy", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let followed_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name, olid) VALUES ('External Followed', 'external followed', 'OL777A') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO author_follows (user_id, author_id) VALUES (?, ?)")
        .bind(user.id)
        .bind(followed_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let ghost_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('External Ghost', 'external ghost') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();

    let following =
        bokhylle_server::library::queries::list_authors(&app.state.db, None, true, user.id)
            .await
            .unwrap();
    assert!(
        following
            .iter()
            .any(|author| author.id == followed_id && author.following && author.book_count == 0),
        "a followed external author with zero books must appear under Following"
    );
    assert!(!following.iter().any(|author| author.id == ghost_id));

    let household =
        bokhylle_server::library::queries::list_authors(&app.state.db, None, false, user.id)
            .await
            .unwrap();
    assert!(
        household.iter().any(|author| author.id == followed_id),
        "scope listings must retain the user's followed authors"
    );
    assert!(
        !household.iter().any(|author| author.id == ghost_id),
        "an unfollowed, bookless author must not appear"
    );
}

#[tokio::test]
async fn refresh_followed_authors_takes_the_owned_branch_end_to_end() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "owned-refresh-key".to_string(),
        title: "Owned Refresh Book".to_string(),
        authors: vec!["Automation Author".to_string()],
        year: Some(2026),
        language: Some("en".to_string()),
        ..Default::default()
    }]));
    let app = common::test_app_with_metadata(provider).await;
    let user = app
        .state
        .auth
        .create_user(
            "refresher",
            "password123",
            bokhylle_server::auth::Role::User,
        )
        .await
        .unwrap();
    let (book_id, _file_id) =
        seed_book(&app.state, "owned-refresh-key", "Owned Refresh Book").await;
    let author_id: i64 = sqlx::query_scalar("SELECT id FROM authors LIMIT 1")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO author_follows (user_id, author_id, auto_acquire, baseline_at)
         VALUES (?, ?, 1, unixepoch())",
    )
    .bind(user.id)
    .bind(author_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    bokhylle_server::updates::refresh_followed_authors(&app.state)
        .await
        .unwrap();

    let on_shelf: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_books WHERE user_id = ? AND book_id = ? AND on_shelf = 1",
    )
    .bind(user.id)
    .bind(book_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(
        on_shelf, 1,
        "a first-seen owned release must reach the shelf through the real refresh"
    );
    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM author_automation_attempts WHERE user_id = ? AND result = 'shelved'",
    )
    .bind(user.id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(attempts, 1);
}

#[tokio::test]
async fn duplicate_acquisitions_keep_automation_provenance() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "brand-new-key".to_string(),
        title: "Brand New Release".to_string(),
        authors: vec!["Automation Author".to_string()],
        year: Some(2026),
        language: Some("en".to_string()),
        ..Default::default()
    }]));
    let app = common::test_app_with_metadata(provider).await;
    let author_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('Automation Author', 'automation author')
         RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    for username in ["first", "second"] {
        let user = app
            .state
            .auth
            .create_user(username, "password123", bokhylle_server::auth::Role::User)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO author_follows (user_id, author_id, auto_acquire, baseline_at)
             VALUES (?, ?, 1, unixepoch())",
        )
        .bind(user.id)
        .bind(author_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    }

    let result = DiscoveryResult {
        provider: "fake".to_string(),
        provider_key: "brand-new-key".to_string(),
        title: "Brand New Release".to_string(),
        authors: vec!["Automation Author".to_string()],
        year: Some(2026),
        language: Some("en".to_string()),
        isbn10: None,
        isbn13: None,
        series: None,
        series_number: None,
        cover_id: None,
        status: DiscoveryStatus::NotInLibrary,
        owned_book_id: None,
        owned_file_id: None,
        on_shelf: false,
        ..Default::default()
    };
    bokhylle_server::updates::handle_automation(
        &app.state,
        author_id,
        "Automation Author",
        &result,
        true,
    )
    .await;

    let acquisitions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM acquisitions")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(acquisitions, 1, "one shared download job");
    let sources: Vec<String> =
        sqlx::query_scalar("SELECT source FROM acquisition_requests ORDER BY user_id")
            .fetch_all(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        sources,
        vec![
            "author_automation".to_string(),
            "author_automation".to_string()
        ],
        "both requesters keep their provenance on a shared acquisition"
    );
    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM author_automation_attempts WHERE result = 'requested'",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(attempts, 2, "both followers are marked as requested");
}

#[tokio::test]
async fn owned_automation_never_sends_a_wrong_language_file() {
    let (app, user_id, author_id, book_id, _file_id) = automation_fixture().await;
    // The household owns only a Swedish edition; the follower reads English.
    sqlx::query("UPDATE editions SET language = 'sv' WHERE book_id = ?")
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE users SET preferred_language = 'en', preferred_languages = '[\"en\"]' WHERE id = ?",
    )
    .bind(user_id)
    .execute(&app.state.db)
    .await
    .unwrap();

    let result = owned_result(book_id, 0);
    bokhylle_server::updates::handle_automation(
        &app.state,
        author_id,
        "Automation Author",
        &result,
        true,
    )
    .await;

    let on_shelf: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_books WHERE user_id = ? AND book_id = ? AND on_shelf = 1",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(on_shelf, 1, "the book still joins the shelf");

    let deliveries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deliveries WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        deliveries, 0,
        "a wrong-language local file must not be sent"
    );
    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM author_automation_attempts
         WHERE user_id = ? AND result = 'delivery_failed'",
    )
    .bind(user_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(attempts, 1);
}
