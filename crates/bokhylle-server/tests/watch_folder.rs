use std::time::{Duration, SystemTime};

use bokhylle_server::{settings, watch_folder};
use serde_json::json;

mod common;

#[tokio::test]
async fn watch_status_is_admin_only_and_reports_review_files() {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use bokhylle_server::auth::Role;
    use tower::ServiceExt;

    let app = common::test_app().await;
    for (username, role) in [("admin", Role::Admin), ("member", Role::User)] {
        app.state
            .auth
            .create_user(username, "password123", role)
            .await
            .unwrap();
    }
    let review = app.state.paths.config_dir.join("ingest/review");
    std::fs::create_dir_all(&review).unwrap();
    std::fs::write(review.join("invalid.epub"), b"invalid publication").unwrap();
    for (username, expected) in [("member", StatusCode::FORBIDDEN), ("admin", StatusCode::OK)] {
        let cookie = common::login(&app, username, "password123").await;
        let response = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/maintenance/watch")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let bytes = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
            let status: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(status["enabled"], false);
            assert_eq!(status["reviewFiles"], 1);
            assert_eq!(status["pending"], 0);
            assert_eq!(status["cleanupPending"], 0);
        }
    }
}

fn age(path: &std::path::Path) {
    let modified = SystemTime::now() - Duration::from_secs(90);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

#[tokio::test]
async fn imports_settled_files_once_and_removes_duplicates() {
    let app = common::test_app().await;
    app.state
        .settings
        .set(settings::WATCH_ENABLED, &json!(true))
        .await
        .unwrap();
    let root = app.state.paths.config_dir.join("ingest");
    std::fs::create_dir_all(&root).unwrap();
    let fixture_root = tempfile::tempdir().unwrap();
    let fixture = bokhylle_library::fixtures::generate_library(fixture_root.path(), 1)
        .unwrap()
        .into_iter()
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "epub")
        })
        .unwrap();
    let first = root.join("first.epub");
    std::fs::copy(&fixture, &first).unwrap();
    age(&first);
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 1);
    assert!(!first.exists());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let duplicate = root.join("duplicate.epub");
    std::fs::copy(&fixture, &duplicate).unwrap();
    age(&duplicate);
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 0);
    assert!(!duplicate.exists());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn stale_library_duplicate_does_not_discard_input() {
    let app = common::test_app().await;
    app.state
        .settings
        .set(settings::WATCH_ENABLED, &json!(true))
        .await
        .unwrap();
    let root = app.state.paths.config_dir.join("ingest");
    std::fs::create_dir_all(&root).unwrap();
    let fixtures = tempfile::tempdir().unwrap();
    let fixture = bokhylle_library::fixtures::generate_library(fixtures.path(), 1)
        .unwrap()
        .into_iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "epub"))
        .unwrap();
    let first = root.join("first.epub");
    std::fs::copy(&fixture, &first).unwrap();
    age(&first);
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 1);
    let placed: String = sqlx::query_scalar("SELECT path FROM book_files LIMIT 1")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    std::fs::remove_file(placed).unwrap();
    let duplicate = root.join("duplicate.epub");
    std::fs::copy(&fixture, &duplicate).unwrap();
    age(&duplicate);
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 0);
    assert!(duplicate.exists());
}

