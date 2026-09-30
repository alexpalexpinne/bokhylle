use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqlitePool, Transaction};

use bokhylle_core::BookFormat;
use bokhylle_core::identity::{isbn10_to_isbn13, normalize_text};
use bokhylle_library::extract::{self, ExtractedMetadata};
use bokhylle_library::{Cover, ScannedFile};

use crate::AppState;
use crate::error::AppError;
use metadata_fields::{MetadataField as Field, Scope};
use serde_json::json;

pub mod import_metadata;
pub mod metadata_fields;
pub mod queries;
pub(crate) mod relevance;
pub mod scan_state;
pub mod subjects;

pub use scan_state::{ScanState, ScanStatus};

#[derive(Debug, Default, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub files_found: usize,
    pub indexed: usize,
    pub updated: usize,
    pub skipped: usize,
    pub duplicates: usize,
    pub errors: usize,
    pub duration_ms: u64,
}

struct ExtractedOrigin<'a> {
    format: &'a str,
    key: &'a str,
    filename_title: bool,
    filename_authors: bool,
}

impl ExtractedOrigin<'_> {
    fn source(&self, field: Field) -> &str {
        if (field == Field::Title && self.filename_title)
            || (field == Field::Authors && self.filename_authors)
        {
            "filename"
        } else {
            self.format
        }
    }
}

enum Outcome {
    Indexed,
    Updated,
    Skipped,
    Duplicate,
}

pub async fn index_library(state: &AppState) -> Result<ScanSummary, AppError> {
    let started = Instant::now();
    tracing::info!("library.scan.started");

    // Walking and hashing a whole library is heavy synchronous work; keep it
    // off the async runtime.
    let root = state.paths.library_root.clone();
    let files = tokio::task::spawn_blocking(move || bokhylle_library::scan(&root))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))??;
    let mut summary = ScanSummary {
        files_found: files.len(),
        ..Default::default()
    };

    for file in files {
        match index_file(state, &file, &file.path).await {
            Ok(Outcome::Indexed) => summary.indexed += 1,
            Ok(Outcome::Updated) => summary.updated += 1,
            Ok(Outcome::Skipped) => summary.skipped += 1,
            Ok(Outcome::Duplicate) => summary.duplicates += 1,
            Err(error) => {
                summary.errors += 1;
                tracing::warn!(
                    path = %file.path.display(),
                    error = %error,
                    "library.file.failed"
                );
            }
        }
    }

    // Authors can be orphaned by manual metadata fixes or re-identification;
    // drop the ones that no longer belong to any book.
    if let Err(error) = sqlx::query(
        "DELETE FROM authors
         WHERE NOT EXISTS (SELECT 1 FROM book_authors WHERE author_id = authors.id)
           AND NOT EXISTS (SELECT 1 FROM author_follows WHERE author_id = authors.id)
           AND NOT EXISTS (SELECT 1 FROM author_discoveries WHERE author_id = authors.id)",
    )
    .execute(&state.db)
    .await
    {
        tracing::warn!(%error, "library.scan.author_prune_failed");
    }

    summary.duration_ms = started.elapsed().as_millis() as u64;
    tracing::info!(
        files_found = summary.files_found,
        indexed = summary.indexed,
        errors = summary.errors,
        duration_ms = summary.duration_ms,
        "library.scan.finished"
    );

    Ok(summary)
}

/// Indexes a file that was just placed into the library by a non-scan flow
/// (the import-existing-downloads job), so its record exists before the next
/// run's duplicate checks and before the following scan.
pub async fn index_placed_file(state: &AppState, path: &Path) -> Result<(), AppError> {
    index_placed_file_with_filename(state, path, path).await
}

pub(crate) async fn index_placed_file_with_filename(
    state: &AppState,
    path: &Path,
    filename: &Path,
) -> Result<(), AppError> {
    let metadata = std::fs::metadata(path)?;
    let format = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(BookFormat::from_extension)
        .unwrap_or(BookFormat::Epub);
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64);
    let file = ScannedFile {
        path: path.to_path_buf(),
        format,
        size: metadata.len(),
        modified,
    };
    if let Outcome::Duplicate = index_file(state, &file, filename).await? {
        tracing::warn!(path = %path.display(), "library.placed.duplicate");
    }
    Ok(())
}

