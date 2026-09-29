use std::time::Duration;

use bokhylle_acquisition::qbittorrent::{DEFAULT_CATEGORY, TAG_PREFIX};
use bokhylle_acquisition::state::AcquisitionStatus;
use serde::Serialize;

use crate::AppState;
use crate::acquisition;
use crate::error::AppError;
use crate::settings;

pub const ACTIVE_POLL_INTERVAL: Duration = Duration::from_secs(3);
pub const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(30);
pub const MISSING_DOWNLOAD_GRACE_SECONDS: i64 = 10 * 60;

#[derive(Debug, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TickReport {
    pub checked: usize,
    pub downloading: usize,
    pub completed: usize,
    pub failed: usize,
    /// Cancelled acquisitions whose client download was removed this tick.
    pub cleaned_up: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedState {
    Downloading,
    Completed,
    Error,
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            let active = match tick(&state).await {
                Ok(report) => report.checked,
                Err(error) => {
                    tracing::warn!(%error, "acquisition.tracker.failed");
                    0
                }
            };

            let interval = if active > 0 {
                ACTIVE_POLL_INTERVAL
            } else {
                IDLE_POLL_INTERVAL
            };
            tokio::time::sleep(interval).await;
        }
    });
}

pub async fn tick(state: &AppState) -> Result<TickReport, AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Ok(TickReport::default());
    };

    let rows: Vec<(String, Option<String>, String, i64)> = sqlx::query_as(
        "SELECT id, provider_download_id, status, created_at
         FROM acquisitions
         WHERE status IN ('QUEUED', 'DOWNLOADING')
           AND COALESCE(download_provider, '') NOT IN ('http', 'sabnzbd')
         ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await?;

    let mut report = TickReport {
        checked: rows.len(),
        ..Default::default()
    };

    for (id, provider_id, status, created_at) in rows {
        let Some(downloader) = factory.downloader(state).await? else {
            break;
        };

        let tag = format!("{TAG_PREFIX}{id}");
        let info = if let Some(provider_id) = provider_id.as_deref() {
            match downloader.status(provider_id).await {
                Ok(info) => info,
                Err(error) => {
                    tracing::warn!(
                        acquisition_id = %id,
                        %error,
                        "acquisition.tracker.provider_unavailable"
                    );
                    continue;
                }
            }
        } else {
            match downloader.find_by_tag(&tag).await {
                Ok(torrents) => torrents.into_iter().next(),
                Err(error) => {
                    tracing::warn!(
                        acquisition_id = %id,
                        %error,
                        "acquisition.tracker.provider_unavailable"
                    );
                    continue;
                }
            }
        };

        let Some(info) = info else {
            if status == "QUEUED" && now_epoch() - created_at < MISSING_DOWNLOAD_GRACE_SECONDS {
                continue;
            }

            acquisition::fail(
                &state.db,
                &id,
                "download_missing",
                "the download is no longer present in the download client",
            )
            .await?;
            report.failed += 1;
            continue;
        };

        if provider_id.is_none() {
            acquisition::set_provider(&state.db, &id, downloader.name(), Some(&info.hash)).await?;
        }

        if let Some(content_path) = info.content_path.clone()
            && crate::paths::is_within(
                &state.paths.downloads_dir,
                std::path::Path::new(&content_path),
            )
        {
            sqlx::query(
                "UPDATE acquisitions SET content_path = COALESCE(content_path, ?) WHERE id = ?",
            )
            .bind(content_path)
            .bind(&id)
            .execute(&state.db)
            .await?;
        }

        let previous = current_progress(state, &id).await?;
        let percent = (info.progress * 100.0).clamp(0.0, 100.0);
        acquisition::set_progress(&state.db, &id, percent).await?;
        let speed = info
            .dl_speed
            .filter(|speed| *speed > 0 && info.progress < 1.0);
        acquisition::set_speed(&state.db, &id, speed).await?;

        let crossed = [25, 50, 75, 100]
            .into_iter()
            .rfind(|milestone| previous < *milestone as f64 && percent >= *milestone as f64);
        if let Some(milestone) = crossed {
            tracing::info!(
                acquisition_id = %id,
                progress = percent,
                milestone,
                "acquisition.download.progress"
            );
        }

        match map_state(&info.state, info.progress) {
            MappedState::Error => {
                acquisition::fail(
                    &state.db,
                    &id,
                    "download_failed",
                    &format!("the download client reported state '{}'", info.state),
                )
                .await?;
                report.failed += 1;
            }
            MappedState::Completed => {
                if status == "QUEUED" {
                    acquisition::transition(&state.db, &id, AcquisitionStatus::Downloading, None)
                        .await?;
                }
                acquisition::transition(&state.db, &id, AcquisitionStatus::Downloaded, None)
                    .await?;
                tracing::info!(acquisition_id = %id, "acquisition.download.completed");
                crate::import_pipeline::spawn(state, id.clone());
                report.completed += 1;
            }
            MappedState::Downloading => {
                if status == "QUEUED" {
                    acquisition::transition(
                        &state.db,
                        &id,
                        AcquisitionStatus::Downloading,
                        Some(serde_json::json!({ "state": info.state })),
                    )
                    .await?;
                }
                report.downloading += 1;
            }
        }
    }

    report.cleaned_up = reconcile_cancellations(state).await?;

    Ok(report)
}

