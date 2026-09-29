use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, FromRow, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub acquisition_id: Option<String>,
    pub book_id: Option<i64>,
    pub read: bool,
    pub created_at: i64,
}

#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &SqlitePool,
    user_id: Option<i64>,
    kind: &str,
    title: &str,
    body: Option<&str>,
    acquisition_id: Option<&str>,
    book_id: Option<i64>,
) -> Result<(), AppError> {
    let Some(user_id) = user_id else {
        return Ok(());
    };

    sqlx::query(
        "INSERT INTO notifications (user_id, kind, title, body, acquisition_id, book_id)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(kind)
    .bind(title)
    .bind(body)
    .bind(acquisition_id)
    .bind(book_id)
    .execute(pool)
    .await?;

    if let Err(error) = send_email(pool, user_id, kind, title, body).await {
        tracing::warn!(user_id, kind, %error, "notification.email.failed");
    }

    Ok(())
}

/// Emails are opt-in per user and best-effort: failures never affect the
/// in-app notification that triggered them.
async fn send_email(
    pool: &SqlitePool,
    user_id: i64,
    kind: &str,
    title: &str,
    body: Option<&str>,
) -> Result<(), AppError> {
    if !matches!(kind, "ready" | "failed" | "needs_selection") {
        return Ok(());
    }

    let row: Option<(Option<String>, i64)> = sqlx::query_as(
        "SELECT notification_email, email_notifications
         FROM users WHERE id = ? AND disabled = 0",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some((Some(address), enabled)) = row else {
        return Ok(());
    };
    let address = address.trim();
    if enabled == 0 || address.is_empty() {
        return Ok(());
    }

    let settings = crate::settings::Settings::new(pool.clone());
    if !crate::delivery::smtp_configured(&settings).await {
        return Ok(());
    }

    let mut text = title.to_string();
    if let Some(body) = body.filter(|body| !body.trim().is_empty()) {
        text.push_str("\n\n");
        text.push_str(body.trim());
    }
    text.push_str("\n\nOpen Bokhylle to see the details.");

    crate::delivery::send_plain(&settings, address, title, &text).await
}

pub async fn create_for_book(
    pool: &SqlitePool,
    user_id: i64,
    book_id: Option<i64>,
    kind: &str,
    title: &str,
    body: Option<&str>,
    acquisition_id: Option<&str>,
) -> Result<(), AppError> {
    let book_title: Option<String> = match book_id {
        Some(book_id) => {
            sqlx::query_scalar("SELECT title FROM books WHERE id = ?")
                .bind(book_id)
                .fetch_optional(pool)
                .await?
        }
        None => None,
    };

    let title = match book_title {
        Some(book_title) => format!("{title}: {book_title}"),
        None => title.to_string(),
    };

    create(
        pool,
        Some(user_id),
        kind,
        &title,
        body,
        acquisition_id,
        book_id,
    )
    .await
}

/// Notify everyone who requested this acquisition (the creator and any
/// household member who asked for the same book while it was active).
pub async fn for_requesters(
    pool: &SqlitePool,
    acquisition_id: &str,
    kind: &str,
    title: &str,
    body: Option<&str>,
) -> Result<(), AppError> {
    let book_id: Option<i64> = sqlx::query_scalar("SELECT book_id FROM acquisitions WHERE id = ?")
        .bind(acquisition_id)
        .fetch_optional(pool)
        .await?;

    for user_id in crate::acquisition_requests::requesters(pool, acquisition_id).await? {
        create_for_book(
            pool,
            user_id,
            book_id,
            kind,
            title,
            body,
            Some(acquisition_id),
        )
        .await?;
    }

    Ok(())
}

/// A child only sees notifications about books on their own shelf, so a
/// converted account cannot read historical adult activity.
pub async fn list(
    pool: &SqlitePool,
    user_id: i64,
    limit: i64,
    child: bool,
) -> Result<Vec<Notification>, AppError> {
    let notifications: Vec<Notification> = sqlx::query_as(
        "SELECT id, kind, title, body, acquisition_id, book_id, read, created_at
         FROM notifications
         WHERE user_id = ?
           AND (? = 0 OR book_id IS NULL OR book_id IN (
               SELECT book_id FROM user_books WHERE user_id = ? AND on_shelf = 1))
         ORDER BY created_at DESC, id DESC
         LIMIT ?",
    )
    .bind(user_id)
    .bind(i64::from(child))
    .bind(user_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await?;

    Ok(notifications)
}

pub async fn unread_count(pool: &SqlitePool, user_id: i64, child: bool) -> Result<i64, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notifications
         WHERE user_id = ? AND read = 0
           AND (? = 0 OR book_id IS NULL OR book_id IN (
               SELECT book_id FROM user_books WHERE user_id = ? AND on_shelf = 1))",
    )
    .bind(user_id)
    .bind(i64::from(child))
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

pub async fn mark_all_read(pool: &SqlitePool, user_id: i64) -> Result<u64, AppError> {
    let result = sqlx::query("UPDATE notifications SET read = 1 WHERE user_id = ? AND read = 0")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
