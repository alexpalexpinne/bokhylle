//! Durable Newznab → SABnzbd retrieval. SAB's completed path is never used
//! until it resolves inside the configured downloads directory.

use std::{path::Path, time::Duration};

use bokhylle_acquisition::{
    model::EvaluatedRelease,
    sabnzbd::{NzbStatus, SabnzbdError},
    state::AcquisitionStatus,
};
use serde_json::json;

use crate::{AppState, acquisition, error::AppError, settings};

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = tick(&state).await {
                tracing::warn!(%error, "nzb_acquisition.tick_failed");
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}

pub async fn queue(state: &AppState, id: &str, release: &EvaluatedRelease) -> Result<(), AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Err(AppError::Unavailable(
            "acquisition providers are not configured".into(),
        ));
    };
    let Some(indexer) = factory.indexer_named(state, "newznab").await? else {
        acquisition::fail(
            &state.db,
            id,
            "integrations_not_configured",
            "Newznab is not configured",
        )
        .await?;
        return Ok(());
    };
    if factory.nzb_downloader(state).await?.is_none() {
        acquisition::fail(
            &state.db,
            id,
            "integrations_not_configured",
            "SABnzbd is not configured",
        )
        .await?;
        return Ok(());
    }
    let url = indexer
        .nzb_url(&release.candidate)
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    let job_name = format!("bokhylle-{id}-{}", uuid::Uuid::new_v4());
    let current = acquisition::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".into()))?;
    let mut tx = state.db.begin().await?;
    // Claim the state before replacing input. A second selection or a
    // cancellation rolls the entire transaction back on a failed CAS.
    acquisition::transition_tx(
        &mut tx,
        id,
        current.status()?,
        AcquisitionStatus::Queued,
        Some(json!({ "releaseName": release.candidate.title })),
    )
    .await?;
    acquisition::set_selected_tx(&mut tx, id, release).await?;
    acquisition::insert_event_tx(
        &mut tx,
        id,
        "acquisition.release.selected",
        Some(json!({
            "releaseName": release.candidate.title,
            "indexer": release.candidate.indexer,
            "score": release.score,
            "confidence": release.confidence,
        })),
    )
    .await?;
    sqlx::query("INSERT INTO nzb_inputs (acquisition_id, url, job_name) VALUES (?, ?, ?) ON CONFLICT(acquisition_id) DO UPDATE SET url = excluded.url, job_name = excluded.job_name, submitted_at = NULL, created_at = unixepoch()")
        .bind(id).bind(&url).bind(&job_name).execute(&mut *tx).await?;
    sqlx::query("UPDATE acquisitions SET download_provider = 'sabnzbd', provider_download_id = NULL, content_path = NULL, progress = 0, cancel_pending = 0 WHERE id = ?")
        .bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    run(state, id).await
}

pub async fn tick(state: &AppState) -> Result<usize, AppError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM acquisitions WHERE download_provider = 'sabnzbd' AND status IN ('QUEUED', 'DOWNLOADING') ORDER BY created_at",
    ).fetch_all(&state.db).await?;
    for id in &ids {
        if let Err(error) = run(state, id).await {
            tracing::warn!(acquisition_id = %id, %error, "nzb_acquisition.poll_failed");
        }
    }
    let pending: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM acquisitions WHERE download_provider = 'sabnzbd' AND status = 'CANCELLED' AND cancel_pending = 1 ORDER BY updated_at LIMIT 100",
    ).fetch_all(&state.db).await?;
    for id in pending {
        if let Err(error) = reconcile_cancel(state, &id).await {
            tracing::warn!(acquisition_id = %id, %error, "nzb_acquisition.cancel_failed");
        }
    }
    Ok(ids.len())
}

pub async fn reconcile_cancel(state: &AppState, id: &str) -> Result<(), AppError> {
    let key = format!("nzb-{id}");
    if !state.pipeline.try_acquire(&key) {
        return Ok(());
    }
    let result = reconcile_cancel_locked(state, id).await;
    state.pipeline.release(&key);
    result
}

