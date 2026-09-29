//! Durable retry intent: a failure that is likely to improve over time
//! schedules the acquisition for another attempt instead of making the user
//! recreate the request.

use sqlx::SqlitePool;

use bokhylle_acquisition::state::AcquisitionStatus;

use crate::AppState;
use crate::error::AppError;
use crate::settings::{self, Settings};

/// Transient download/search failures worth retrying automatically.
pub const RETRYABLE_DOWNLOAD_ERRORS: [&str; 3] =
    ["download_failed", "download_missing", "search_failed"];
/// Import failures that indicate a path or mount problem rather than wrong
/// content; intrinsic failures are blocklisted instead.
pub const RETRYABLE_IMPORT_ERRORS: [&str; 1] = ["content_missing"];

const BACKOFF_DAYS: [i64; 4] = [1, 2, 4, 7];
const MAX_ATTEMPTS: i64 = 8;

pub fn is_retryable(status: AcquisitionStatus, error_code: Option<&str>) -> bool {
    match status {
        AcquisitionStatus::NoReleaseFound => true,
        AcquisitionStatus::DownloadFailed => {
            error_code.is_some_and(|code| RETRYABLE_DOWNLOAD_ERRORS.contains(&code))
        }
        AcquisitionStatus::ImportFailed => {
            error_code.is_some_and(|code| RETRYABLE_IMPORT_ERRORS.contains(&code))
        }
        _ => false,
    }
}

/// 1, 2, 4, 7 days, then weekly.
pub fn backoff_seconds(attempts: i64) -> i64 {
    BACKOFF_DAYS[attempts.clamp(0, 3) as usize] * 24 * 3600
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

/// Runs after a failure transition: schedules the next automatic attempt, or
/// stops with a notification when the retry window is exhausted.
pub async fn schedule_after_failure(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    // A direct link is a particular user choice, often short-lived. Retrying
    // it for weeks would repeat the same URL rather than discover a new copy.
    let direct: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM acquisition_inputs WHERE acquisition_id = ?)",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    if direct != 0 {
        return Ok(());
    }
    let settings = Settings::new(pool.clone());
    let enabled = settings
        .get_bool(settings::RETRIES_ENABLED, true)
        .await
        .unwrap_or(true);
    let max_days = settings
        .get_int(settings::RETRIES_MAX_DAYS, 60)
        .await
        .unwrap_or(60);

    type RetryRow = (String, Option<String>, i64, Option<i64>, i64);
    let row: Option<RetryRow> = sqlx::query_as(
        "SELECT status, error_code, retry_attempts, retry_started_at, retry_stopped
         FROM acquisitions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some((status, error_code, attempts, started_at, stopped)) = row else {
        return Ok(());
    };
    let Some(status) = AcquisitionStatus::from_db(&status) else {
        return Ok(());
    };
    if stopped != 0 || !enabled || !is_retryable(status, error_code.as_deref()) {
        return Ok(());
    }

    let now = now_epoch();
    let started_at = started_at.unwrap_or(now);
    let next = now + backoff_seconds(attempts);
    let window_exhausted =
        max_days > 0 && (now - started_at + backoff_seconds(attempts)) > max_days * 24 * 3600;
    if window_exhausted || attempts >= MAX_ATTEMPTS {
        sqlx::query(
            "UPDATE acquisitions
             SET retry_stopped = 1, next_retry_at = NULL, retry_started_at = ?, updated_at = unixepoch()
             WHERE id = ?",
        )
        .bind(started_at)
        .bind(id)
        .execute(pool)
        .await?;
        crate::notifications::for_requesters(
            pool,
            id,
            "failed",
            "We stopped looking for this book",
            Some("Retries ran out. Request it again when you want another attempt."),
        )
        .await
        .ok();
        tracing::info!(acquisition_id = %id, attempts, "keep_looking.stopped");
        return Ok(());
    }

    sqlx::query(
        "UPDATE acquisitions
         SET retry_started_at = ?, next_retry_at = ?, updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(started_at)
    .bind(next)
    .bind(id)
    .execute(pool)
    .await?;
    tracing::info!(acquisition_id = %id, attempts, next_retry_at = next, "keep_looking.scheduled");
    Ok(())
}

/// The scheduler: claim due retries atomically, then re-run the pipeline.
pub async fn tick(state: &AppState) -> Result<u32, AppError> {
    let enabled = state
        .settings
        .get_bool(settings::RETRIES_ENABLED, true)
        .await
        .unwrap_or(true);
    if !enabled {
        return Ok(0);
    }

    let due: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, status, error_code FROM acquisitions
         WHERE retry_stopped = 0 AND next_retry_at IS NOT NULL AND next_retry_at <= unixepoch()
         ORDER BY next_retry_at
         LIMIT 20",
    )
    .fetch_all(&state.db)
    .await?;

    let mut retried = 0u32;
    for (id, status, error_code) in due {
        let Some(status) = AcquisitionStatus::from_db(&status) else {
            continue;
        };
        if !is_retryable(status, error_code.as_deref()) {
            continue;
        }
        // Claim and reopen commit together: an error leaves the due retry in
        // place for the next tick instead of clearing the schedule.
        match crate::acquisition::claim_retry(&state.db, &id).await {
            Ok(true) => {
                tracing::info!(acquisition_id = %id, "keep_looking.retry");
                crate::acquisition_pipeline::spawn(state, id);
                retried += 1;
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(acquisition_id = %id, %error, "keep_looking.retry_failed");
            }
        }
    }
    Ok(retried)
}

