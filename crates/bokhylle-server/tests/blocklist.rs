use bokhylle_metadata::MetadataResult;

mod common;

#[tokio::test]
async fn import_failure_blocklists_the_release_fingerprint() {
    let test_app = common::test_app().await;
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLBLOCKW".to_string(),
            title: "Blocked Book".to_string(),
            authors: vec!["Some Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO acquisitions
            (id, book_id, status, selected_release_name, selected_release_indexer, selected_release_key)
         VALUES ('acq-block', ?, 'IMPORTING', 'Dune.Frank.Herbert.EPUB', 'Indexer A', 'indexer a::bad-guid')",
    )
    .bind(book_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    bokhylle_server::acquisition::fail_import(
        &test_app.state.db,
        "acq-block",
        "unsupported_files",
        "the release contained a different book",
    )
    .await
    .unwrap();

    let (name, indexer, reason): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT release_name, indexer, reason FROM release_blocklist WHERE release_key = ?",
    )
    .bind("indexer a::bad-guid")
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(name, "Dune.Frank.Herbert.EPUB");
    assert_eq!(indexer.as_deref(), Some("Indexer A"));
    assert_eq!(
        reason.as_deref(),
        Some("the release contained a different book")
    );

    let blocked = bokhylle_server::acquisition::blocked_release_keys(&test_app.state.db)
        .await
        .unwrap();
    assert!(blocked.contains("indexer a::bad-guid"));

    // A release without a stored fingerprint (older rows) must not break the
    // failure path or add junk to the blocklist.
    sqlx::query(
        "INSERT INTO acquisitions (id, book_id, status, selected_release_name)
         VALUES ('acq-no-key', ?, 'IMPORTING', 'Unknown.EPUB')",
    )
    .bind(book_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();
    bokhylle_server::acquisition::fail_import(&test_app.state.db, "acq-no-key", "x", "y")
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM release_blocklist")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn retry_reopens_failed_requests_and_clears_the_selection() {
    let test_app = common::test_app().await;
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLRETRYW".to_string(),
            title: "Retry Book".to_string(),
            authors: vec!["Some Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO acquisitions
            (id, book_id, status, error_code, error_message, selected_release_name,
             selected_release_indexer, selected_release_key, provider_download_id)
         VALUES ('acq-retry', ?, 'IMPORT_FAILED', 'wrong_book', 'it was another book',
                 'Bad.Release.EPUB', 'Indexer A', 'indexer a::bad-guid', 'download-1')",
    )
    .bind(book_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    bokhylle_server::acquisition::retry(&test_app.state.db, "acq-retry")
        .await
        .unwrap();

    let (status, error_code, release, key, download): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT status, error_code, selected_release_name, selected_release_key, provider_download_id
         FROM acquisitions WHERE id = 'acq-retry'",
    )
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(status, "REQUESTED");
    assert_eq!(error_code, None);
    assert_eq!(release, None);
    assert_eq!(key, None);
    assert_eq!(download, None);

    // Ready requests are not retryable.
    sqlx::query("UPDATE acquisitions SET status = 'READY' WHERE id = 'acq-retry'")
        .execute(&test_app.state.db)
        .await
        .unwrap();
    assert!(
        bokhylle_server::acquisition::retry(&test_app.state.db, "acq-retry")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn infrastructure_import_failures_do_not_blocklist() {
    let test_app = common::test_app().await;
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLINFRAW".to_string(),
            title: "Infra Book".to_string(),
            authors: vec!["Some Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO acquisitions
            (id, book_id, status, selected_release_name, selected_release_indexer, selected_release_key)
         VALUES ('acq-infra', ?, 'IMPORTING', 'Good.Release.EPUB', 'Indexer A', 'indexer a::good-guid')",
    )
    .bind(book_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    // A missing mount or path problem says nothing about the release.
    bokhylle_server::acquisition::fail_import(
        &test_app.state.db,
        "acq-infra",
        "content_missing",
        "the downloaded content could not be located",
    )
    .await
    .unwrap();

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM release_blocklist")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let blocked = bokhylle_server::acquisition::blocked_release_keys(&test_app.state.db)
        .await
        .unwrap();
    assert!(!blocked.contains("indexer a::good-guid"));
}

#[tokio::test]
async fn concurrent_shared_acquisition_registers_both_requesters() {
    let test_app = common::test_app().await;
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLRACEW".to_string(),
            title: "Race Book".to_string(),
            authors: vec!["Some Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    test_app
        .state
        .auth
        .create_user("alex", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let alex_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alex'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    test_app
        .state
        .auth
        .create_user("emma", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let emma_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'emma'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    // Both create the same request close together: whichever wins the insert,
    // both requester intents must end up on the shared acquisition.
    let (first, second) = tokio::join!(
        bokhylle_server::acquisition::create(
            &test_app.state.db,
            book_id,
            Some(alex_id),
            None,
            None,
            true,
            false,
        ),
        bokhylle_server::acquisition::create(
            &test_app.state.db,
            book_id,
            Some(emma_id),
            None,
            None,
            false,
            false,
        ),
    );
    assert!(first.is_ok(), "first create failed: {first:?}");
    assert!(second.is_ok(), "second create failed: {second:?}");

    let requesters: Vec<i64> = sqlx::query_scalar(
        "SELECT user_id FROM acquisition_requests
         WHERE acquisition_id = (SELECT id FROM acquisitions WHERE book_id = ? LIMIT 1)
         ORDER BY user_id",
    )
    .bind(book_id)
    .fetch_all(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(requesters.len(), 2, "both requesters must be registered");

    let acquisitions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM acquisitions WHERE book_id = ?")
            .bind(book_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(acquisitions, 1, "only one acquisition per book");
}

#[tokio::test]
async fn choose_file_reopens_a_failed_import_for_inspection() {
    let test_app = common::test_app().await;
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLINSPECTW".to_string(),
            title: "Inspect Book".to_string(),
            authors: vec!["Some Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO acquisitions (id, book_id, status, error_code, error_message)
         VALUES ('acq-inspect', ?, 'IMPORT_FAILED', 'no_supported_files', 'nothing usable')",
    )
    .bind(book_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    let reopened = bokhylle_server::acquisition::inspect_files(&test_app.state.db, "acq-inspect")
        .await
        .unwrap();
    assert_eq!(reopened.status.as_str(), "DOWNLOADED");
    assert_eq!(reopened.error_code, None);

    // Only failed imports can be reopened for inspection.
    assert!(
        bokhylle_server::acquisition::inspect_files(&test_app.state.db, "acq-inspect")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn reader_targets_are_email_only() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user(
            "device_user",
            "password123",
            bokhylle_server::auth::Role::User,
        )
        .await
        .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'device_user'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    let target = bokhylle_server::delivery::create_target(
        &test_app.state.db,
        user_id,
        "Kindle",
        "reader@example.com",
        "email",
        Some("kindle"),
    )
    .await
    .unwrap();
    assert_eq!(target.connector, "email");
    assert_eq!(target.kind, "kindle");

    let pocketbook = bokhylle_server::delivery::create_target(
        &test_app.state.db,
        user_id,
        "PocketBook",
        "reader@pbsync.com",
        "email",
        Some("pocketbook"),
    )
    .await
    .unwrap();
    assert_eq!(pocketbook.kind, "pocketbook");

    // There is no self-serve "this device" target: download is an action and
    // OPDS covers reader apps, so connectors must be real transports and a
    // target always needs an address.
    assert!(
        bokhylle_server::delivery::create_target(
            &test_app.state.db,
            user_id,
            "This device",
            "",
            "download",
            None,
        )
        .await
        .is_err()
    );
    assert!(
        bokhylle_server::delivery::create_target(
            &test_app.state.db,
            user_id,
            "Bogus",
            "x@example.com",
            "carrier-pigeon",
            None,
        )
        .await
        .is_err()
    );
}
