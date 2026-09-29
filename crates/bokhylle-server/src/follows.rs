use sqlx::SqlitePool;

use crate::error::AppError;

pub async fn set(
    pool: &SqlitePool,
    user_id: i64,
    author_id: i64,
    follow: bool,
) -> Result<(), AppError> {
    if follow {
        sqlx::query(
            "INSERT INTO author_follows (user_id, author_id) VALUES (?, ?)
             ON CONFLICT(user_id, author_id) DO NOTHING",
        )
        .bind(user_id)
        .bind(author_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("DELETE FROM author_follows WHERE user_id = ? AND author_id = ?")
            .bind(user_id)
            .bind(author_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

pub async fn set_automation(
    pool: &SqlitePool,
    user_id: i64,
    author_id: i64,
    auto_acquire: bool,
    delivery_target_id: Option<i64>,
) -> Result<(), AppError> {
    // Enabling sets the baseline only once; existing catalogue entries are
    // never eligible for automatic acquisition.
    sqlx::query(
        "UPDATE author_follows
         SET auto_acquire = ?,
             delivery_target_id = ?,
             baseline_at = CASE
                 WHEN ? = 1 AND baseline_at IS NULL THEN unixepoch()
                 ELSE baseline_at
             END
         WHERE user_id = ? AND author_id = ?",
    )
    .bind(auto_acquire)
    .bind(delivery_target_id)
    .bind(auto_acquire)
    .bind(user_id)
    .bind(author_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn automation_for(
    pool: &SqlitePool,
    user_id: i64,
    author_id: i64,
) -> Result<(bool, Option<i64>), AppError> {
    let row: Option<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT auto_acquire, delivery_target_id FROM author_follows
         WHERE user_id = ? AND author_id = ?",
    )
    .bind(user_id)
    .bind(author_id)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some((auto, target)) => (auto != 0, target),
        None => (false, None),
    })
}

pub async fn is_following(
    pool: &SqlitePool,
    user_id: i64,
    author_id: i64,
) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM author_follows WHERE user_id = ? AND author_id = ?",
    )
    .bind(user_id)
    .bind(author_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn followed_authors(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<(i64, String)>, AppError> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT a.id, a.name FROM author_follows f
         JOIN authors a ON a.id = f.author_id
         WHERE f.user_id = ?
         ORDER BY f.created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
