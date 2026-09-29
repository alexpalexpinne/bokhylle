use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use sqlx::SqlitePool;

use bokhylle_core::BookFormat;
use bokhylle_library::covers::usable_cover;
use bokhylle_library::extract::title_like_author;
use bokhylle_metadata::MetadataQuery;

use crate::AppState;
use crate::error::AppError;

#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobFailure {
    pub book_id: i64,
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetadataJobStatus {
    pub running: bool,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub cancelled: bool,
    pub books_enriched: u64,
    pub books_failed: u64,
    pub failures: Vec<JobFailure>,
    pub error: Option<String>,
}

const MAX_REPORTED_FAILURES: usize = 50;

#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageJobStatus {
    pub running: bool,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub authors_resolved: u64,
    pub authors_failed: u64,
    pub covers_fetched: u64,
    pub covers_failed: u64,
    pub books_repaired: u64,
    pub error: Option<String>,
}

static STATUS: OnceLock<Mutex<ImageJobStatus>> = OnceLock::new();
static METADATA_STATUS: OnceLock<Mutex<MetadataJobStatus>> = OnceLock::new();
static METADATA_CANCEL: AtomicBool = AtomicBool::new(false);

fn status_lock() -> &'static Mutex<ImageJobStatus> {
    STATUS.get_or_init(|| Mutex::new(ImageJobStatus::default()))
}

pub fn status() -> ImageJobStatus {
    status_lock().lock().expect("image job lock").clone()
}

fn update(f: impl FnOnce(&mut ImageJobStatus)) {
    let mut status = status_lock().lock().expect("image job lock");
    f(&mut status);
}

pub fn start(state: AppState) -> bool {
    {
        let mut status = status_lock().lock().expect("image job lock");
        if status.running {
            return false;
        }
        *status = ImageJobStatus {
            running: true,
            started_at: Some(now_epoch()),
            ..Default::default()
        };
    }

    tokio::spawn(async move {
        let result = run(&state).await;
        update(|status| {
            status.running = false;
            status.finished_at = Some(now_epoch());
            status.error = result.err().map(|error| error.to_string());
        });
    });

    true
}

fn metadata_lock() -> &'static Mutex<MetadataJobStatus> {
    METADATA_STATUS.get_or_init(|| Mutex::new(MetadataJobStatus::default()))
}

pub fn metadata_status() -> MetadataJobStatus {
    metadata_lock().lock().expect("metadata job lock").clone()
}

fn update_metadata(f: impl FnOnce(&mut MetadataJobStatus)) {
    let mut status = metadata_lock().lock().expect("metadata job lock");
    f(&mut status);
}

pub fn cancel_metadata() -> bool {
    if !metadata_status().running {
        return false;
    }
    METADATA_CANCEL.store(true, Ordering::SeqCst);
    true
}

pub fn start_metadata(state: AppState, force: bool) -> bool {
    {
        let mut status = metadata_lock().lock().expect("metadata job lock");
        if status.running {
            return false;
        }
        METADATA_CANCEL.store(false, Ordering::SeqCst);
        *status = MetadataJobStatus {
            running: true,
            started_at: Some(now_epoch()),
            ..Default::default()
        };
    }

    tokio::spawn(async move {
        let result = run_metadata(&state, force).await;
        update_metadata(|status| {
            status.running = false;
            status.finished_at = Some(now_epoch());
            status.error = result.err().map(|error| error.to_string());
        });
    });

    true
}

type MetadataCandidate = (
    i64,
    String,
    Option<String>,
    Option<String>,
    i64,
    Option<i64>,
    Option<String>,
);

