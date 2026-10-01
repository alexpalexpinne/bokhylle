use crate::{AppState, error::AppError};
use serde::Serialize;
use sqlx::FromRow;

pub const RETRY_SECONDS: i64 = 300;

#[derive(Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackupEvent {
    pub at: i64,
    pub summary: Option<String>,
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LatestBackup {
    pub created_at: i64,
    pub size: u64,
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub interval_hours: f64,
    pub keep: i64,
    pub latest: Option<LatestBackup>,
    pub last_success: Option<LatestBackup>,
    pub inventory_error: Option<String>,
    pub outcome: String,
    pub last_attempt: Option<BackupEvent>,
    pub last_failure: Option<BackupEvent>,
    pub next_scheduled_at: Option<i64>,
    pub scheduler_enabled: bool,
}

#[derive(FromRow)]
struct RecordedState {
    last_attempt_at: Option<i64>,
    outcome: String,
    last_failure_at: Option<i64>,
    last_failure_summary: Option<String>,
    last_success_at: Option<i64>,
    last_success_size: Option<i64>,
}

pub async fn status_at(state: &AppState, now: i64) -> Result<BackupStatus, AppError> {
    let row: RecordedState = sqlx::query_as("SELECT last_attempt_at, outcome, last_failure_at, last_failure_summary, last_success_at, last_success_size FROM server_backup_state WHERE id = 1")
        .fetch_one(&state.db).await?;
    let dir = state.paths.config_dir.join("backups");
    let inventory = tokio::task::spawn_blocking(move || super::latest(&dir))
        .await
        .map_err(|_| AppError::Unavailable("Backup inventory could not be read".into()))?;
    let inventory_error = inventory
        .as_ref()
        .err()
        .map(|_| "Backup directory is unavailable; check config storage and permissions".into());
    let latest = inventory.ok().flatten();
    let last_success = row
        .last_success_at
        .zip(row.last_success_size)
        .map(|(created_at, size)| LatestBackup {
            created_at,
            size: size.max(0) as u64,
        })
        .or_else(|| latest.map(|(created_at, size)| LatestBackup { created_at, size }));
    let interval_hours = state
        .settings
        .get_float(super::INTERVAL_HOURS, 24.0)
        .await?;
    let keep = state.settings.get_int(super::KEEP, 7).await?;
    let scheduler_enabled = state.demo.is_none() && interval_hours > 0.0;
    let next_scheduled_at = scheduler_enabled.then(|| {
        if row.outcome == "failed" {
            row.last_attempt_at
                .unwrap_or(now)
                .saturating_add(RETRY_SECONDS)
                .max(now)
        } else if inventory_error.is_some() {
            now
        } else {
            latest
                .map(|(stamp, _)| stamp.saturating_add((interval_hours * 3600.0).ceil() as i64))
                .unwrap_or(now)
                .max(now)
        }
    });
    Ok(BackupStatus {
        interval_hours,
        keep,
        latest: latest.map(|(created_at, size)| LatestBackup { created_at, size }),
        last_success,
        inventory_error,
        outcome: row.outcome,
        last_attempt: row
            .last_attempt_at
            .map(|at| BackupEvent { at, summary: None }),
        last_failure: row.last_failure_at.map(|at| BackupEvent {
            at,
            summary: row.last_failure_summary,
        }),
        next_scheduled_at,
        scheduler_enabled,
    })
}

pub async fn recover(state: &AppState) -> Result<(), AppError> {
    sqlx::query("UPDATE server_backup_state SET outcome = 'failed', last_failure_at = ?, last_failure_summary = 'The server stopped before the backup attempt finished' WHERE id = 1 AND outcome = 'running'")
        .bind(crate::server::now()).execute(&state.db).await?;
    Ok(())
}

pub async fn scheduler_tick_at(state: &AppState, now: i64) -> Result<(), AppError> {
    let _guard = state.server.backup_gate.lock().await;
    let status = status_at(state, now).await?;
    if status.next_scheduled_at.is_none_or(|at| at > now) || status.outcome == "running" {
        return Ok(());
    }
    sqlx::query(
        "UPDATE server_backup_state SET last_attempt_at = ?, outcome = 'running' WHERE id = 1",
    )
    .bind(now)
    .execute(&state.db)
    .await?;
    let dir = state.paths.config_dir.join("backups");
    // A retention failure retries cleanup without creating another fresh snapshot.
    let recent = status.latest.as_ref().is_some_and(|backup| {
        now.saturating_sub(backup.created_at) < (status.interval_hours * 3600.0).ceil() as i64
    });
    let mut cleaning_up = false;
    let result = async {
        if !recent {
            let path = super::create_at(&state.db, &dir, false, now).await?;
            let size = tokio::fs::metadata(path).await?.len() as i64;
            sqlx::query("UPDATE server_backup_state SET last_success_at = ?, last_success_size = ? WHERE id = 1")
                .bind(now).bind(size).execute(&state.db).await?;
        }
        cleaning_up = true;
        tokio::task::spawn_blocking(move || super::prune(&dir, status.keep as usize)).await
            .map_err(|_| AppError::Unavailable("Backup retention could not finish".into()))??;
        Ok::<(), AppError>(())
    }.await;
    match result {
        Ok(()) => {
            sqlx::query("UPDATE server_backup_state SET outcome = 'succeeded' WHERE id = 1")
                .execute(&state.db)
                .await?;
            tracing::info!("backup.scheduled");
            Ok(())
        }
        Err(error) => {
            let summary = if cleaning_up {
                "The backup is available, but old backups could not be removed. Check backup directory permissions"
            } else {
                "Could not complete the database backup. Check config storage, permissions, and server logs"
            };
            sqlx::query("UPDATE server_backup_state SET outcome = 'failed', last_failure_at = ?, last_failure_summary = ? WHERE id = 1")
                .bind(now).bind(summary).execute(&state.db).await?;
            Err(error)
        }
    }
}
