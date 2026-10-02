use sqlx::SqlitePool;

use crate::error::AppError;

/// Adds a book to a user's shelf, ignoring duplicates. Every signal that a
/// user wants a book — requesting it, receiving it, sending it, asking for it
/// manually — funnels through here.
const ADD_SQL: &str = "INSERT INTO user_books (user_id, book_id, source, on_shelf)
     VALUES (?, ?, ?, 1)
     ON CONFLICT(user_id, book_id) DO UPDATE SET
         on_shelf = 1,
         added_at = CASE
             WHEN user_books.on_shelf = 0 THEN unixepoch()
             ELSE user_books.added_at
         END,
         source = CASE
             WHEN user_books.on_shelf = 0 THEN excluded.source
             ELSE user_books.source
         END";

pub async fn add(
    pool: &SqlitePool,
    user_id: i64,
    book_id: i64,
    source: &str,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    add_tx(&mut tx, user_id, book_id, source).await?;
    tx.commit().await?;
    Ok(())
}

/// Shelf membership inside an existing transaction, so acquisition creation
/// is one atomic write.
pub async fn add_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: i64,
    book_id: i64,
    source: &str,
) -> Result<(), AppError> {
    if matches!(source, "manual" | "claimed" | "sent" | "agent") {
        crate::services::sharing::require_access_tx(tx, user_id, book_id).await?;
    }
    crate::services::sharing::grant_tx(tx, user_id, book_id).await?;
    sqlx::query(ADD_SQL)
        .bind(user_id)
        .bind(book_id)
        .bind(source)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn remove(pool: &SqlitePool, user_id: i64, book_id: i64) -> Result<(), AppError> {
    // Taking a book off the shelf keeps a preference-only row (likes and
    // "not for me" stay meaningful without claiming shelf membership).
    sqlx::query("UPDATE user_books SET on_shelf = 0 WHERE user_id = ? AND book_id = ?")
        .bind(user_id)
        .bind(book_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM user_books
         WHERE user_id = ? AND book_id = ? AND preference IS NULL",
    )
    .bind(user_id)
    .bind(book_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn contains(pool: &SqlitePool, user_id: i64, book_id: i64) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM user_books WHERE user_id = ? AND book_id = ? AND on_shelf = 1",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

/// Claims every household book for a user (the "add all to my shelf" action).
pub async fn claim_all(pool: &SqlitePool, user_id: i64) -> Result<u64, AppError> {
    let mut tx = pool.begin().await?;
    let visibility = crate::services::sharing::predicate("b.id", user_id);
    let sql = format!("SELECT b.id FROM books b WHERE {visibility}
        AND EXISTS(SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id WHERE e.book_id = b.id)");
    let ids: Vec<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *tx)
        .await?;
    for id in &ids {
        add_tx(&mut tx, user_id, *id, "claimed").await?;
    }
    tx.commit().await?;
    Ok(ids.len() as u64)
}

pub async fn set_preference(
    pool: &SqlitePool,
    user_id: i64,
    book_id: i64,
    preference: Option<&str>,
) -> Result<(), AppError> {
    if let Some(value) = preference
        && !matches!(value, "liked" | "not_for_me")
    {
        return Err(AppError::Unprocessable(
            "preference must be 'liked' or 'not_for_me'".to_string(),
        ));
    }
    // Preferences are affinity, not membership: they never add a book to the
    // shelf. The row can exist with on_shelf = 0.
    sqlx::query(
        "INSERT INTO user_books (user_id, book_id, source, on_shelf, preference)
         VALUES (?, ?, 'manual', 0, ?)
         ON CONFLICT(user_id, book_id) DO UPDATE SET preference = excluded.preference",
    )
    .bind(user_id)
    .bind(book_id)
    .bind(preference)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn preference(
    pool: &SqlitePool,
    user_id: i64,
    book_id: i64,
) -> Result<Option<String>, AppError> {
    Ok(
        sqlx::query_scalar("SELECT preference FROM user_books WHERE user_id = ? AND book_id = ?")
            .bind(user_id)
            .bind(book_id)
            .fetch_optional(pool)
            .await?
            .flatten(),
    )
}
