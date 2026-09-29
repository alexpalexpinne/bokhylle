use std::path::{Path, PathBuf};
use std::sync::Arc;

use bokhylle_acquisition::qbittorrent::TorrentInfo;
use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;

mod common;

// The job status is process-wide, so these tests run one at a time and assert
// deltas rather than absolute counters.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn torrent(hash: &str, name: &str, content: &Path, tags: Option<&str>) -> TorrentInfo {
    TorrentInfo {
        hash: hash.to_string(),
        name: name.to_string(),
        state: "uploading".to_string(),
        progress: 1.0,
        size: 1000,
        downloaded: 1000,
        save_path: "/data/torrents/books".to_string(),
        content_path: Some(content.to_string_lossy().into_owned()),
        category: Some("books".to_string()),
        tags: tags.map(str::to_string),
        eta: None,
        dl_speed: None,
    }
}

fn find_epubs(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(find_epubs(&path));
        } else if path
            .extension()
            .is_some_and(|extension| extension == "epub")
        {
            found.push(path);
        }
    }
    found
}

fn find_pdfs(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(find_pdfs(&path));
        } else if path.extension().is_some_and(|extension| extension == "pdf") {
            found.push(path);
        }
    }
    found
}

fn find_fixture(paths: &[PathBuf]) -> PathBuf {
    paths
        .iter()
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "epub")
        })
        .cloned()
        .expect("fixture has an epub")
}