async fn reconcile_cancel_locked(state: &AppState, id: &str) -> Result<(), AppError> {
    let Some(current) = acquisition::get(&state.db, id).await? else {
        return Ok(());
    };
    if current.status()? != AcquisitionStatus::Cancelled
        || current.download_provider.as_deref() != Some("sabnzbd")
    {
        return Ok(());
    }
    if current.content_path.is_some() {
        return acquisition::clear_cancel_pending(&state.db, id).await;
    }
    let Some(factory) = state.providers.as_ref() else {
        return Ok(());
    };
    let Some(client) = factory.nzb_downloader(state).await? else {
        return Ok(());
    };
    let provider_id = if let Some(provider_id) = current.provider_download_id {
        Some(provider_id)
    } else {
        let input: Option<(String, Option<i64>)> = sqlx::query_as(
            "SELECT job_name, submitted_at FROM nzb_inputs WHERE acquisition_id = ?",
        )
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
        let Some((job_name, submitted_at)) = input else {
            return acquisition::clear_cancel_pending(&state.db, id).await;
        };
        let found = client
            .find_by_name(&job_name)
            .await
            .map_err(|error| AppError::Unavailable(error.to_string()))?;
        if found.is_none() && submitted_at.is_some_and(|at| at + 10 * 60 > now_epoch()) {
            // addurl can still be fetching the NZB after its response was lost.
            return Ok(());
        }
        if let Some(provider_id) = found.as_deref() {
            acquisition::set_provider(&state.db, id, "sabnzbd", Some(provider_id)).await?;
        }
        found
    };
    if let Some(provider_id) = provider_id {
        client
            .cancel(&provider_id)
            .await
            .map_err(|error| AppError::Unavailable(error.to_string()))?;
    }
    acquisition::clear_cancel_pending(&state.db, id).await
}

pub async fn run(state: &AppState, id: &str) -> Result<(), AppError> {
    let key = format!("nzb-{id}");
    if !state.pipeline.try_acquire(&key) {
        return Ok(());
    }
    let result = run_locked(state, id).await;
    state.pipeline.release(&key);
    result
}

