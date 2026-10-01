use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use futures_core::Stream;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};
use sqlx::{Connection, SqlitePool};
use tokio_util::io::ReaderStream;

use crate::AppState;
use crate::error::AppError;
use crate::settings;

mod status;
pub use status::{BackupStatus, recover, scheduler_tick_at, status_at};

pub const INTERVAL_HOURS: &str = "backups.interval_hours";
pub const KEEP: &str = "backups.keep";

/// Scheduled backups stay inside the config directory, so they keep secrets and
/// can be restored as-is. The downloadable copy lives outside that trust
/// boundary and has secret settings removed.
pub async fn create(
    pool: &SqlitePool,
    dir: &Path,
    redact_secrets: bool,
) -> Result<PathBuf, AppError> {
    create_at(pool, dir, redact_secrets, now_epoch()).await
}

pub async fn create_at(
    pool: &SqlitePool,
    dir: &Path,
    redact_secrets: bool,
    stamp: i64,
) -> Result<PathBuf, AppError> {
    tokio::fs::create_dir_all(dir).await?;
    let target = dir.join(format!("bokhylle-{stamp}.db"));
    let staging = dir.join(format!(
        ".bokhylle-{stamp}-{}.partial",
        uuid::Uuid::new_v4()
    ));

    let result = async {
        sqlx::query("VACUUM INTO ?")
            .bind(staging.to_string_lossy().as_ref())
            .execute(pool)
            .await?;
        if redact_secrets {
            redact(&staging).await?;
        }
        verify(&staging).await?;
        // Publishing a hard link cannot replace a previous valid snapshot.
        tokio::fs::hard_link(&staging, &target).await?;
        Ok::<(), AppError>(())
    }
    .await;
    if let Err(error) = tokio::fs::remove_file(&staging).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, path = %staging.display(), "backup.staging.cleanup_failed");
    }
    result?;

    Ok(target)
}

/// Check the standalone snapshot before it is offered for download or rotation.
pub async fn verify(path: &Path) -> Result<(), AppError> {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .foreign_keys(true),
    )
    .await?;
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut connection)
        .await?;
    if integrity.len() != 1 || integrity[0] != "ok" {
        return Err(AppError::Unavailable(format!(
            "backup integrity check failed: {}",
            integrity.join("; ")
        )));
    }
    let foreign_keys: Vec<(String, i64, String, i64)> = sqlx::query_as("PRAGMA foreign_key_check")
        .fetch_all(&mut connection)
        .await?;
    if !foreign_keys.is_empty() {
        return Err(AppError::Unavailable(
            "backup foreign key check failed".to_string(),
        ));
    }
    connection.close().await?;
    Ok(())
}

/// Stream a downloadable backup without holding the database in memory. The
/// temporary copy is removed when the response completes or is cancelled.
pub async fn stream_download(path: PathBuf) -> Result<(Body, u64), AppError> {
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(error) => {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(error.into());
        }
    };
    let length = match file.metadata().await {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            drop(file);
            let _ = tokio::fs::remove_file(&path).await;
            return Err(error.into());
        }
    };
    Ok((
        Body::from_stream(RemoveFileOnDrop {
            inner: Some(ReaderStream::new(file)),
            path: Some(path),
        }),
        length,
    ))
}

struct RemoveFileOnDrop<S> {
    inner: Option<S>,
    path: Option<PathBuf>,
}

impl<S: Stream + Unpin> Stream for RemoveFileOnDrop<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        Pin::new(this.inner.as_mut().expect("stream is present until drop")).poll_next(cx)
    }
}

impl<S> Drop for RemoveFileOnDrop<S> {
    fn drop(&mut self) {
        // Close the handle before unlinking, including when a client cancels.
        self.inner.take();
        let Some(path) = self.path.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = tokio::fs::remove_file(&path).await {
                    tracing::warn!(%error, path = %path.display(), "backup.download.cleanup_failed");
                }
            });
        } else if let Err(error) = std::fs::remove_file(&path) {
            tracing::warn!(%error, path = %path.display(), "backup.download.cleanup_failed");
        }
    }
}

async fn redact(path: &Path) -> Result<(), AppError> {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false),
    )
    .await?;

    // Deleting rows alone can leave credential bytes in free SQLite pages.
    sqlx::query("PRAGMA secure_delete = ON")
        .execute(&mut connection)
        .await?;

    let placeholders = vec!["?"; settings::SECRET_KEYS.len()].join(",");
    let query = format!("DELETE FROM settings WHERE key IN ({placeholders})");
    let mut delete = sqlx::query(sqlx::AssertSqlSafe(query));
    for key in settings::SECRET_KEYS {
        delete = delete.bind(key);
    }
    delete.execute(&mut connection).await?;
    // Selected NZB URLs contain an indexer API key too. Preserve the job name
    // and provider id for reconciliation, but omit submission credentials
    // from downloadable backups. Local scheduled backups retain them.
    sqlx::query("UPDATE nzb_inputs SET url = ''")
        .execute(&mut connection)
        .await?;
    sqlx::query("VACUUM").execute(&mut connection).await?;
    connection.close().await?;
    Ok(())
}

pub fn prune(dir: &Path, keep: usize) -> Result<(), AppError> {
    let mut stamps = list(dir)?;
    while stamps.len() > keep.max(1) {
        let (stamp, _) = stamps.remove(0);
        match std::fs::remove_file(dir.join(format!("bokhylle-{stamp}.db"))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub fn latest(dir: &Path) -> Result<Option<(i64, u64)>, AppError> {
    Ok(list(dir)?.pop())
}

fn list(dir: &Path) -> Result<Vec<(i64, u64)>, AppError> {
    let mut entries = Vec::new();
    let read_dir = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
        Err(error) => return Err(error.into()),
    };
    for entry in read_dir {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(stamp) = name
            .strip_prefix("bokhylle-")
            .and_then(|rest| rest.strip_suffix(".db"))
            .and_then(|value| value.parse::<i64>().ok())
        else {
            continue;
        };
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let size = metadata.len();
        entries.push((stamp, size));
    }
    entries.sort_unstable();
    Ok(entries)
}

pub fn spawn_scheduler(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = recover(&state).await {
            tracing::warn!(%error, "backup.recovery.failed");
        }
        loop {
            let notified = state.server.backup_wakeup.notified();
            if let Err(error) = scheduler_tick_at(&state, crate::server::now()).await {
                tracing::warn!(%error, "backup.scheduled.failed");
            }
            let delay = status_at(&state, crate::server::now())
                .await
                .ok()
                .and_then(|status| status.next_scheduled_at)
                .map(|at| at.saturating_sub(crate::server::now()).max(1) as u64)
                .unwrap_or(300);
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(delay)) => {},
                _ = notified => {},
            }
        }
    });
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