pub fn spawn_scheduler(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = tick(&state).await {
                tracing::warn!(%error, "keep_looking.tick_failed");
            }
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
        }
    });
}

/// Stop or resume automatic retries for one acquisition. Resuming schedules
/// an immediate attempt when the current failure is retryable.
pub async fn set_keep_looking(pool: &SqlitePool, id: &str, enabled: bool) -> Result<(), AppError> {
    if enabled {
        let row: Option<(String, Option<String>)> =
            sqlx::query_as("SELECT status, error_code FROM acquisitions WHERE id = ?")
                .bind(id)
                .fetch_optional(pool)
                .await?;
        let Some((status, error_code)) = row else {
            return Err(AppError::NotFound("acquisition not found".to_string()));
        };
        let status = AcquisitionStatus::from_db(&status).ok_or_else(|| {
            AppError::Unprocessable(format!("unknown acquisition status '{status}'"))
        })?;
        let immediate = is_retryable(status, error_code.as_deref());
        sqlx::query(
            "UPDATE acquisitions
             SET retry_stopped = 0,
                 next_retry_at = CASE WHEN ? THEN unixepoch() ELSE next_retry_at END,
                 retry_started_at = COALESCE(retry_started_at, unixepoch()),
                 updated_at = unixepoch()
             WHERE id = ?",
        )
        .bind(immediate)
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "UPDATE acquisitions
             SET retry_stopped = 1, next_retry_at = NULL, updated_at = unixepoch()
             WHERE id = ?",
        )
        .bind(id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_separates_transient_from_intrinsic() {
        assert!(is_retryable(AcquisitionStatus::NoReleaseFound, None));
        assert!(is_retryable(
            AcquisitionStatus::DownloadFailed,
            Some("download_missing")
        ));
        assert!(is_retryable(
            AcquisitionStatus::ImportFailed,
            Some("content_missing")
        ));
        assert!(!is_retryable(
            AcquisitionStatus::ImportFailed,
            Some("corrupt_archive")
        ));
        assert!(!is_retryable(
            AcquisitionStatus::DownloadFailed,
            Some("integrations_not_configured")
        ));
        assert!(!is_retryable(AcquisitionStatus::Cancelled, None));
        assert!(!is_retryable(AcquisitionStatus::NeedsReview, None));
    }

    #[test]
    fn backoff_grows_then_holds_at_a_week() {
        assert_eq!(backoff_seconds(0), 24 * 3600);
        assert_eq!(backoff_seconds(1), 2 * 24 * 3600);
        assert_eq!(backoff_seconds(2), 4 * 24 * 3600);
        assert_eq!(backoff_seconds(3), 7 * 24 * 3600);
        assert_eq!(backoff_seconds(9), 7 * 24 * 3600);
    }
}