async fn run_locked(state: &AppState, id: &str) -> Result<(), AppError> {
    let Some(current) = acquisition::get(&state.db, id).await? else {
        return Ok(());
    };
    if !matches!(
        current.status()?,
        AcquisitionStatus::Queued | AcquisitionStatus::Downloading
    ) {
        return Ok(());
    }
    let Some(factory) = state.providers.as_ref() else {
        return Ok(());
    };
    let Some(client) = factory.nzb_downloader(state).await? else {
        return Ok(());
    };
    let Some((url, job_name, submitted_at)): Option<(String, String, Option<i64>)> =
        sqlx::query_as(
            "SELECT url, job_name, submitted_at FROM nzb_inputs WHERE acquisition_id = ?",
        )
        .bind(id)
        .fetch_optional(&state.db)
        .await?
    else {
        return Ok(());
    };

    let provider_id = if let Some(provider_id) = current.provider_download_id.as_ref() {
        provider_id.clone()
    } else {
        let provider_id = match client.find_by_name(&job_name).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                if url.is_empty() {
                    acquisition::fail(&state.db, id, "download_input_redacted",
                        "The private NZB submission input was omitted from this backup. Configure Newznab and SABnzbd, then try again.").await?;
                    return Ok(());
                }
                if let Some(at) = submitted_at {
                    if at + 10 * 60 <= now_epoch() {
                        acquisition::fail(&state.db, id, "download_submission_unknown",
                            "SABnzbd submission could not be recovered; check the client before trying again").await?;
                    }
                    return Ok(());
                }
                let category = state
                    .settings
                    .get_string(settings::SABNZBD_CATEGORY, "books-app")
                    .await?;
                let claimed = sqlx::query("UPDATE nzb_inputs SET submitted_at = unixepoch() WHERE acquisition_id = ? AND job_name = ? AND submitted_at IS NULL AND EXISTS (SELECT 1 FROM acquisitions WHERE id = ? AND status IN ('QUEUED', 'DOWNLOADING'))")
                    .bind(id).bind(&job_name).bind(id).execute(&state.db).await?;
                if claimed.rows_affected() == 0 {
                    return Ok(());
                }
                match client.add_url(&url, category.trim(), &job_name).await {
                    Ok(id) => id,
                    Err(error) => {
                        if !matches!(error, SabnzbdError::Rejected | SabnzbdError::Configuration) {
                            // A timeout can mean SAB accepted the job. Keep
                            // its durable name and reconcile before resubmitting.
                            return Err(AppError::Unavailable(error.to_string()));
                        }
                        acquisition::fail(
                            &state.db,
                            id,
                            "download_failed",
                            &format!("SABnzbd could not add the NZB: {error}"),
                        )
                        .await?;
                        return Ok(());
                    }
                }
            }
            Err(error) => return Err(AppError::Unavailable(error.to_string())),
        };
        // Save the id even if cancellation won the race, so failed external
        // cancellation remains recoverable on the next tick or restart.
        let saved = sqlx::query("UPDATE acquisitions SET provider_download_id = ?, updated_at = unixepoch() WHERE id = ? AND download_provider = 'sabnzbd' AND status IN ('QUEUED', 'DOWNLOADING', 'CANCELLED') AND EXISTS (SELECT 1 FROM nzb_inputs WHERE acquisition_id = ? AND job_name = ?)")
            .bind(&provider_id).bind(id).bind(id).bind(&job_name).execute(&state.db).await?;
        if saved.rows_affected() == 0 {
            let _ = client.cancel(&provider_id).await;
            return Ok(());
        }
        let Some(latest) = acquisition::get(&state.db, id).await? else {
            return Ok(());
        };
        if latest.status()? == AcquisitionStatus::Cancelled {
            if client.cancel(&provider_id).await.is_err() {
                acquisition::mark_cancel_pending(&state.db, id).await?;
            } else {
                acquisition::clear_cancel_pending(&state.db, id).await?;
            }
            return Ok(());
        }
        acquisition::log_event(
            &state.db,
            id,
            "acquisition.download.queued",
            Some(json!({
                "provider": client.name(), "providerDownloadId": provider_id,
            })),
        )
        .await?;
        provider_id
    };

    match client
        .status(&provider_id)
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))?
    {
        NzbStatus::Queued { progress } => {
            acquisition::set_progress(&state.db, id, progress).await?;
            if current.status()? == AcquisitionStatus::Queued {
                acquisition::transition(&state.db, id, AcquisitionStatus::Downloading, None)
                    .await?;
            }
        }
        NzbStatus::Completed { path } => {
            let path = Path::new(&path);
            if !crate::paths::is_within(&state.paths.downloads_dir, path) {
                acquisition::fail(
                    &state.db,
                    id,
                    "download_path_invalid",
                    "SABnzbd completed outside the configured downloads directory",
                )
                .await?;
                return Ok(());
            }
            sqlx::query("UPDATE acquisitions SET content_path = ? WHERE id = ?")
                .bind(path.to_string_lossy().as_ref())
                .bind(id)
                .execute(&state.db)
                .await?;
            acquisition::set_progress(&state.db, id, 100.0).await?;
            if current.status()? == AcquisitionStatus::Queued {
                acquisition::transition(&state.db, id, AcquisitionStatus::Downloading, None)
                    .await?;
            }
            acquisition::transition(&state.db, id, AcquisitionStatus::Downloaded, None).await?;
            crate::import_pipeline::spawn(state, id.to_owned());
        }
        NzbStatus::Failed => {
            acquisition::fail(
                &state.db,
                id,
                "download_failed",
                "SABnzbd reported a failed download",
            )
            .await?;
        }
        NzbStatus::Missing => {
            if current.provider_download_id.is_some() && current.updated_at + 10 * 60 < now_epoch()
            {
                acquisition::fail(
                    &state.db,
                    id,
                    "download_missing",
                    "the NZB is no longer present in SABnzbd",
                )
                .await?;
            }
        }
    }
    Ok(())
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