async fn index_file(
    state: &AppState,
    file: &ScannedFile,
    filename: &Path,
) -> Result<Outcome, AppError> {
    let path = file.path.to_string_lossy().to_string();

    let existing: Option<(String, i64, Option<i64>)> =
        sqlx::query_as("SELECT sha256, size, mtime FROM book_files WHERE path = ?")
            .bind(&path)
            .fetch_optional(&state.db)
            .await?;

    if let Some((existing_hash, existing_size, existing_mtime)) = existing {
        if existing_size == file.size as i64 && existing_mtime == file.modified {
            return Ok(Outcome::Skipped);
        }

        let digest = hash_file_blocking(file.path.clone()).await?;
        if digest == existing_hash {
            sqlx::query("UPDATE book_files SET size = ?, mtime = ?, updated_at = unixepoch() WHERE path = ?")
                .bind(file.size as i64)
                .bind(file.modified)
                .bind(&path)
                .execute(&state.db)
                .await?;
        } else {
            sqlx::query("UPDATE book_files SET size = ?, mtime = ?, sha256 = ?, updated_at = unixepoch() WHERE path = ?")
                .bind(file.size as i64)
                .bind(file.modified)
                .bind(&digest)
                .bind(&path)
                .execute(&state.db)
                .await?;
            tracing::info!(path = %path, "library.file.changed");
        }
        return Ok(Outcome::Updated);
    }

    let digest = hash_file_blocking(file.path.clone()).await?;

    let duplicate: Option<i64> = sqlx::query_scalar("SELECT id FROM book_files WHERE sha256 = ?")
        .bind(&digest)
        .fetch_optional(&state.db)
        .await?;
    if duplicate.is_some() {
        tracing::info!(path = %path, "library.file.duplicate");
        return Ok(Outcome::Duplicate);
    }

    let extracted =
        extract_blocking(file.path.clone(), file.format, filename.to_path_buf()).await?;
    let metadata = normalize_metadata(extracted.metadata, filename);
    let origin = ExtractedOrigin {
        format: file.format.as_str(),
        key: &digest,
        filename_title: metadata.title_from_filename,
        filename_authors: metadata.authors_from_filename,
    };
    let covers_dir = state.paths.config_dir.join("artwork").join("covers");

    let mut tx = state.db.begin().await?;

    let book_id = match identify_book(&mut tx, &metadata).await? {
        Some(book_id) => {
            merge_book_metadata(&mut tx, book_id, &metadata, &origin).await?;
            book_id
        }
        None => create_book(&mut tx, &metadata, &origin).await?,
    };

    let edition_id = identify_or_create_edition(&mut tx, book_id, &metadata, &origin).await?;

    let cover_path = extracted
        .cover
        .as_ref()
        .filter(|cover| bokhylle_library::covers::usable_cover(&cover.bytes))
        .and_then(|cover| write_cover(&covers_dir, cover).ok())
        .map(|path| path.to_string_lossy().into_owned());

    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256, mtime)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(edition_id)
    .bind(&path)
    .bind(file.format.as_str())
    .bind(file.size as i64)
    .bind(&digest)
    .bind(file.modified)
    .execute(&mut *tx)
    .await?;

    if let Some(cover_path) = cover_path {
        metadata_fields::automatic(
            &mut tx,
            Scope::Book(book_id),
            Field::Cover,
            json!(cover_path),
            file.format.as_str(),
            Some(&digest),
            false,
        )
        .await?;
    }

    tx.commit().await?;

    tracing::info!(path = %path, book_id, "library.file.indexed");
    Ok(Outcome::Indexed)
}

fn normalize_metadata(mut metadata: ExtractedMetadata, path: &Path) -> ExtractedMetadata {
    if metadata.title.as_deref().is_none_or(str::is_empty) {
        let (title, authors) = extract::fallback_from_filename(path);
        metadata.title_from_filename = title.is_some();
        metadata.title = title.or_else(|| Some("Untitled".to_string()));
        if metadata.authors.is_empty() {
            metadata.authors_from_filename = !authors.is_empty();
            metadata.authors = authors;
        }
    }

    metadata.language = metadata
        .language
        .as_deref()
        .and_then(bokhylle_library::extract::normalize_language);

    metadata
}

