use sqlx::SqlitePool;

use crate::error::AppError;

pub async fn register(
    pool: &SqlitePool,
    acquisition_id: &str,
    user_id: i64,
    deliver_on_ready: bool,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    register_tx(&mut tx, acquisition_id, user_id, deliver_on_ready).await?;
    tx.commit().await?;
    Ok(())
}

/// Registration inside an existing transaction: the request row and the
/// requester's shelf membership commit with the acquisition itself.
pub async fn register_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    acquisition_id: &str,
    user_id: i64,
    deliver_on_ready: bool,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO acquisition_requests (acquisition_id, user_id, deliver_on_ready)
         VALUES (?, ?, ?)
         ON CONFLICT(acquisition_id, user_id) DO UPDATE SET
             deliver_on_ready = MAX(deliver_on_ready, excluded.deliver_on_ready)",
    )
    .bind(acquisition_id)
    .bind(user_id)
    .bind(deliver_on_ready)
    .execute(&mut **tx)
    .await?;

    let book_id: Option<i64> = sqlx::query_scalar("SELECT book_id FROM acquisitions WHERE id = ?")
        .bind(acquisition_id)
        .fetch_optional(&mut **tx)
        .await?;
    if let Some(book_id) = book_id {
        crate::services::sharing::own_tx(tx, user_id, book_id).await?;
        crate::user_books::add_tx(tx, user_id, book_id, "requested").await?;
    }

    Ok(())
}

pub async fn requesters(pool: &SqlitePool, acquisition_id: &str) -> Result<Vec<i64>, AppError> {
    let users: Vec<i64> =
        sqlx::query_scalar("SELECT user_id FROM acquisition_requests WHERE acquisition_id = ?")
            .bind(acquisition_id)
            .fetch_all(pool)
            .await?;
    Ok(users)
}

#[derive(sqlx::FromRow)]
pub struct PendingDelivery {
    pub user_id: i64,
    pub delivery_target_id: Option<i64>,
    pub delivery_address: Option<String>,
}

pub async fn pending_deliveries(
    pool: &SqlitePool,
    acquisition_id: &str,
) -> Result<Vec<PendingDelivery>, AppError> {
    let users = sqlx::query_as(
        "SELECT user_id, delivery_target_id, delivery_address FROM acquisition_requests
         WHERE acquisition_id = ? AND deliver_on_ready = 1",
    )
    .bind(acquisition_id)
    .fetch_all(pool)
    .await?;
    Ok(users)
}

pub async fn consume(
    pool: &SqlitePool,
    acquisition_id: &str,
    user_id: i64,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        "UPDATE acquisition_requests SET deliver_on_ready = 0
         WHERE acquisition_id = ? AND user_id = ? AND deliver_on_ready = 1",
    )
    .bind(acquisition_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}