#[tokio::test]
async fn recovery_finishes_journaled_placement() {
    let app = common::test_app().await;
    app.state
        .settings
        .set(settings::WATCH_ENABLED, &json!(true))
        .await
        .unwrap();
    let root = app.state.paths.config_dir.join("ingest");
    std::fs::create_dir_all(&root).unwrap();
    let fixture_root = tempfile::tempdir().unwrap();
    let fixture = bokhylle_library::fixtures::generate_library(fixture_root.path(), 1)
        .unwrap()
        .into_iter()
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "epub")
        })
        .unwrap();
    let source = root.join("book.epub");
    std::fs::copy(&fixture, &source).unwrap();
    let staging = app.state.paths.config_dir.join("staging/watch");
    std::fs::create_dir_all(&staging).unwrap();
    let staged = staging.join("recovery.epub");
    std::fs::copy(&source, &staged).unwrap();
    let target = app
        .state
        .paths
        .library_root
        .join("Recovery")
        .join("Recovery.epub");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    let digest = bokhylle_server::library::hash_file(&staged).unwrap();
    sqlx::query("INSERT INTO watch_imports (id, source_path, staged_path, target_path, sha256, status) VALUES ('recovery', ?, ?, ?, ?, 'placing')")
        .bind(source.to_string_lossy().as_ref()).bind(staged.to_string_lossy().as_ref())
        .bind(target.to_string_lossy().as_ref()).bind(digest).execute(&app.state.db).await.unwrap();
    watch_folder::recover(&app.state).await.unwrap();
    assert!(target.exists());
    let status: String =
        sqlx::query_scalar("SELECT status FROM watch_imports WHERE id = 'recovery'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(status, "imported");
    assert!(!source.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn recovery_rejects_an_existing_target_that_escapes_the_library() {
    let app = common::test_app().await;
    let outside = tempfile::tempdir().unwrap();
    let file = bokhylle_library::fixtures::generate_library(outside.path(), 1)
        .unwrap()
        .into_iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "epub"))
        .unwrap();
    let staging = app.state.paths.config_dir.join("staging/watch");
    std::fs::create_dir_all(&staging).unwrap();
    let staged = staging.join("escape.epub");
    std::fs::copy(&file, &staged).unwrap();
    let target = app.state.paths.library_root.join("escape.epub");
    std::os::unix::fs::symlink(&file, &target).unwrap();
    let digest = bokhylle_server::library::hash_file(&staged).unwrap();
    sqlx::query("INSERT INTO watch_imports (id, source_path, staged_path, target_path, sha256, status) VALUES ('escape', ?, ?, ?, ?, 'placing')")
        .bind(file.to_string_lossy().as_ref()).bind(staged.to_string_lossy().as_ref())
        .bind(target.to_string_lossy().as_ref()).bind(digest).execute(&app.state.db).await.unwrap();
    watch_folder::recover(&app.state).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM watch_imports WHERE id = 'escape'")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(status, "placing");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert!(file.exists());
    assert!(staged.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_to_outside_is_not_imported() {
    let app = common::test_app().await;
    app.state
        .settings
        .set(settings::WATCH_ENABLED, &json!(true))
        .await
        .unwrap();
    let root = app.state.paths.config_dir.join("ingest");
    std::fs::create_dir_all(&root).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("outside.epub");
    std::fs::write(&file, b"private").unwrap();
    std::os::unix::fs::symlink(&file, root.join("outside.epub")).unwrap();
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 0);
    assert!(file.exists());
}

#[tokio::test]
async fn invalid_publication_moves_to_review() {
    let app = common::test_app().await;
    app.state
        .settings
        .set(settings::WATCH_ENABLED, &json!(true))
        .await
        .unwrap();
    let root = app.state.paths.config_dir.join("ingest");
    std::fs::create_dir_all(&root).unwrap();
    let invalid = root.join("broken.pdf");
    std::fs::write(&invalid, b"not a PDF").unwrap();
    age(&invalid);
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 0);
    assert!(!invalid.exists());
    assert_eq!(std::fs::read_dir(root.join("review")).unwrap().count(), 1);
}

#[tokio::test]
async fn new_file_waits_until_it_has_settled() {
    let app = common::test_app().await;
    app.state
        .settings
        .set(settings::WATCH_ENABLED, &json!(true))
        .await
        .unwrap();
    let root = app.state.paths.config_dir.join("ingest");
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("writing.epub");
    std::fs::write(&file, b"in progress").unwrap();
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 0);
    assert!(file.exists());
}