pub async fn run_metadata(state: &AppState, force: bool) -> Result<(), AppError> {
    // Books that still lack subjects, a description, series information or a
    // rating check, and that enrichment has not already exhausted.
    let books: Vec<MetadataCandidate> = sqlx::query_as(
        "SELECT b.id, b.title, b.description, b.series,
                (SELECT count(*) FROM book_subjects bs WHERE bs.book_id = b.id),
                b.rating_checked_at,
                b.cover_path
         FROM books b
         WHERE EXISTS (
               SELECT 1 FROM book_files f
               JOIN editions e ON e.id = f.edition_id
               WHERE e.book_id = b.id
           )
           AND (
               NOT EXISTS (SELECT 1 FROM book_subjects bs WHERE bs.book_id = b.id)
               OR b.description IS NULL
               OR b.series IS NULL
               OR b.rating_checked_at IS NULL
           )
           AND (? = 1 OR b.metadata_checked_at IS NULL OR b.rating_checked_at IS NULL)
         ORDER BY b.id",
    )
    .bind(force)
    .fetch_all(&state.db)
    .await?;

    for (id, title, description, series, subject_count, rating_checked_at, cover_path) in books {
        if METADATA_CANCEL.load(Ordering::SeqCst) {
            update_metadata(|status| status.cancelled = true);
            break;
        }

        let mut metadata: Option<bokhylle_metadata::MetadataResult> = None;
        let mut reason = "no provider key or ISBN";
        let fields_only = subject_count > 0 && (!description.is_none() || !series.is_none());

        let isbns: Vec<String> = sqlx::query_scalar(
            "SELECT COALESCE(isbn13, isbn10) FROM editions
             WHERE book_id = ? AND (isbn13 IS NOT NULL OR isbn10 IS NOT NULL)",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let authors: Vec<String> = sqlx::query_scalar(
            "SELECT a.name FROM book_authors ba
             JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = ?
             ORDER BY ba.position, a.name",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let keys: Vec<(String, String)> = sqlx::query_as(
            "SELECT provider, provider_key FROM editions
             WHERE book_id = ? AND provider_key IS NOT NULL AND provider_key != ''
             ORDER BY id LIMIT 3",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        for (provider, key) in keys {
            // A stored key belongs to its provider; never ask another one.
            if provider != state.metadata.name() {
                continue;
            }
            reason = "no metadata found";
            match state.metadata.get_book(&key).await {
                Ok(Some(found)) => {
                    metadata = Some(found);
                    break;
                }
                Err(_) => reason = "request failed",
                _ => {}
            }
        }

        if metadata.is_none() {
            for isbn in &isbns {
                reason = "no metadata found";
                match state
                    .metadata
                    .search(&MetadataQuery {
                        isbn: Some(isbn.clone()),
                        limit: 1,
                        ..Default::default()
                    })
                    .await
                {
                    Ok(results) => {
                        if let Some(first) = results.into_iter().next() {
                            metadata = Some(first);
                            break;
                        }
                    }
                    Err(_) => reason = "request failed",
                }
            }
        }

        if metadata.is_none() {
            // Last resort for scanned books without a key or ISBN: search by
            // title and first author, accepting only a plausible title match.
            if !title.trim().is_empty() {
                reason = "no metadata found";
                match state
                    .metadata
                    .search(&MetadataQuery {
                        title: Some(title.clone()),
                        author: authors.first().cloned(),
                        limit: 3,
                        ..Default::default()
                    })
                    .await
                {
                    Ok(results) => {
                        if let Some(found) = results
                            .into_iter()
                            .find(|candidate| titles_confidently_match(&title, &candidate.title))
                        {
                            metadata = Some(found);
                        }
                    }
                    Err(_) => reason = "request failed",
                }
            }
        }

        // Conservative fallback: only when the primary provider could not
        // identify the book, has no description, or has no cover, fill from
        // the other provider (identity stays with the primary; ratings have
        // their own provider and provenance; an existing cover is never
        // replaced).
        let mut cover_saved = false;
        if (metadata.is_none()
            || metadata
                .as_ref()
                .is_some_and(|found| found.description.is_none())
            || cover_path.is_none())
            && let Some(fallback_provider) = state.metadata_fallback.as_ref()
        {
            let mut fallback_result = None;
            for isbn in &isbns {
                match fallback_provider
                    .search(&MetadataQuery {
                        isbn: Some(isbn.clone()),
                        limit: 1,
                        ..Default::default()
                    })
                    .await
                {
                    Ok(candidates) => {
                        // ISBN alone can point at a different-language or
                        // different-work edition; require a title match too.
                        if let Some(candidate) = candidates
                            .into_iter()
                            .find(|candidate| titles_confidently_match(&title, &candidate.title))
                        {
                            fallback_result = Some(candidate);
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if fallback_result.is_none()
                && !title.trim().is_empty()
                && let Ok(candidates) = fallback_provider
                    .search(&MetadataQuery {
                        title: Some(title.clone()),
                        author: authors.first().cloned(),
                        limit: 3,
                        ..Default::default()
                    })
                    .await
            {
                fallback_result = candidates
                    .into_iter()
                    .find(|candidate| titles_confidently_match(&title, &candidate.title));
            }

            if let Some(found) = fallback_result {
                if cover_path.is_none()
                    && let Some(cover_id) = found.cover_id.clone()
                    && let Ok(Some(bytes)) = fallback_provider.fetch_cover(&cover_id).await
                    && bytes.len() >= 2048
                {
                    // Small thumbnails are not a usable cover.
                    let covers_dir = state.paths.config_dir.join("artwork").join("covers");
                    let path = covers_dir.join(format!("{}-{id}.jpg", fallback_provider.name()));
                    cover_saved = write_cover(&covers_dir, &path, &bytes).await
                        && set_cover_path(state, id, &path).await;
                }

                match metadata.as_mut() {
                    Some(existing) => {
                        if existing.description.is_none() {
                            existing.description = found.description;
                        }
                        if existing.subjects.is_empty() {
                            existing.subjects = found.subjects;
                        }
                    }
                    None => metadata = Some(found),
                }
            }
        }

        let mut stored_subjects = false;
        let mut enriched_fields = false;

        if let Some(found) = &metadata {
            if !found.subjects.is_empty() && subject_count == 0 {
                stored_subjects =
                    crate::library::import_metadata::store_subjects(&state.db, id, &found.subjects)
                        .await
                        .is_ok();
            }

            // Only missing fields are filled: existing local metadata wins.
            if (description.is_none() && found.description.is_some())
                || (series.is_none() && found.series.is_some())
            {
                enriched_fields = sqlx::query(
                    "UPDATE books SET
                         description = COALESCE(description, ?),
                         series = COALESCE(series, ?),
                         series_number = CASE WHEN series_link_locked = 1 THEN series_number
                                              ELSE COALESCE(series_number, ?) END,
                         updated_at = unixepoch()
                     WHERE id = ?",
                )
                .bind(&found.description)
                .bind(&found.series)
                .bind(&found.series_number)
                .bind(id)
                .execute(&state.db)
                .await
                .is_ok();
            }
        }

        // Keep the matched provider key so later runs are keyed lookups.
        if let Some(found) = &metadata
            && !found.provider_key.trim().is_empty()
        {
            let _ = sqlx::query(
                "UPDATE editions SET provider = ?, provider_key = ?
                 WHERE book_id = ? AND (provider_key IS NULL OR provider_key = '')",
            )
            .bind(&found.provider)
            .bind(&found.provider_key)
            .bind(id)
            .execute(&state.db)
            .await;
        }

        let mut rating_updated = false;
        let mut rating_ok = true;
        let ratings_provider = state.ratings.name();
        if ratings_provider != "disabled"
            && (force || rating_checked_at.is_none())
            && let Some(found) = &metadata
        {
            let (rating, source_key) = if found.provider == ratings_provider {
                let primary = match state.ratings.fetch_ratings(&found.provider_key).await {
                    Ok(value) => value,
                    Err(_) => {
                        rating_ok = false;
                        None
                    }
                };
                match primary {
                    Some((average, count)) => {
                        (Some((average, count)), Some(found.provider_key.clone()))
                    }
                    None => match ratings_fallback(state, &title, &authors, &isbns).await {
                        Some((average, count, key)) => (Some((average, count)), Some(key)),
                        None => (None, None),
                    },
                }
            } else {
                // A different ratings provider matches by ISBN instead, so
                // the score keeps its own provenance.
                let mut found_rating = None;
                for isbn in &isbns {
                    let candidates = match state
                        .ratings
                        .search(&MetadataQuery {
                            isbn: Some(isbn.clone()),
                            limit: 1,
                            ..Default::default()
                        })
                        .await
                    {
                        Ok(candidates) => candidates,
                        Err(_) => {
                            rating_ok = false;
                            continue;
                        }
                    };
                    let candidate = candidates.into_iter().find(|candidate| {
                        titles_confidently_match(&title, &candidate.title)
                            && candidate.authors.iter().any(|candidate_author| {
                                authors
                                    .iter()
                                    .any(|author| candidate_author.eq_ignore_ascii_case(author))
                            })
                    });
                    if let Some(candidate) = candidate
                        && let Ok(Some((average, count))) =
                            state.ratings.fetch_ratings(&candidate.provider_key).await
                    {
                        found_rating = Some((average, count, candidate.provider_key));
                        break;
                    }
                }
                match found_rating {
                    Some((average, count, key)) => (Some((average, count)), Some(key)),
                    None => (None, None),
                }
            };

            if let Some((average, count)) = rating {
                rating_updated = sqlx::query(
                    "UPDATE books SET rating = ?, rating_count = ?, rating_source = ?,
                         rating_source_key = ?, updated_at = unixepoch()
                     WHERE id = ?",
                )
                .bind(average)
                .bind(count)
                .bind(ratings_provider)
                .bind(&source_key)
                .bind(id)
                .execute(&state.db)
                .await
                .is_ok();
            }
        }

        // A definitive outcome is recorded so the book is not retried on
        // every run; transient request failures stay retryable. The ratings
        // phase has its own marker so a resolved "no ratings" lookup is not
        // re-searched while a transient failure still is.
        if reason != "request failed" {
            let _ = sqlx::query("UPDATE books SET metadata_checked_at = unixepoch() WHERE id = ?")
                .bind(id)
                .execute(&state.db)
                .await;
        }
        if reason != "request failed" && rating_ok {
            let _ = sqlx::query("UPDATE books SET rating_checked_at = unixepoch() WHERE id = ?")
                .bind(id)
                .execute(&state.db)
                .await;
        }

        if stored_subjects || enriched_fields || rating_updated || cover_saved {
            update_metadata(|status| status.books_enriched += 1);
        } else {
            // Only call it "nothing new" when metadata was actually found and
            // nothing needed writing; otherwise report the real failure.
            let reason = if metadata.is_some() && fields_only {
                "nothing new found"
            } else {
                reason
            };
            update_metadata(|status| {
                status.books_failed += 1;
                if status.failures.len() < MAX_REPORTED_FAILURES {
                    status.failures.push(JobFailure {
                        book_id: id,
                        title: title.clone(),
                        reason: reason.to_string(),
                    });
                }
            });
        }
    }

    Ok(())
}

/// Match titles that share a work even when one carries a series prefix or
/// subtitle ("Dune: House Atreides" vs "House Atreides"): parentheticals are
/// dropped, punctuation flattened, and the shorter normalized title must be
/// at least 8 characters and contained in the longer one.
fn match_title(title: &str) -> String {
    let without_parentheticals = title.split(['(', '[']).next().unwrap_or(title);
    without_parentheticals
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn titles_confidently_match(left: &str, right: &str) -> bool {
    let left = match_title(left);
    let right = match_title(right);
    let (short, long) = if left.len() <= right.len() {
        (&left, &right)
    } else {
        (&right, &left)
    };
    short.len() >= 8 && long.contains(short.as_str())
}

/// Conservative duplicate-work ratings fallback: only exact normalized
/// titles with an author match qualify, ISBN overlap is a confidence
/// signal, ties are treated as ambiguous, and the canonical metadata key
/// is never overwritten (the rating keeps its own source key).
async fn ratings_fallback(
    state: &AppState,
    title: &str,
    authors: &[String],
    isbns: &[String],
) -> Option<(f64, i64, String)> {
    if match_title(title).len() < 4 {
        return None;
    }

    // Search by title only: Open Library's author-filtered search drops
    // valid duplicate works, and the author check below is stricter anyway.
    let results = state
        .metadata
        .search(&MetadataQuery {
            title: Some(title.to_string()),
            limit: 8,
            ..Default::default()
        })
        .await
        .ok()?;

    let mut matches: Vec<(f64, i64, String, bool)> = Vec::new();
    for candidate in results {
        if !titles_confidently_match(title, &candidate.title) {
            continue;
        }
        let author_ok = candidate.authors.iter().any(|candidate_author| {
            authors
                .iter()
                .any(|book_author| candidate_author.eq_ignore_ascii_case(book_author))
        });
        if !author_ok {
            continue;
        }
        let isbn_overlap = [candidate.isbn13.as_ref(), candidate.isbn10.as_ref()]
            .into_iter()
            .flatten()
            .any(|candidate_isbn| isbns.iter().any(|isbn| isbn == candidate_isbn));
        if let Ok(Some((average, count))) =
            state.ratings.fetch_ratings(&candidate.provider_key).await
        {
            matches.push((average, count, candidate.provider_key, isbn_overlap));
        }
    }

    if matches.is_empty() {
        return None;
    }
    // Rating count decides; ISBN overlap only breaks count ties, and equal
    // counts with equal confidence are treated as ambiguous.
    matches.sort_by(|a, b| b.1.cmp(&a.1).then(b.3.cmp(&a.3)));
    if matches.len() > 1 && matches[0].1 == matches[1].1 {
        return None;
    }
    let (average, count, key, _) = matches.remove(0);
    Some((average, count, key))
}

pub async fn run(state: &AppState) -> Result<(), AppError> {
    resolve_author_ids(state).await;
    backfill_covers(state).await;
    repair_title_like_authors(state).await;
    prune_orphan_authors(&state.db).await?;
    Ok(())
}

async fn resolve_author_ids(state: &AppState) {
    let authors: Vec<(i64, String)> = match sqlx::query_as(
        "SELECT id, name FROM authors a
         WHERE a.olid IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM author_external_ids x
               WHERE x.author_id = a.id AND x.provider = 'openlibrary'
           )",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return,
    };

    for (id, name) in authors {
        match state.metadata.resolve_author_olid(&name).await {
            Ok(Some(olid)) => {
                let updated = sqlx::query("UPDATE authors SET olid = ? WHERE id = ?")
                    .bind(&olid)
                    .bind(id)
                    .execute(&state.db)
                    .await;
                if updated.is_ok()
                    && crate::external_ids::link_author(&state.db, id, "openlibrary", &olid)
                        .await
                        .is_ok()
                {
                    update(|status| status.authors_resolved += 1);
                } else {
                    update(|status| status.authors_failed += 1);
                }
            }
            _ => update(|status| status.authors_failed += 1),
        }
    }
}

async fn backfill_covers(state: &AppState) {
    let books: Vec<(i64, String)> =
        match sqlx::query_as("SELECT id, title FROM books WHERE cover_path IS NULL")
            .fetch_all(&state.db)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return,
        };

    let covers_dir = state.paths.config_dir.join("artwork").join("covers");

    for (id, title) in books {
        let isbns: Vec<String> = sqlx::query_scalar(
            "SELECT COALESCE(isbn13, isbn10) FROM editions
             WHERE book_id = ? AND (isbn13 IS NOT NULL OR isbn10 IS NOT NULL)",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let authors: Vec<String> = sqlx::query_scalar(
            "SELECT a.name FROM book_authors ba JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = ? ORDER BY ba.position, a.name",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

        let mut saved = false;

        for isbn in &isbns {
            if let Ok(Some(bytes)) = state.metadata.fetch_cover_by_isbn(isbn).await
                && usable_cover(&bytes)
            {
                let path = covers_dir.join(format!("ol-isbn-{isbn}.jpg"));
                if write_cover(&covers_dir, &path, &bytes).await
                    && set_cover_path(state, id, &path).await
                {
                    saved = true;
                    break;
                }
            }
        }

        if !saved
            && let Ok(results) = state
                .metadata
                .search(&MetadataQuery {
                    title: Some(title.clone()),
                    author: authors.first().cloned(),
                    isbn: None,
                    free_text: None,
                    limit: 5,
                    continuation: None,
                })
                .await
        {
            for hit in results {
                let Some(cover_id) = hit.cover_id else {
                    continue;
                };
                if !titles_match(&hit.title, &title) {
                    continue;
                }
                if let Ok(Some(bytes)) = state.metadata.fetch_cover(&cover_id).await
                    && usable_cover(&bytes)
                {
                    let path = covers_dir.join(format!("ol-{cover_id}.jpg"));
                    if write_cover(&covers_dir, &path, &bytes).await
                        && set_cover_path(state, id, &path).await
                    {
                        saved = true;
                        break;
                    }
                }
            }
        }

        update(|status| {
            if saved {
                status.covers_fetched += 1;
            } else {
                status.covers_failed += 1;
            }
        });
    }
}

async fn repair_title_like_authors(state: &AppState) {
    let files: Vec<(i64, String, String, String)> = match sqlx::query_as(
        "SELECT b.id, b.title, f.path, f.format
         FROM books b
         JOIN editions e ON e.book_id = b.id
         JOIN book_files f ON f.edition_id = e.id
         ORDER BY b.id, f.id",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(_) => return,
    };

    let mut authors_by_book: HashMap<i64, Vec<String>> = HashMap::new();
    if let Ok(rows) = sqlx::query_as::<_, (i64, String)>(
        "SELECT ba.book_id, a.name FROM book_authors ba JOIN authors a ON a.id = ba.author_id",
    )
    .fetch_all(&state.db)
    .await
    {
        for (book_id, name) in rows {
            authors_by_book.entry(book_id).or_default().push(name);
        }
    }

    let mut seen: Vec<i64> = Vec::new();

    for (book_id, title, path, format) in files {
        if seen.contains(&book_id) {
            continue;
        }
        seen.push(book_id);

        let Some(authors) = authors_by_book.get(&book_id) else {
            continue;
        };
        if !authors
            .iter()
            .any(|author| title_like_author(&title, author))
        {
            continue;
        }

        let book_format = BookFormat::from_extension(&format).unwrap_or(BookFormat::Epub);
        let Ok(extracted) = bokhylle_library::extract::extract(Path::new(&path), book_format)
        else {
            continue;
        };
        let new_authors = extracted.metadata.authors;
        if new_authors.is_empty() {
            continue;
        }

        if replace_authors(&state.db, book_id, &new_authors)
            .await
            .is_ok()
        {
            update(|status| status.books_repaired += 1);
        }
    }
}

async fn replace_authors(
    pool: &SqlitePool,
    book_id: i64,
    authors: &[String],
) -> Result<(), AppError> {
    let mut transaction = pool.begin().await?;

    sqlx::query("DELETE FROM book_authors WHERE book_id = ?")
        .bind(book_id)
        .execute(&mut *transaction)
        .await?;

    for (position, author) in authors
        .iter()
        .map(|author| author.trim())
        .filter(|author| !author.is_empty())
        .enumerate()
    {
        let normalized = bokhylle_core::identity::normalize_text(author);
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM authors WHERE normalized_name = ? LIMIT 1")
                .bind(&normalized)
                .fetch_optional(&mut *transaction)
                .await?;
        let author_id = match existing {
            Some(id) => id,
            None => sqlx::query("INSERT INTO authors (name, normalized_name) VALUES (?, ?)")
                .bind(author)
                .bind(&normalized)
                .execute(&mut *transaction)
                .await?
                .last_insert_rowid(),
        };

        sqlx::query(
            "INSERT OR IGNORE INTO book_authors (book_id, author_id, position) VALUES (?, ?, ?)",
        )
        .bind(book_id)
        .bind(author_id)
        .bind(position as i64)
        .execute(&mut *transaction)
        .await?;
    }

    crate::library::refresh_fts(&mut transaction, book_id).await?;
    transaction.commit().await?;
    Ok(())
}

async fn prune_orphan_authors(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::query(
        // Retention is about durability, not a provider id: local books,
        // follows and discovery rows keep an author alive; a search-only
        // author with just an olid does not.
        "DELETE FROM authors
         WHERE NOT EXISTS (SELECT 1 FROM book_authors WHERE author_id = authors.id)
           AND NOT EXISTS (SELECT 1 FROM author_follows f WHERE f.author_id = authors.id)
           AND NOT EXISTS (SELECT 1 FROM author_discoveries d WHERE d.author_id = authors.id)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn set_cover_path(state: &AppState, book_id: i64, path: &Path) -> bool {
    sqlx::query("UPDATE books SET cover_path = ?, updated_at = unixepoch() WHERE id = ?")
        .bind(path.to_string_lossy().as_ref())
        .bind(book_id)
        .execute(&state.db)
        .await
        .is_ok()
}

async fn write_cover(dir: &Path, path: &Path, bytes: &[u8]) -> bool {
    tokio::fs::create_dir_all(dir).await.is_ok()
        && crate::routes::image_cache::write_atomic(path, bytes)
            .await
            .is_ok()
}

fn titles_match(candidate: &str, expected: &str) -> bool {
    let candidate = bokhylle_core::identity::normalize_text(candidate);
    let expected = bokhylle_core::identity::normalize_text(expected);
    !expected.is_empty()
        && (candidate == expected || candidate.contains(&expected) || expected.contains(&candidate))
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
