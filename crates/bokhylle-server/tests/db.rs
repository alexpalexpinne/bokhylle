use bokhylle_server::db;

#[tokio::test]
async fn avatar_table_keeps_bytes_off_users_and_cascades_on_account_deletion() {
    let temp_dir = tempfile::tempdir().unwrap();
    let database_path = temp_dir.path().join("bokhylle.db");
    let pool = db::init(&database_path).await.unwrap();
    let old_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pragma_table_info('users') WHERE name IN ('avatar_data', 'avatar_mime')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(old_columns, 0);

    sqlx::query("INSERT INTO users (id, username, password_hash, role) VALUES (1, 'reader', 'hash', 'user')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_avatars (user_id, data, mime) VALUES (1, ?, 'image/png')")
        .bind(b"picture".as_slice())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM user_avatars")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn released_database_reopens_without_losing_profile_and_book_data() {
    let temp_dir = tempfile::tempdir().unwrap();
    let database_path = temp_dir.path().join("bokhylle.db");
    let pool = db::init(&database_path).await.unwrap();
    sqlx::query("INSERT INTO users (id, username, password_hash, role) VALUES (1, 'reader', 'hash', 'user')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO books (id, title, normalized_title) VALUES (1, 'The Lantern Archive', 'the lantern archive')").execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO book_available_languages (book_id, language) VALUES (1, 'en'), (1, 'sv')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    let reopened = db::init(&database_path).await.unwrap();
    let profile: (String, String, i64) =
        sqlx::query_as("SELECT username, shelf_finish, can_discover FROM users WHERE id = 1")
            .fetch_one(&reopened)
            .await
            .unwrap();
    assert_eq!(profile, ("reader".into(), "oak".into(), 0));
    let languages: Vec<String> = sqlx::query_scalar(
        "SELECT language FROM book_available_languages WHERE book_id = 1 ORDER BY language",
    )
    .fetch_all(&reopened)
    .await
    .unwrap();
    assert_eq!(languages, ["en", "sv"]);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&reopened)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn migrations_apply_cleanly_and_are_idempotent() {
    let temp_dir = tempfile::tempdir().unwrap();
    let database_path = temp_dir.path().join("bokhylle.db");

    let pool = db::init(&database_path).await.unwrap();

    let settings_table: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'settings'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(settings_table, 1);

    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(foreign_keys, 1);

    pool.close().await;

    let pool = db::init(&database_path).await.unwrap();
    let applied_migrations: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();

    let migrations_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let expected_migrations = std::fs::read_dir(migrations_dir)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .map(|entry| entry.path().extension().is_some_and(|ext| ext == "sql"))
                .unwrap_or(false)
        })
        .count() as i64;

    assert_eq!(applied_migrations, expected_migrations);
}

#[tokio::test]
async fn migrations_leave_referential_integrity_intact() {
    let temp_dir = tempfile::tempdir().unwrap();
    let database_path = temp_dir.path().join("bokhylle.db");
    let pool = db::init(&database_path).await.unwrap();

    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(integrity, "ok");

    // Rebuild-heavy migrations (table copies) must not orphan any rows.
    let violations: Vec<(String, i64, String, i64)> = sqlx::query_as("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        violations.is_empty(),
        "foreign key violations after migrations: {violations:?}"
    );
}