#[tokio::test]
async fn missing_metadata_uses_original_filenames_for_every_format() {
    use std::io::Write;
    let app = common::test_app().await;
    app.state
        .settings
        .set(settings::WATCH_ENABLED, &json!(true))
        .await
        .unwrap();
    let root = app.state.paths.config_dir.join("ingest");
    std::fs::create_dir_all(&root).unwrap();
    for extension in ["epub", "cbz"] {
        let path = root.join(format!(
            "The Paper Moon {extension} - Mira Vale.{extension}"
        ));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        if extension == "epub" {
            zip.start_file("META-INF/container.xml", options).unwrap();
            zip.write_all(br#"<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#).unwrap();
            zip.start_file("content.opf", options).unwrap();
            zip.write_all(b"<package><metadata/><manifest/><spine/></package>")
                .unwrap();
        } else {
            zip.start_file("page1.png", options).unwrap();
            zip.write_all(b"\x89PNG\r\n\x1a\nfictional").unwrap();
        }
        zip.finish().unwrap();
        age(&path);
    }
    let fixture_dir = tempfile::tempdir().unwrap();
    let pdf = bokhylle_library::fixtures::generate_library(fixture_dir.path(), 4)
        .unwrap()
        .into_iter()
        .find(|path| path.extension().is_some_and(|extension| extension == "pdf"))
        .unwrap();
    let mut bytes = std::fs::read(pdf).unwrap();
    // Dictionary key replacements retain byte offsets in the PDF's xref table.
    for (key, replacement) in [
        (b"/Title".as_slice(), b"/Other".as_slice()),
        (b"/Author".as_slice(), b"/Otherx".as_slice()),
    ] {
        for index in 0..bytes.len().saturating_sub(key.len()) {
            if &bytes[index..index + key.len()] == key {
                bytes[index..index + key.len()].copy_from_slice(replacement);
            }
        }
    }
    let pdf = root.join("The Paper Moon pdf - Mira Vale.pdf");
    std::fs::write(&pdf, bytes).unwrap();
    age(&pdf);
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 3);
    let titles: Vec<String> = sqlx::query_scalar("SELECT title FROM books ORDER BY title")
        .fetch_all(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        titles,
        [
            "The Paper Moon cbz",
            "The Paper Moon epub",
            "The Paper Moon pdf"
        ]
    );
    let authors: Vec<String> = sqlx::query_scalar("SELECT name FROM authors")
        .fetch_all(&app.state.db)
        .await
        .unwrap();
    assert_eq!(authors, ["Mira Vale"]);
}

#[tokio::test]
async fn completed_journal_recovers_cleanup_even_when_watcher_is_disabled() {
    let app = common::test_app().await;
    let root = app.state.paths.config_dir.join("ingest");
    let staging = app.state.paths.config_dir.join("staging/watch");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    let source = root.join("completed.epub");
    let staged = staging.join(format!("{}.epub", uuid::Uuid::new_v4()));
    std::fs::write(&source, b"already imported").unwrap();
    std::fs::copy(&source, &staged).unwrap();
    let digest = bokhylle_server::library::hash_file(&source).unwrap();
    sqlx::query("INSERT INTO watch_imports (id, source_path, staged_path, target_path, sha256, status) VALUES ('completed', ?, ?, '', ?, 'imported')")
        .bind(source.to_string_lossy().as_ref()).bind(staged.to_string_lossy().as_ref()).bind(digest).execute(&app.state.db).await.unwrap();
    assert_eq!(watch_folder::tick(&app.state).await.unwrap(), 0);
    assert!(!source.exists());
    assert!(!staged.exists());
    let pending: i64 =
        sqlx::query_scalar("SELECT cleanup_pending FROM watch_imports WHERE id = 'completed'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(pending, 0);
}

#[tokio::test]
async fn recovery_removes_only_stale_owned_orphans() {
    let app = common::test_app().await;
    let staging = app.state.paths.config_dir.join("staging/watch");
    std::fs::create_dir_all(&staging).unwrap();
    let orphan = staging.join(format!("{}.epub", uuid::Uuid::new_v4()));
    let partial = staging.join(format!("{}.partial", uuid::Uuid::new_v4()));
    let recent = staging.join(format!("{}.epub", uuid::Uuid::new_v4()));
    let unknown = staging.join("operator-notes.txt");
    let protected = staging.join(format!("{}.epub", uuid::Uuid::new_v4()));
    for path in [&orphan, &partial, &recent, &unknown, &protected] {
        std::fs::write(path, b"retained bytes").unwrap();
    }
    for path in [&orphan, &partial, &unknown, &protected] {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(SystemTime::now() - Duration::from_secs(7200)),
            )
            .unwrap();
    }
    let outside = tempfile::tempdir().unwrap();
    sqlx::query("INSERT INTO watch_imports (id, source_path, staged_path, target_path, sha256, status) VALUES ('protected', '', ?, ?, 'unknown', 'placing')")
        .bind(protected.to_string_lossy().as_ref()).bind(outside.path().join("outside.epub").to_string_lossy().as_ref()).execute(&app.state.db).await.unwrap();
    watch_folder::recover(&app.state).await.unwrap();
    assert!(!orphan.exists());
    assert!(!partial.exists());
    assert!(recent.exists());
    assert!(unknown.exists());
    assert!(protected.exists());
}
