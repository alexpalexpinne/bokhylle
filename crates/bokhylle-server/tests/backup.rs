use std::path::Path;

use bokhylle_server::backup;
use tower::ServiceExt;

mod common;

async fn setting(db: &sqlx::SqlitePool, key: &str, value: &str) {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)")
        .bind(key)
        .bind(value)
        .execute(db)
        .await
        .unwrap();
}

#[tokio::test]
async fn download_backup_redacts_secrets_and_keeps_metadata() {
    let test_app = common::test_app().await;
    setting(&test_app.state.db, "smtp.host", "\"smtp.example.com\"").await;
    setting(&test_app.state.db, "smtp.password", "\"hunter2\"").await;
    setting(
        &test_app.state.db,
        "integrations.qbittorrent.api_key",
        "\"qbit\"",
    )
    .await;
    let book_id: i64 = sqlx::query_scalar("INSERT INTO books (title, normalized_title) VALUES ('Backup Book', 'backup book') RETURNING id")
        .fetch_one(&test_app.state.db).await.unwrap();
    sqlx::query("INSERT INTO acquisitions (id, book_id, status, download_provider) VALUES ('backup-nzb', ?, 'QUEUED', 'sabnzbd')")
        .bind(book_id).execute(&test_app.state.db).await.unwrap();
    let private_url = "https://indexer.test/api?t=get&id=1&apikey=nzb-submission-secret";
    sqlx::query("INSERT INTO nzb_inputs (acquisition_id, url, job_name) VALUES ('backup-nzb', ?, 'bokhylle-backup-nzb')")
        .bind(private_url).execute(&test_app.state.db).await.unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = backup::create_at(&test_app.state.db, dir.path(), true, 1000)
        .await
        .unwrap();
    assert!(path.exists());

    let copy = sqlx::SqlitePool::connect(&format!("sqlite:{}", path.display()))
        .await
        .unwrap();
    let secrets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM settings WHERE key = 'smtp.password'")
            .fetch_one(&copy)
            .await
            .unwrap();
    assert_eq!(secrets, 0);
    assert!(
        !std::fs::read(&path)
            .unwrap()
            .windows(b"hunter2".len())
            .any(|window| window == b"hunter2"),
        "deleted credentials must not remain in free SQLite pages"
    );
    let api_key: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM settings WHERE key = 'integrations.qbittorrent.api_key'",
    )
    .fetch_one(&copy)
    .await
    .unwrap();
    assert_eq!(api_key, 0);
    let host: i64 = sqlx::query_scalar("SELECT count(*) FROM settings WHERE key = 'smtp.host'")
        .fetch_one(&copy)
        .await
        .unwrap();
    assert_eq!(host, 1);
    let books: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'books'",
    )
    .fetch_one(&copy)
    .await
    .unwrap();
    assert_eq!(books, 1);
    let input: (String, String) =
        sqlx::query_as("SELECT url, job_name FROM nzb_inputs WHERE acquisition_id = 'backup-nzb'")
            .fetch_one(&copy)
            .await
            .unwrap();
    assert_eq!(input, (String::new(), "bokhylle-backup-nzb".into()));
    assert!(
        !std::fs::read(&path)
            .unwrap()
            .windows(b"nzb-submission-secret".len())
            .any(|window| window == b"nzb-submission-secret")
    );
    copy.close().await;

    let scheduled = backup::create_at(&test_app.state.db, dir.path(), false, 1001)
        .await
        .unwrap();
    let copy = sqlx::SqlitePool::connect(&format!("sqlite:{}", scheduled.display()))
        .await
        .unwrap();
    let input: String =
        sqlx::query_scalar("SELECT url FROM nzb_inputs WHERE acquisition_id = 'backup-nzb'")
            .fetch_one(&copy)
            .await
            .unwrap();
    assert_eq!(input, private_url);
    copy.close().await;
}