async fn identify_book(
    tx: &mut Transaction<'_, Sqlite>,
    metadata: &ExtractedMetadata,
) -> Result<Option<i64>, AppError> {
    let (isbn10, isbn13) = isbn_forms(metadata.isbn.as_deref());

    if let Some(isbn13) = &isbn13
        && let Some(book_id) = sqlx::query_scalar("SELECT book_id FROM editions WHERE isbn13 = ?")
            .bind(isbn13)
            .fetch_optional(&mut **tx)
            .await?
    {
        tracing::info!(reason = "isbn13", isbn = %isbn13, book_id, "library.identity.merged");
        return Ok(Some(book_id));
    }

    if let Some(isbn10) = &isbn10
        && let Some(book_id) = sqlx::query_scalar("SELECT book_id FROM editions WHERE isbn10 = ?")
            .bind(isbn10)
            .fetch_optional(&mut **tx)
            .await?
    {
        tracing::info!(reason = "isbn10", isbn = %isbn10, book_id, "library.identity.merged");
        return Ok(Some(book_id));
    }

    let Some(title) = metadata.title.as_deref() else {
        return Ok(None);
    };
    let normalized_title = normalize_text(title);
    if normalized_title.is_empty() || metadata.authors.is_empty() {
        return Ok(None);
    }

    let mut candidates: Vec<i64> = Vec::new();
    for author in &metadata.authors {
        let normalized_author = normalize_text(author);
        if normalized_author.is_empty() {
            continue;
        }

        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT b.id
             FROM books b
             JOIN book_authors ba ON ba.book_id = b.id
             JOIN authors a ON a.id = ba.author_id
             WHERE b.normalized_title = ? AND a.normalized_name = ?",
        )
        .bind(&normalized_title)
        .bind(&normalized_author)
        .fetch_all(&mut **tx)
        .await?;
        candidates.extend(ids);
    }

    candidates.sort_unstable();
    candidates.dedup();

    match candidates.as_slice() {
        [book_id] => {
            tracing::info!(reason = "title_author", book_id, "library.identity.merged");
            Ok(Some(*book_id))
        }
        [] => Ok(None),
        multiple => {
            tracing::warn!(candidates = multiple.len(), "library.identity.ambiguous");
            Ok(None)
        }
    }
}

async fn create_book(
    tx: &mut Transaction<'_, Sqlite>,
    metadata: &ExtractedMetadata,
    origin: &ExtractedOrigin<'_>,
) -> Result<i64, AppError> {
    let title = metadata
        .title
        .clone()
        .unwrap_or_else(|| "Untitled".to_string());
    let normalized_title = normalize_text(&title);

    let book_id = sqlx::query(
        "INSERT INTO books (title, normalized_title, description, language, series, series_number)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&title)
    .bind(&normalized_title)
    .bind(&metadata.description)
    .bind(&metadata.language)
    .bind(&metadata.series)
    .bind(&metadata.series_number)
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();

    link_authors(tx, book_id, &metadata.authors, 0).await?;
    merge_book_metadata(tx, book_id, metadata, origin).await?;

    Ok(book_id)
}

async fn merge_book_metadata(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
    metadata: &ExtractedMetadata,
    origin: &ExtractedOrigin<'_>,
) -> Result<(), AppError> {
    let unverified_work: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM editions e
            WHERE e.book_id = ? AND e.provider = 'openlibrary'
              AND e.provider_key LIKE '/works/%'
              AND NOT EXISTS (
                SELECT 1 FROM book_files f JOIN editions owned ON owned.id = f.edition_id
                WHERE owned.book_id = e.book_id
              )
        )",
    )
    .bind(book_id)
    .fetch_one(&mut **tx)
    .await?;
    for (field, value) in [
        (Field::Title, json!(metadata.title)),
        (Field::Authors, json!(metadata.authors)),
        (Field::Description, json!(metadata.description)),
        (Field::Language, json!(metadata.language)),
        (Field::Series, json!(metadata.series)),
        (Field::SeriesNumber, json!(metadata.series_number)),
    ] {
        metadata_fields::automatic(
            tx,
            Scope::Book(book_id),
            field,
            value,
            origin.source(field),
            Some(origin.key),
            field == Field::Language && unverified_work,
        )
        .await?;
    }
    refresh_fts(tx, book_id).await?;

    Ok(())
}