#[tokio::test]
async fn imports_untracked_downloads_and_skips_tracked_or_tagged() {
    let _serial = SERIAL.lock().await;
    let baseline = bokhylle_server::adopt::status();

    let download = Arc::new(FakeDownloadProvider::default());
    let library_dir = tempfile::tempdir().unwrap();
    let test_app = common::test_app_full(
        library_dir.path().to_path_buf(),
        Arc::new(FakeMetadataProvider::new(vec![])),
        Arc::new(FakeIndexerProvider::default()),
        download.clone(),
    )
    .await;
    // Imports are only trusted from the downloads directory.
    let written =
        bokhylle_library::fixtures::generate_library(&test_app.state.paths.downloads_dir, 1)
            .unwrap();
    let epub = find_fixture(&written);

    let tracked_book = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLTRACKEDW".to_string(),
            title: "Tracked Book".to_string(),
            authors: vec!["Tracked Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO acquisitions (id, book_id, status, provider_download_id)
         VALUES ('acq-tracked', ?, 'READY', 'cccccccccccccccccccccccccccccccccccccccc')",
    )
    .bind(tracked_book)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    let mut still_downloading = torrent(
        "dddddddddddddddddddddddddddddddddddddddd",
        "Still Downloading",
        &epub,
        None,
    );
    still_downloading.progress = 0.4;

    download.set_category_torrents(vec![
        torrent(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "Manual Book",
            &epub,
            None,
        ),
        torrent(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "Tagged Book",
            &epub,
            Some("books-acquisition-acq-1"),
        ),
        torrent(
            "cccccccccccccccccccccccccccccccccccccccc",
            "Tracked Book",
            &epub,
            None,
        ),
        still_downloading,
    ]);

    bokhylle_server::adopt::run(&test_app.state).await.unwrap();
    let status = bokhylle_server::adopt::status();
    assert_eq!(status.imported - baseline.imported, 1);
    assert_eq!(status.already - baseline.already, 0);
    assert_eq!(status.skipped - baseline.skipped, 3);
    assert_eq!(status.failed - baseline.failed, 0);

    let placed = find_epubs(library_dir.path());
    assert_eq!(placed.len(), 1);
    assert_eq!(
        bokhylle_server::library::hash_file(&placed[0]).unwrap(),
        bokhylle_server::library::hash_file(&epub).unwrap()
    );

    // run() triggers a scan; wait for it so the adopted file is indexed.
    for _ in 0..400 {
        if !test_app.state.scan_state.status().running {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT path, sha256 FROM book_files")
        .fetch_all(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "book_files: {rows:?}");
    let catalogued: i64 = sqlx::query_scalar("SELECT count(*) FROM books WHERE id != ?")
        .bind(tracked_book)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(catalogued, 1);

    // A second run sees the digest in the library and reports it as already
    // present instead of importing a duplicate file.
    let after_first = bokhylle_server::adopt::status();
    bokhylle_server::adopt::run(&test_app.state).await.unwrap();
    let status = bokhylle_server::adopt::status();
    assert_eq!(status.imported - after_first.imported, 0);
    assert_eq!(status.already - after_first.already, 1);
    assert_eq!(status.skipped - after_first.skipped, 3);
    assert_eq!(find_epubs(library_dir.path()).len(), 1);
}

#[tokio::test]
async fn collection_torrent_imports_every_missing_book() {
    let _serial = SERIAL.lock().await;
    let baseline = bokhylle_server::adopt::status();
    let source = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(source.path(), 3).unwrap();

    let download = Arc::new(FakeDownloadProvider::default());
    let library_dir = tempfile::tempdir().unwrap();
    let test_app = common::test_app_full_with_downloads(
        library_dir.path().to_path_buf(),
        source.path().to_path_buf(),
        Arc::new(FakeMetadataProvider::new(vec![])),
        Arc::new(FakeIndexerProvider::default()),
        download.clone(),
    )
    .await;

    download.set_category_torrents(vec![torrent(
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "NPR Top 100 SF Collection",
        source.path(),
        None,
    )]);

    bokhylle_server::adopt::run(&test_app.state).await.unwrap();
    let status = bokhylle_server::adopt::status();
    let imported = status.imported - baseline.imported;
    // Three generated fixtures plus the metadata-less "Mystery Book"; the
    // duplicate copy and the broken epub are not separate books.
    assert_eq!(imported, 4, "{status:?}");
    assert_eq!(status.already - baseline.already, 0, "{status:?}");
    assert_eq!(status.skipped - baseline.skipped, 0);
    assert_eq!(status.failed - baseline.failed, 0);

    let mut placed = find_epubs(library_dir.path());
    placed.extend(find_pdfs(library_dir.path()));
    assert_eq!(placed.len() as u64, imported, "{placed:?}");

    // Re-running the same collection imports nothing new.
    let after_first = bokhylle_server::adopt::status();
    bokhylle_server::adopt::run(&test_app.state).await.unwrap();
    let status = bokhylle_server::adopt::status();
    assert_eq!(status.imported - after_first.imported, 0);
    assert_eq!(status.already - after_first.already, imported);
    assert_eq!(status.skipped - after_first.skipped, 0);
}

#[tokio::test]
async fn catalog_dedupe_skips_books_seen_under_another_file() {
    let _serial = SERIAL.lock().await;
    let baseline = bokhylle_server::adopt::status();

    let download = Arc::new(FakeDownloadProvider::default());
    let library_dir = tempfile::tempdir().unwrap();
    let test_app = common::test_app_full(
        library_dir.path().to_path_buf(),
        Arc::new(FakeMetadataProvider::new(vec![])),
        Arc::new(FakeIndexerProvider::default()),
        download.clone(),
    )
    .await;
    let written =
        bokhylle_library::fixtures::generate_library(&test_app.state.paths.downloads_dir, 1)
            .unwrap();
    let epub = find_fixture(&written);

    // The library already has the fixture book, but from a different file:
    // another digest, same title and author.
    let existing = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: "/works/OLEXISTINGW".to_string(),
            title: "Project Hail Mary".to_string(),
            authors: vec!["Andy Weir".to_string()],
            isbn13: Some("9780593135204".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(existing)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, '/library/Andy Weir/Project Hail Mary.epub', 'epub', 10, 'other-build-digest')",
    )
    .bind(edition_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    download.set_category_torrents(vec![torrent(
        "ffffffffffffffffffffffffffffffffffffffff",
        "Project Hail Mary",
        &epub,
        None,
    )]);

    bokhylle_server::adopt::run(&test_app.state).await.unwrap();
    let status = bokhylle_server::adopt::status();
    assert_eq!(status.imported - baseline.imported, 0, "{status:?}");
    assert_eq!(status.already - baseline.already, 1);
    assert_eq!(status.skipped - baseline.skipped, 0);
    assert_eq!(find_epubs(library_dir.path()).len(), 0);
}

#[tokio::test]
async fn downloads_outside_the_downloads_directory_are_refused() {
    let _serial = SERIAL.lock().await;
    let baseline = bokhylle_server::adopt::status();
    let source = tempfile::tempdir().unwrap();
    let written = bokhylle_library::fixtures::generate_library(source.path(), 1).unwrap();
    let epub = find_fixture(&written);

    // The app's downloads directory is elsewhere, so this path is out of bounds.
    let download = Arc::new(FakeDownloadProvider::default());
    let library_dir = tempfile::tempdir().unwrap();
    let test_app = common::test_app_full(
        library_dir.path().to_path_buf(),
        Arc::new(FakeMetadataProvider::new(vec![])),
        Arc::new(FakeIndexerProvider::default()),
        download.clone(),
    )
    .await;

    download.set_category_torrents(vec![torrent(
        "ffffffffffffffffffffffffffffffffffffffff",
        "Outside",
        &epub,
        None,
    )]);

    bokhylle_server::adopt::run(&test_app.state).await.unwrap();
    let status = bokhylle_server::adopt::status();
    assert_eq!(
        status.failed - baseline.failed,
        1,
        "an out-of-root content path must not be imported: {status:?}"
    );
    assert_eq!(status.imported - baseline.imported, 0);
}
