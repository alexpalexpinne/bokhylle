use sqlx::{Sqlite, SqlitePool, Transaction};

use bokhylle_core::identity::{isbn10_to_isbn13, normalize_text};
use bokhylle_metadata::MetadataResult;

use crate::error::AppError;
use crate::library::subjects;

pub async fn upsert_book_from_metadata(
    pool: &SqlitePool,
    metadata: &MetadataResult,
) -> Result<i64, AppError> {
    let title = if metadata.title.trim().is_empty() {
        "Untitled".to_string()
    } else {
        metadata.title.trim().to_string()
    };
    let normalized_title = normalize_text(&title);
    let isbn10 = metadata.isbn10.clone();
    let isbn13 = metadata
        .isbn13
        .clone()
        .or_else(|| isbn10.as_deref().and_then(isbn10_to_isbn13));

    // The upsert reads before it writes, so it must take the write lock up
    // front: a deferred transaction would fail with BUSY_SNAPSHOT when two
    // callers (e.g. two concurrent requests for the same book) overlap.
    let mut tx = pool
        .begin_with(sqlx::AssertSqlSafe("BEGIN IMMEDIATE".to_string()))
        .await?;

    let mut book_id = None;

    if let Some(key) = provider_key(metadata) {
        if let Some(found) = sqlx::query_scalar(
            "SELECT book_id FROM book_external_ids WHERE provider = ? AND provider_key = ?",
        )
        .bind(&metadata.provider)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?
        {
            book_id = Some(found);
        } else if let Some(found) = sqlx::query_scalar(
            "SELECT book_id FROM editions WHERE provider = ? AND provider_key = ? LIMIT 1",
        )
        .bind(&metadata.provider)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await?
        {
            book_id = Some(found);
        }
    }

    if book_id.is_none()
        && let Some(isbn13) = &isbn13
    {
        book_id = sqlx::query_scalar("SELECT book_id FROM editions WHERE isbn13 = ?")
            .bind(isbn13)
            .fetch_optional(&mut *tx)
            .await?;
    }

    if book_id.is_none()
        && let Some(isbn10) = &isbn10
    {
        book_id = sqlx::query_scalar("SELECT book_id FROM editions WHERE isbn10 = ?")
            .bind(isbn10)
            .fetch_optional(&mut *tx)
            .await?;
    }

    if book_id.is_none() && !normalized_title.is_empty() && !metadata.authors.is_empty() {
        book_id =
            find_book_by_title_and_author(&mut tx, &normalized_title, &metadata.authors).await?;
    }

    let book_id = match book_id {
        Some(book_id) => {
            merge_book(&mut tx, book_id, metadata).await?;
            book_id
        }
        None => create_book(&mut tx, &title, &normalized_title, metadata).await?,
    };

    let edition_id = find_edition(
        &mut tx,
        book_id,
        metadata,
        isbn10.as_deref(),
        isbn13.as_deref(),
    )
    .await?;
    match edition_id {
        Some(edition_id) => update_edition(&mut tx, edition_id, metadata, &isbn10, &isbn13).await?,
        None => {
            create_edition(&mut tx, book_id, &title, metadata, &isbn10, &isbn13).await?;
        }
    }

    if let Some(key) = provider_key(metadata) {
        crate::external_ids::link_book_tx(&mut tx, book_id, &metadata.provider, key).await?;
    }
    for language in metadata
        .languages
        .iter()
        .chain(metadata.language.iter())
        .map(|language| language.trim().to_ascii_lowercase())
        .filter(|language| !language.is_empty())
    {
        sqlx::query(
            "INSERT OR IGNORE INTO book_available_languages (book_id, language) VALUES (?, ?)",
        )
        .bind(book_id)
        .bind(language)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(book_id)
}

fn provider_key(metadata: &MetadataResult) -> Option<&str> {
    let key = metadata.provider_key.trim();
    (!key.is_empty()).then_some(key)
}

fn is_open_library_work(metadata: &MetadataResult) -> bool {
    metadata.provider == "openlibrary" && metadata.provider_key.starts_with("/works/")
}

async fn find_book_by_title_and_author(
    tx: &mut Transaction<'_, Sqlite>,
    normalized_title: &str,
    authors: &[String],
) -> Result<Option<i64>, AppError> {
    let mut candidates: Vec<i64> = Vec::new();

    for author in authors {
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
        .bind(normalized_title)
        .bind(&normalized_author)
        .fetch_all(&mut **tx)
        .await?;
        candidates.extend(ids);
    }

    candidates.sort_unstable();
    candidates.dedup();

    match candidates.as_slice() {
        [book_id] => Ok(Some(*book_id)),
        [] => Ok(None),
        multiple => {
            tracing::warn!(candidates = multiple.len(), "library.identity.ambiguous");
            Ok(None)
        }
    }
}

async fn create_book(
    tx: &mut Transaction<'_, Sqlite>,
    title: &str,
    normalized_title: &str,
    metadata: &MetadataResult,
) -> Result<i64, AppError> {
    let book_id = sqlx::query(
        "INSERT INTO books (title, normalized_title, description, language, series, series_number)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(title)
    .bind(normalized_title)
    .bind(&metadata.description)
    .bind(
        metadata
            .language
            .as_deref()
            .filter(|_| !is_open_library_work(metadata)),
    )
    .bind(&metadata.series)
    .bind(&metadata.series_number)
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();

    link_authors(tx, book_id, &metadata.authors).await?;
    link_subjects(tx, book_id, &metadata.subjects).await?;
    refresh_fts(tx, book_id).await?;
    Ok(book_id)
}

async fn merge_book(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
    metadata: &MetadataResult,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE books SET
            description = COALESCE(description, ?),
            language = COALESCE(language, ?),
            series = COALESCE(series, ?),
            series_number = CASE WHEN series_link_locked = 1 THEN series_number
                                 ELSE COALESCE(series_number, ?) END,
            updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(&metadata.description)
    .bind(
        metadata
            .language
            .as_deref()
            .filter(|_| !is_open_library_work(metadata)),
    )
    .bind(&metadata.series)
    .bind(&metadata.series_number)
    .bind(book_id)
    .execute(&mut **tx)
    .await?;

    link_authors(tx, book_id, &metadata.authors).await?;
    link_subjects(tx, book_id, &metadata.subjects).await?;
    refresh_fts(tx, book_id).await?;
    Ok(())
}

async fn link_subjects(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
    subjects_list: &[String],
) -> Result<(), AppError> {
    let offset: i64 = sqlx::query_scalar("SELECT count(*) FROM book_subjects WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&mut **tx)
        .await?;

    for (index, name) in subjects_list.iter().take(40).enumerate() {
        let name = name.trim();
        let normalized = subjects::normalized(name);
        if normalized.is_empty() {
            continue;
        }

        let subject_id: i64 =
            match sqlx::query_scalar("SELECT id FROM subjects WHERE normalized_name = ?")
                .bind(&normalized)
                .fetch_optional(&mut **tx)
                .await?
            {
                Some(id) => id,
                None => sqlx::query("INSERT INTO subjects (name, normalized_name) VALUES (?, ?)")
                    .bind(name)
                    .bind(&normalized)
                    .execute(&mut **tx)
                    .await?
                    .last_insert_rowid(),
            };

        sqlx::query(
            "INSERT OR IGNORE INTO book_subjects (book_id, subject_id, position) VALUES (?, ?, ?)",
        )
        .bind(book_id)
        .bind(subject_id)
        .bind(offset + index as i64)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

pub async fn store_subjects(
    pool: &SqlitePool,
    book_id: i64,
    subjects_list: &[String],
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    link_subjects(&mut tx, book_id, subjects_list).await?;
    tx.commit().await?;
    Ok(())
}

async fn link_authors(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
    authors: &[String],
) -> Result<(), AppError> {
    let offset: i64 = sqlx::query_scalar("SELECT count(*) FROM book_authors WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&mut **tx)
        .await?;

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
        .bind(offset + index as i64)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn refresh_fts(tx: &mut Transaction<'_, Sqlite>, book_id: i64) -> Result<(), AppError> {
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

async fn find_edition(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
    metadata: &MetadataResult,
    isbn10: Option<&str>,
    isbn13: Option<&str>,
) -> Result<Option<i64>, AppError> {
    if let Some(key) = provider_key(metadata)
        && let Some(edition_id) = sqlx::query_scalar(
            "SELECT id FROM editions WHERE provider = ? AND provider_key = ? LIMIT 1",
        )
        .bind(&metadata.provider)
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok(Some(edition_id));
    }

    if let Some(isbn13) = isbn13
        && let Some(edition_id) = sqlx::query_scalar("SELECT id FROM editions WHERE isbn13 = ?")
            .bind(isbn13)
            .fetch_optional(&mut **tx)
            .await?
    {
        return Ok(Some(edition_id));
    }

    if let Some(isbn10) = isbn10
        && let Some(edition_id) = sqlx::query_scalar("SELECT id FROM editions WHERE isbn10 = ?")
            .bind(isbn10)
            .fetch_optional(&mut **tx)
            .await?
    {
        return Ok(Some(edition_id));
    }

    sqlx::query_scalar(
        "SELECT id FROM editions WHERE book_id = ? AND is_unknown = 1 ORDER BY id LIMIT 1",
    )
    .bind(book_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(AppError::from)
}

async fn update_edition(
    tx: &mut Transaction<'_, Sqlite>,
    edition_id: i64,
    metadata: &MetadataResult,
    isbn10: &Option<String>,
    isbn13: &Option<String>,
) -> Result<(), AppError> {
    // A work result is not a verified edition. Its first ISBN, language and
    // publication year come from unrelated entries in a small editions sample.
    let work = is_open_library_work(metadata);
    sqlx::query(
        "UPDATE editions SET
            title = CASE WHEN title IS NULL OR title = '' THEN ? ELSE title END,
            language = COALESCE(language, ?),
            publication_year = COALESCE(publication_year, ?),
            publisher = COALESCE(publisher, ?),
            isbn10 = COALESCE(isbn10, ?),
            isbn13 = COALESCE(isbn13, ?),
            provider = COALESCE(provider, ?),
            provider_key = COALESCE(provider_key, ?),
            is_unknown = CASE WHEN ? THEN is_unknown ELSE 0 END,
            updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(&metadata.title)
    .bind(metadata.language.as_deref().filter(|_| !work))
    .bind((!work).then_some(metadata.year).flatten())
    .bind(&metadata.publisher)
    .bind(isbn10.as_deref().filter(|_| !work))
    .bind(isbn13.as_deref().filter(|_| !work))
    .bind(&metadata.provider)
    .bind(provider_key(metadata))
    .bind(work)
    .bind(edition_id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn create_edition(
    tx: &mut Transaction<'_, Sqlite>,
    book_id: i64,
    title: &str,
    metadata: &MetadataResult,
    isbn10: &Option<String>,
    isbn13: &Option<String>,
) -> Result<i64, AppError> {
    let work = is_open_library_work(metadata);
    let edition_id = sqlx::query(
        "INSERT INTO editions
            (book_id, title, language, publication_year, isbn10, isbn13, publisher, provider, provider_key, is_unknown)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(book_id)
    .bind(title)
    .bind(metadata.language.as_deref().filter(|_| !work))
    .bind((!work).then_some(metadata.year).flatten())
    .bind(isbn10.as_deref().filter(|_| !work))
    .bind(isbn13.as_deref().filter(|_| !work))
    .bind(&metadata.publisher)
    .bind(&metadata.provider)
    .bind(provider_key(metadata))
    .bind(work)
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();

    Ok(edition_id)
}
