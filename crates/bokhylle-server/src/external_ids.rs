//! Provider identities as attributes of Bokhylle entities. A provider key is
//! unique per provider and never becomes the entity's identity; the legacy
//! `authors.olid` column is read as a fallback during the transition.

use sqlx::SqlitePool;

use crate::error::AppError;

pub async fn link_author(
    pool: &SqlitePool,
    author_id: i64,
    provider: &str,
    provider_key: &str,
) -> Result<(), AppError> {
    let provider_key = provider_key.trim();
    if provider.is_empty() || provider_key.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO author_external_ids (author_id, provider, provider_key)
         VALUES (?, ?, ?)
         ON CONFLICT(provider, provider_key) DO NOTHING",
    )
    .bind(author_id)
    .bind(provider)
    .bind(provider_key)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn link_book(
    pool: &SqlitePool,
    book_id: i64,
    provider: &str,
    provider_key: &str,
) -> Result<(), AppError> {
    let provider_key = provider_key.trim();
    if provider.is_empty() || provider_key.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO book_external_ids (book_id, provider, provider_key)
         VALUES (?, ?, ?)
         ON CONFLICT(provider, provider_key) DO NOTHING",
    )
    .bind(book_id)
    .bind(provider)
    .bind(provider_key)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn link_book_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: i64,
    provider: &str,
    provider_key: &str,
) -> Result<(), AppError> {
    let provider_key = provider_key.trim();
    if provider.is_empty() || provider_key.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO book_external_ids (book_id, provider, provider_key)
         VALUES (?, ?, ?)
         ON CONFLICT(provider, provider_key) DO NOTHING",
    )
    .bind(book_id)
    .bind(provider)
    .bind(provider_key)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn author_by_provider(
    pool: &SqlitePool,
    provider: &str,
    provider_key: &str,
) -> Result<Option<i64>, AppError> {
    Ok(sqlx::query_scalar(
        "SELECT author_id FROM author_external_ids WHERE provider = ? AND provider_key = ?",
    )
    .bind(provider)
    .bind(provider_key.trim())
    .fetch_optional(pool)
    .await?
    .flatten())
}

pub async fn book_by_provider(
    pool: &SqlitePool,
    provider: &str,
    provider_key: &str,
) -> Result<Option<i64>, AppError> {
    Ok(sqlx::query_scalar(
        "SELECT book_id FROM book_external_ids WHERE provider = ? AND provider_key = ?",
    )
    .bind(provider)
    .bind(provider_key.trim())
    .fetch_optional(pool)
    .await?
    .flatten())
}

/// The author's Open Library id from the identity table, falling back to the
/// legacy column for rows written before the transition.
pub async fn author_olid(pool: &SqlitePool, author_id: i64) -> Result<Option<String>, AppError> {
    if let Some(olid) = sqlx::query_scalar(
        "SELECT provider_key FROM author_external_ids
         WHERE author_id = ? AND provider = 'openlibrary' LIMIT 1",
    )
    .bind(author_id)
    .fetch_optional(pool)
    .await?
    .flatten()
    {
        return Ok(Some(olid));
    }
    Ok(sqlx::query_scalar("SELECT olid FROM authors WHERE id = ?")
        .bind(author_id)
        .fetch_optional(pool)
        .await?
        .flatten())
}

/// A book's primary provider link. Durable-identity providers (Open Library)
/// win over enrichment-only ones; otherwise the oldest link is used and the
/// edition columns remain a fallback.
pub async fn book_provider(
    pool: &SqlitePool,
    book_id: i64,
    durable: &[&str],
) -> Result<Option<(String, String)>, AppError> {
    let links: Vec<(String, String)> = sqlx::query_as(
        "SELECT provider, provider_key FROM book_external_ids
         WHERE book_id = ? ORDER BY created_at, provider",
    )
    .bind(book_id)
    .fetch_all(pool)
    .await?;
    if let Some(link) = links
        .iter()
        .find(|(provider, _)| durable.contains(&provider.as_str()))
    {
        return Ok(Some(link.clone()));
    }
    if let Some(link) = links.first() {
        return Ok(Some(link.clone()));
    }
    Ok(sqlx::query_as(
        "SELECT provider, provider_key FROM editions
         WHERE book_id = ? AND provider IS NOT NULL AND provider_key IS NOT NULL
         ORDER BY id LIMIT 1",
    )
    .bind(book_id)
    .fetch_optional(pool)
    .await?)
}