async fn link_authors(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
    authors: &[String],
    position_offset: i64,
) -> Result<(), AppError> {
    for (index, author) in authors.iter().enumerate() {
        let normalized = normalize_text(author);
        if normalized.is_empty() {
            continue;
        }

        let author_id: i64 =
            match sqlx::query_scalar("SELECT id FROM authors WHERE normalized_name = ?")
                .bind(&normalized)
                .fetch_optional(&mut **tx)
                .await?
            {
                Some(author_id) => author_id,
                None => sqlx::query("INSERT INTO authors (name, normalized_name) VALUES (?, ?)")
                    .bind(author)
                    .bind(&normalized)
                    .execute(&mut **tx)
                    .await?
                    .last_insert_rowid(),
            };

        sqlx::query(
            "INSERT OR IGNORE INTO book_authors (book_id, author_id, position) VALUES (?, ?, ?)",
        )
        .bind(book_id)
        .bind(author_id)
        .bind(position_offset + index as i64)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

pub(crate) async fn refresh_fts(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
) -> Result<(), AppError> {
    let (title, authors): (String, String) = sqlx::query_as(
        "SELECT b.title, COALESCE(group_concat(a.name, ', '), '')
         FROM books b
         LEFT JOIN book_authors ba ON ba.book_id = b.id
         LEFT JOIN authors a ON a.id = ba.author_id
         WHERE b.id = ?
         GROUP BY b.id",
    )
    .bind(book_id)
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query("DELETE FROM books_fts WHERE rowid = ?")
        .bind(book_id)
        .execute(&mut **tx)
        .await?;

    sqlx::query("INSERT INTO books_fts (rowid, title, author) VALUES (?, ?, ?)")
        .bind(book_id)
        .bind(&title)
        .bind(&authors)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

async fn identify_or_create_edition(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
    metadata: &ExtractedMetadata,
    origin: &ExtractedOrigin<'_>,
) -> Result<i64, AppError> {
    let (isbn10, isbn13) = isbn_forms(metadata.isbn.as_deref());

    if let Some(isbn13) = &isbn13
        && let Some(edition_id) = sqlx::query_scalar("SELECT id FROM editions WHERE isbn13 = ?")
            .bind(isbn13)
            .fetch_optional(&mut **tx)
            .await?
    {
        update_edition(tx, edition_id, metadata, origin).await?;
        return Ok(edition_id);
    }

    if let Some(isbn10) = &isbn10
        && let Some(edition_id) = sqlx::query_scalar("SELECT id FROM editions WHERE isbn10 = ?")
            .bind(isbn10)
            .fetch_optional(&mut **tx)
            .await?
    {
        update_edition(tx, edition_id, metadata, origin).await?;
        return Ok(edition_id);
    }

    if let Some(edition_id) = sqlx::query_scalar(
        "SELECT id FROM editions WHERE book_id = ? AND is_unknown = 1 ORDER BY id LIMIT 1",
    )
    .bind(book_id)
    .fetch_optional(&mut **tx)
    .await?
    {
        update_edition(tx, edition_id, metadata, origin).await?;
        return Ok(edition_id);
    }

    let title = metadata
        .title
        .clone()
        .unwrap_or_else(|| "Untitled".to_string());
    let edition_id = sqlx::query(
        "INSERT INTO editions
            (book_id, title, language, publication_year, isbn10, isbn13, publisher, is_unknown)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(book_id)
    .bind(&title)
    .bind(&metadata.language)
    .bind(metadata.year)
    .bind(&isbn10)
    .bind(&isbn13)
    .bind(&metadata.publisher)
    .bind(isbn10.is_none() && isbn13.is_none())
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();

    update_edition(tx, edition_id, metadata, origin).await?;
    Ok(edition_id)
}

async fn update_edition(
    tx: &mut Transaction<'_, Sqlite>,
    edition_id: i64,
    metadata: &ExtractedMetadata,
    origin: &ExtractedOrigin<'_>,
) -> Result<(), AppError> {
    let unverified_work: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM editions e
            WHERE e.id = ? AND e.provider = 'openlibrary'
              AND e.provider_key LIKE '/works/%'
              AND NOT EXISTS (SELECT 1 FROM book_files f WHERE f.edition_id = e.id)
        )",
    )
    .bind(edition_id)
    .fetch_one(&mut **tx)
    .await?;
    for (field, value) in [
        (Field::Title, json!(metadata.title)),
        (Field::Language, json!(metadata.language)),
        (Field::PublicationYear, json!(metadata.year)),
        (Field::Publisher, json!(metadata.publisher)),
    ] {
        metadata_fields::automatic(
            tx,
            Scope::Edition(edition_id),
            field,
            value,
            origin.source(field),
            Some(origin.key),
            unverified_work && matches!(field, Field::Language | Field::PublicationYear),
        )
        .await?;
    }

    Ok(())
}

fn isbn_forms(isbn: Option<&str>) -> (Option<String>, Option<String>) {
    match isbn {
        Some(isbn) if isbn.len() == 10 => (Some(isbn.to_string()), isbn10_to_isbn13(isbn)),
        Some(isbn) if isbn.len() == 13 => (None, Some(isbn.to_string())),
        _ => (None, None),
    }
}

/// Hashing and metadata extraction read whole files; run them on the
/// blocking pool so request handling is never stalled by a scan.
async fn hash_file_blocking(path: PathBuf) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || hash_file(&path))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))?
}