/// Retries external cleanup for CANCELLED acquisitions whose download could
/// not be removed when the user cancelled. The flag survives restarts, so
/// this is the recovery path; Needs Attention surfaces it if it stays stuck.
async fn reconcile_cancellations(state: &AppState) -> Result<usize, AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Ok(0);
    };
    let pending = acquisition::pending_cancellations(&state.db, 20).await?;
    if pending.is_empty() {
        return Ok(0);
    }
    let Some(downloader) = factory.downloader(state).await? else {
        return Ok(0);
    };
    let configured = state
        .settings
        .get_string(settings::QBITTORRENT_CATEGORY, DEFAULT_CATEGORY)
        .await
        .unwrap_or_else(|_| DEFAULT_CATEGORY.to_string());
    let category = match configured.trim() {
        "" => DEFAULT_CATEGORY,
        value => value,
    };

    let mut cleaned = 0;
    for (id, provider_id) in pending {
        let provider: Option<String> =
            sqlx::query_scalar("SELECT download_provider FROM acquisitions WHERE id = ?")
                .bind(&id)
                .fetch_optional(&state.db)
                .await?
                .flatten();
        if provider.as_deref() == Some("sabnzbd") {
            continue;
        }
        let Some(provider_id) = provider_id.as_deref() else {
            // Without an id there is nothing we can address; clear it rather
            // than keep an action the admin cannot resolve.
            acquisition::clear_cancel_pending(&state.db, &id).await?;
            continue;
        };
        match downloader.cancel_owned(provider_id, category, false).await {
            Ok(_) => {
                acquisition::clear_cancel_pending(&state.db, &id).await?;
                cleaned += 1;
                tracing::info!(acquisition_id = %id, "acquisition.cancel.reconciled");
            }
            Err(error) => {
                tracing::warn!(acquisition_id = %id, %error, "acquisition.cancel.retry_failed");
            }
        }
    }
    Ok(cleaned)
}

async fn current_progress(state: &AppState, id: &str) -> Result<f64, AppError> {
    let progress: Option<f64> =
        sqlx::query_scalar("SELECT progress FROM acquisitions WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    Ok(progress.unwrap_or(0.0))
}

pub fn map_state(state: &str, progress: f64) -> MappedState {
    match state {
        "error" | "missingFiles" => MappedState::Error,
        "uploading" | "stalledUP" | "forcedUP" | "queuedUP" | "pausedUP" | "checkingUP" => {
            MappedState::Completed
        }
        _ if progress >= 1.0 => MappedState::Completed,
        _ => MappedState::Downloading,
    }
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_qbittorrent_states() {
        assert_eq!(map_state("downloading", 0.4), MappedState::Downloading);
        assert_eq!(map_state("stalledDL", 0.4), MappedState::Downloading);
        assert_eq!(map_state("metaDL", 0.0), MappedState::Downloading);
        assert_eq!(map_state("uploading", 1.0), MappedState::Completed);
        assert_eq!(map_state("stalledUP", 1.0), MappedState::Completed);
        assert_eq!(map_state("pausedDL", 1.0), MappedState::Completed);
        assert_eq!(map_state("error", 0.3), MappedState::Error);
        assert_eq!(map_state("missingFiles", 0.3), MappedState::Error);
    }
}
