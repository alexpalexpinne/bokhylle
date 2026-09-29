//! Household book requests: durable user intent that an adult decides, with
//! approval delegated to the normal acquisition pipeline.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::AppError;

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookRequestView {
    pub id: i64,
    pub book_id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub requester: String,
    pub requester_user_id: i64,
    pub status: String,
    /// User-facing stage: requested | looking | getting | ready | declined |
    /// unavailable. Internal acquisition states never leave the server.
    pub phase: String,
    pub acquisition_id: Option<String>,
    pub keep_looking: bool,
    pub error_code: Option<String>,
    /// Latest reader send for this approval, when one has been attempted.
    pub delivery_status: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct BookRequest {
    pub id: i64,
    pub book_id: i64,
    pub user_id: i64,
    pub status: String,
}

type ViewRow = (
    i64,
    i64,
    String,
    String,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
    i64,
);

const VIEW_SQL: &str = "SELECT r.id, r.book_id, b.title,
       COALESCE((SELECT group_concat(au.name, ', ')
                 FROM book_authors ba JOIN authors au ON au.id = ba.author_id
                 WHERE ba.book_id = b.id), '') AS authors,
       r.status, r.user_id, COALESCE(u.display_name, u.username) AS requester,
       r.acquisition_id, a.status AS acquisition_status, a.error_code,
       (SELECT d.status FROM deliveries d
        WHERE r.status = 'approved' AND d.book_id = r.book_id AND d.user_id = r.user_id
          AND d.created_at >= r.updated_at
        ORDER BY d.id DESC LIMIT 1) AS delivery_status,
       CASE WHEN a.retry_stopped = 0 AND a.next_retry_at IS NOT NULL THEN 1 ELSE 0 END AS keep_looking,
       r.created_at, r.updated_at
FROM book_requests r
JOIN books b ON b.id = r.book_id
JOIN users u ON u.id = r.user_id
LEFT JOIN acquisitions a ON a.id = r.acquisition_id";

pub fn phase(
    status: &str,
    acquisition_id: Option<&str>,
    acquisition_status: Option<&str>,
    keep_looking: bool,
) -> String {
    match status {
        "declined" => "declined",
        "requested" => "requested",
        // Approved without an acquisition: a household copy already satisfied
        // the request.
        _ if acquisition_id.is_none() => "ready",
        _ => match acquisition_status {
            Some("READY") => "ready",
            Some(
                "QUEUED" | "DOWNLOADING" | "DOWNLOADED" | "INSPECTING" | "IDENTIFIED" | "IMPORTING",
            ) => "getting",
            Some("CANCELLED") => "declined",
            Some("REQUESTED" | "SEARCHING" | "EVALUATING") => "looking",
            Some("NEEDS_SELECTION" | "NEEDS_REVIEW") => "looking",
            Some(_) if keep_looking => "looking",
            Some(_) => "unavailable",
            None => "looking",
        },
    }
    .to_string()
}

fn view(row: ViewRow) -> BookRequestView {
    let (
        id,
        book_id,
        title,
        authors,
        status,
        requester_user_id,
        requester,
        acquisition_id,
        acquisition_status,
        error_code,
        delivery_status,
        keep_looking,
        created_at,
        updated_at,
    ) = row;
    BookRequestView {
        id,
        book_id,
        title,
        authors: authors
            .split(", ")
            .map(str::trim)
            .filter(|author| !author.is_empty())
            .map(str::to_string)
            .collect(),
        requester,
        requester_user_id,
        phase: phase(
            &status,
            acquisition_id.as_deref(),
            acquisition_status.as_deref(),
            keep_looking != 0,
        ),
        status,
        acquisition_id,
        keep_looking: keep_looking != 0,
        error_code,
        delivery_status,
        created_at,
        updated_at,
    }
}

/// Creates (or returns the existing) pending request for this user and book.
/// The partial unique index is the synchronization primitive: a concurrent
/// duplicate insert is ignored and both callers return the canonical row.
pub async fn create(
    pool: &SqlitePool,
    book_id: i64,
    user_id: i64,
) -> Result<(i64, bool), AppError> {
    let mut tx = pool.begin().await?;
    let inserted =
        sqlx::query("INSERT OR IGNORE INTO book_requests (book_id, user_id) VALUES (?, ?)")
            .bind(book_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM book_requests
         WHERE book_id = ? AND user_id = ? AND status = 'requested'",
    )
    .bind(book_id)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((id, inserted.rows_affected() == 0))
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<BookRequestView>, AppError> {
    let sql = format!("{VIEW_SQL} WHERE r.id = ?");
    let row: Option<ViewRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(view))
}

pub async fn raw(pool: &SqlitePool, id: i64) -> Result<Option<BookRequest>, AppError> {
    let row: Option<(i64, i64, i64, String)> =
        sqlx::query_as("SELECT id, book_id, user_id, status FROM book_requests WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id, book_id, user_id, status)| BookRequest {
        id,
        book_id,
        user_id,
        status,
    }))
}

/// `all` lists the household's requests for an administrator; otherwise only
/// the caller's own requests.
pub async fn list(
    pool: &SqlitePool,
    viewer_id: i64,
    all: bool,
) -> Result<Vec<BookRequestView>, AppError> {
    let sql = if all {
        format!(
            "{VIEW_SQL}
             ORDER BY CASE r.status WHEN 'requested' THEN 0 ELSE 1 END,
                      r.created_at DESC
             LIMIT 100"
        )
    } else {
        format!(
            "{VIEW_SQL}
             WHERE r.user_id = ?
             ORDER BY CASE r.status WHEN 'requested' THEN 0 ELSE 1 END,
                      r.created_at DESC
             LIMIT 100"
        )
    };
    let mut query = sqlx::query_as::<_, ViewRow>(sqlx::AssertSqlSafe(sql));
    if !all {
        query = query.bind(viewer_id);
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows.into_iter().map(view).collect())
}

/// The request-decision compare-and-swap, usable inside the approval
/// transaction so a decision cannot commit without its acquisition or shelf
/// membership. Returns false when another adult already decided.
pub async fn mark_approved_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: i64,
    decider_id: i64,
) -> Result<bool, AppError> {
    let updated = sqlx::query(
        "UPDATE book_requests
         SET status = 'approved', decided_by = ?, decided_at = unixepoch(),
             updated_at = unixepoch()
         WHERE id = ? AND status = 'requested'",
    )
    .bind(decider_id)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(updated.rows_affected() > 0)
}

pub async fn mark_declined(pool: &SqlitePool, id: i64, decider_id: i64) -> Result<bool, AppError> {
    let updated = sqlx::query(
        "UPDATE book_requests
         SET status = 'declined', decided_by = ?, decided_at = unixepoch(), updated_at = unixepoch()
         WHERE id = ? AND status = 'requested'",
    )
    .bind(decider_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(updated.rows_affected() > 0)
}
