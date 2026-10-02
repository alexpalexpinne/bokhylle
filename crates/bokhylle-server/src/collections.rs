use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, FromRow, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: i64,
    pub name: String,
    pub book_count: i64,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<CollectionSummary>, AppError> {
    let collections: Vec<CollectionSummary> = sqlx::query_as(
        "SELECT c.id, c.name,
                (SELECT count(*) FROM collection_books cb WHERE cb.collection_id = c.id) AS book_count
         FROM collections c
         ORDER BY c.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;

    Ok(collections)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<CollectionSummary>, AppError> {
    let collection: Option<CollectionSummary> = sqlx::query_as(
        "SELECT c.id, c.name,
                (SELECT count(*) FROM collection_books cb WHERE cb.collection_id = c.id) AS book_count
         FROM collections c
         WHERE c.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(collection)
}

pub async fn create(pool: &SqlitePool, name: &str) -> Result<CollectionSummary, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Unprocessable(
            "collection name must not be empty".to_string(),
        ));
    }

    let result = sqlx::query("INSERT INTO collections (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(ref database_error) if database_error.is_unique_violation() => {
                AppError::Conflict(format!("collection '{name}' already exists"))
            }
            other => AppError::Internal(other),
        })?;

    get(pool, result.last_insert_rowid())
        .await?
        .ok_or_else(|| AppError::Unprocessable("collection disappeared after insert".to_string()))
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM collections WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn add_book(pool: &SqlitePool, collection_id: i64, book_id: i64) -> Result<(), AppError> {
    if get(pool, collection_id).await?.is_none() {
        return Err(AppError::NotFound("collection not found".to_string()));
    }
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("book not found".to_string()));
    }

    sqlx::query("INSERT OR IGNORE INTO collection_books (collection_id, book_id) VALUES (?, ?)")
        .bind(collection_id)
        .bind(book_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn remove_book(
    pool: &SqlitePool,
    collection_id: i64,
    book_id: i64,
) -> Result<bool, AppError> {
    let result =
        sqlx::query("DELETE FROM collection_books WHERE collection_id = ? AND book_id = ?")
            .bind(collection_id)
            .bind(book_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn for_book(pool: &SqlitePool, book_id: i64) -> Result<Vec<CollectionSummary>, AppError> {
    let collections: Vec<CollectionSummary> = sqlx::query_as(
        "SELECT c.id, c.name,
                (SELECT count(*) FROM collection_books cb2 WHERE cb2.collection_id = c.id) AS book_count
         FROM collections c
         JOIN collection_books cb ON cb.collection_id = c.id
         WHERE cb.book_id = ?
         ORDER BY c.name COLLATE NOCASE",
    )
    .bind(book_id)
    .fetch_all(pool)
    .await?;

    Ok(collections)
}

pub async fn list_visible(
    pool: &SqlitePool,
    viewer_id: i64,
) -> Result<Vec<CollectionSummary>, AppError> {
    let visibility = crate::services::sharing::predicate("cb.book_id", viewer_id);
    let sql = format!("SELECT c.id, c.name,
        (SELECT count(*) FROM collection_books cb WHERE cb.collection_id = c.id AND {visibility}) AS book_count
        FROM collections c WHERE NOT EXISTS(SELECT 1 FROM collection_books cb WHERE cb.collection_id = c.id)
        OR EXISTS(SELECT 1 FROM collection_books cb WHERE cb.collection_id = c.id AND {visibility}) ORDER BY c.name COLLATE NOCASE");
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?)
}

pub async fn require_visible(pool: &SqlitePool, viewer_id: i64, id: i64) -> Result<(), AppError> {
    let visibility = crate::services::sharing::predicate("cb.book_id", viewer_id);
    let sql = format!("SELECT EXISTS(SELECT 1 FROM collections c WHERE c.id = ? AND (
        NOT EXISTS(SELECT 1 FROM collection_books cb WHERE cb.collection_id = c.id)
        OR EXISTS(SELECT 1 FROM collection_books cb WHERE cb.collection_id = c.id AND {visibility})))");
    let visible: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await?;
    if !visible {
        return Err(AppError::NotFound("collection not found".into()));
    }
    Ok(())
}