#[tokio::test]
async fn scheduled_backup_restores_accounts_books_and_files() {
    let library_dir = tempfile::tempdir().unwrap();
    let book_path = library_dir.path().join("recover.epub");
    std::fs::write(&book_path, b"test epub content").unwrap();
    let app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    let config_dir = app._temp_dir.path().join("config");
    let cover_path = config_dir.join("artwork/covers/recover.jpg");
    std::fs::create_dir_all(cover_path.parent().unwrap()).unwrap();
    std::fs::write(&cover_path, b"test cover").unwrap();

    app.state
        .auth
        .create_user(
            "restore-admin",
            "password123",
            bokhylle_server::auth::Role::Admin,
        )
        .await
        .unwrap();
    setting(&app.state.db, "smtp.password", "\"restore-secret\"").await;
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &bokhylle_metadata::MetadataResult {
            provider: "openlibrary".to_string(),
            provider_key: "/works/recover".to_string(),
            title: "Recovered Book".to_string(),
            authors: vec!["Recovery Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE books SET cover_path = ? WHERE id = ?")
        .bind(cover_path.to_str().unwrap())
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, ?, 'epub', ?, ?)")
        .bind(edition_id)
        .bind(book_path.to_str().unwrap())
        .bind(17_i64)
        .bind("test-recovery-sha")
        .execute(&app.state.db)
        .await
        .unwrap();

    let backup_dir = config_dir.join("backups");
    let backup_path = backup::create_at(&app.state.db, &backup_dir, false, 1000)
        .await
        .unwrap();
    backup::verify(&backup_path).await.unwrap();
    app.state.db.close().await;
    for name in ["bokhylle.db", "bokhylle.db-wal", "bokhylle.db-shm"] {
        let path = config_dir.join(name);
        if path.exists() {
            std::fs::remove_file(path).unwrap();
        }
    }
    std::fs::copy(&backup_path, config_dir.join("bokhylle.db")).unwrap();

    let restored =
        common::test_app_from_existing_config(config_dir, library_dir.path().to_path_buf()).await;
    let cookie = common::login(&restored, "restore-admin", "password123").await;
    let response = restored
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/books")
                .header(axum::http::header::COOKIE, cookie)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Recovered Book"));
    assert_eq!(std::fs::read(book_path).unwrap(), b"test epub content");
    assert_eq!(std::fs::read(cover_path).unwrap(), b"test cover");
    let secret: String =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'smtp.password'")
            .fetch_one(&restored.state.db)
            .await
            .unwrap();
    assert_eq!(secret, "\"restore-secret\"");
}

#[tokio::test]
async fn verify_rejects_a_damaged_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged.db");
    std::fs::write(&path, b"not a SQLite database").unwrap();
    assert!(backup::verify(&path).await.is_err());
}

#[tokio::test]
async fn a_failed_replacement_keeps_the_previous_backup() {
    let app = common::test_app().await;
    let dir = tempfile::tempdir().unwrap();
    let first = backup::create_at(&app.state.db, dir.path(), false, 1000)
        .await
        .unwrap();
    let original = std::fs::read(&first).unwrap();
    assert!(
        backup::create_at(&app.state.db, dir.path(), false, 1000)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&first).unwrap(), original);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn scheduled_backups_keep_secrets_and_prune_to_retention() {
    let test_app = common::test_app().await;
    setting(&test_app.state.db, "smtp.password", "\"hunter2\"").await;

    let dir = tempfile::tempdir().unwrap();
    for stamp in [1000, 2000, 3000] {
        backup::create_at(&test_app.state.db, dir.path(), false, stamp)
            .await
            .unwrap();
    }

    let latest = backup::latest(dir.path()).unwrap().unwrap();
    assert_eq!(latest.0, 3000);

    backup::prune(dir.path(), 2).unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"bokhylle-2000.db".to_string()));
    assert!(names.contains(&"bokhylle-3000.db".to_string()));

    let copy = sqlx::SqlitePool::connect(&format!(
        "sqlite:{}",
        dir.path().join("bokhylle-3000.db").display()
    ))
    .await
    .unwrap();
    let secret: i64 =
        sqlx::query_scalar("SELECT count(*) FROM settings WHERE key = 'smtp.password'")
            .fetch_one(&copy)
            .await
            .unwrap();
    assert_eq!(secret, 1);
    copy.close().await;

    assert!(!Path::new(&dir.path().join("bokhylle-1000.db")).exists());
}