async fn extract_blocking(
    path: PathBuf,
    format: BookFormat,
    filename: PathBuf,
) -> Result<bokhylle_library::extract::Extracted, AppError> {
    tokio::task::spawn_blocking(move || extract::extract_with_filename(&path, format, &filename))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))?
        .map_err(AppError::from)
}

pub fn hash_file(path: &Path) -> Result<String, AppError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];

    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

fn write_cover(dir: &Path, cover: &Cover) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let digest = hex::encode(Sha256::digest(&cover.bytes));
    let path = dir.join(format!("{digest}.{}", cover.extension));

    if !path.exists() {
        let temporary = dir.join(format!("{digest}.tmp-{}", uuid::Uuid::new_v4()));
        if let Err(error) = std::fs::write(&temporary, &cover.bytes) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
    }

    Ok(path)
}

#[allow(clippy::too_many_arguments)]
pub async fn add_file_to_library(
    pool: &SqlitePool,
    book_id: i64,
    canonical_path: &Path,
    format: bokhylle_core::BookFormat,
    size: i64,
    sha256: &str,
    mtime: Option<i64>,
    source_path: Option<&str>,
) -> Result<i64, AppError> {
    let mut tx = pool.begin().await?;

    let edition_id: i64 = match sqlx::query_scalar(
        "SELECT e.id
         FROM editions e
         LEFT JOIN book_files f ON f.edition_id = e.id
         WHERE e.book_id = ? AND f.id IS NULL
         ORDER BY e.is_unknown DESC, e.id
         LIMIT 1",
    )
    .bind(book_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        Some(edition_id) => edition_id,
        None => {
            let title: String = sqlx::query_scalar("SELECT title FROM books WHERE id = ?")
                .bind(book_id)
                .fetch_one(&mut *tx)
                .await?;

            sqlx::query("INSERT INTO editions (book_id, title, is_unknown) VALUES (?, ?, 1)")
                .bind(book_id)
                .bind(&title)
                .execute(&mut *tx)
                .await?
                .last_insert_rowid()
        }
    };

    let file_id = sqlx::query(
        "INSERT INTO book_files
            (edition_id, path, format, size, sha256, mtime, source_path, imported_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, unixepoch())",
    )
    .bind(edition_id)
    .bind(canonical_path.to_string_lossy().to_string())
    .bind(format.as_str())
    .bind(size)
    .bind(sha256)
    .bind(mtime)
    .bind(source_path)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();

    sqlx::query("UPDATE books SET updated_at = unixepoch() WHERE id = ?")
        .bind(book_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(file_id)
}
