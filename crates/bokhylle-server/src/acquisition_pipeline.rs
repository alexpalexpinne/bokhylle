use std::collections::HashSet;
use std::sync::Mutex;

use bokhylle_acquisition::evaluator;
use bokhylle_acquisition::model::{AcquisitionMethod, EvaluatedRelease, ExpectedBook, Selection};
use bokhylle_acquisition::provider::DownloadSource;
use bokhylle_acquisition::qbittorrent::{DEFAULT_CATEGORY, TAG_PREFIX, TorrentInfo};
use bokhylle_acquisition::state::AcquisitionStatus;
use serde_json::json;

use crate::AppState;
use crate::acquisition;
use crate::error::AppError;
use crate::settings;

#[derive(Default)]
pub struct PipelineGuard {
    inner: Mutex<HashSet<String>>,
}

impl PipelineGuard {
    pub fn try_acquire(&self, id: &str) -> bool {
        self.inner
            .lock()
            .expect("pipeline lock")
            .insert(id.to_string())
    }

    pub fn release(&self, id: &str) {
        self.inner.lock().expect("pipeline lock").remove(id);
    }
}

pub fn spawn(state: &AppState, acquisition_id: String) {
    if !state.pipeline.try_acquire(&acquisition_id) {
        return;
    }

    let state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = run(&state, &acquisition_id).await {
            tracing::warn!(
                acquisition_id = %acquisition_id,
                %error,
                "acquisition.pipeline.failed"
            );
        }
        state.pipeline.release(&acquisition_id);
    });
}

pub async fn recover(state: &AppState) {
    let ids: Vec<String> = match sqlx::query_scalar(
        "SELECT a.id FROM acquisitions a
         WHERE a.status IN ('REQUESTED', 'SEARCHING', 'EVALUATING')
            OR (a.status IN ('QUEUED', 'DOWNLOADING')
                AND EXISTS (SELECT 1 FROM acquisition_inputs i WHERE i.acquisition_id = a.id))
         ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(ids) => ids,
        Err(error) => {
            tracing::error!(%error, "acquisition.recovery.failed");
            return;
        }
    };

    for id in ids {
        tracing::info!(acquisition_id = %id, "acquisition.recovered");
        spawn(state, id);
    }

    let awaiting: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM acquisitions WHERE status IN ('QUEUED', 'DOWNLOADING')",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    if awaiting > 0 {
        tracing::info!(awaiting, "acquisition.awaiting_reconciler");
    }
}

pub async fn run(state: &AppState, acquisition_id: &str) -> Result<(), AppError> {
    let Some(acquisition) = acquisition::get(&state.db, acquisition_id).await? else {
        return Ok(());
    };

    if let Some(input) = crate::http_acquisition::input(state, acquisition_id).await? {
        return crate::http_acquisition::run(state, acquisition_id, input).await;
    }

    // Torrent requests have historically remained REQUESTED when integrations
    // are disabled. HTTP jobs carry their own durable retrieval instruction.
    let Some(factory) = state.providers.as_ref() else {
        return Ok(());
    };

    let status = acquisition.status()?;
    if !matches!(
        status,
        AcquisitionStatus::Requested | AcquisitionStatus::Searching | AcquisitionStatus::Evaluating
    ) {
        return Ok(());
    }

    if status == AcquisitionStatus::Requested {
        acquisition::transition(
            &state.db,
            acquisition_id,
            AcquisitionStatus::Searching,
            None,
        )
        .await?;
    }

    let Some(indexer) = factory.indexer(state).await? else {
        acquisition::fail(
            &state.db,
            acquisition_id,
            "integrations_not_configured",
            "the indexer is not configured",
        )
        .await?;
        return Ok(());
    };

    let book = load_expected_book(state, acquisition.book_id, &acquisition).await?;

    let outcome = match indexer.search_book(&book).await {
        Ok(outcome) => outcome,
        Err(error) => {
            acquisition::fail(
                &state.db,
                acquisition_id,
                "search_failed",
                &format!("release search failed: {error}"),
            )
            .await?;
            return Ok(());
        }
    };

    if !still_active(state, acquisition_id).await? {
        return Ok(());
    }

    acquisition::transition(
        &state.db,
        acquisition_id,
        AcquisitionStatus::Evaluating,
        None,
    )
    .await?;

    let evaluated = evaluator::rank(&book, &outcome.candidates);
    // Releases that already failed to import for this library are excluded so
    // a retry picks the next candidate instead of the same broken file.
    let blocked = acquisition::blocked_release_keys(&state.db)
        .await
        .unwrap_or_default();
    let evaluated = filter_blocked(evaluated, &blocked);
    let ask_before_download = acquisition::get(&state.db, acquisition_id)
        .await?
        .map(|acquisition| acquisition.ask_before_download)
        .unwrap_or(false);
    let selection = if ask_before_download {
        match evaluator::select(&evaluated) {
            Selection::None => Selection::None,
            _ => Selection::NeedsSelection,
        }
    } else {
        evaluator::select(&evaluated)
    };

    acquisition::log_event(
        &state.db,
        acquisition_id,
        "acquisition.candidates.evaluated",
        Some(json!({
            "queries": outcome.queries,
            "candidates": evaluated,
        })),
    )
    .await?;

    match selection {
        Selection::None => {
            acquisition::transition(
                &state.db,
                acquisition_id,
                AcquisitionStatus::NoReleaseFound,
                Some(json!({ "candidates": evaluated.len() })),
            )
            .await?;
            crate::notifications::for_requesters(
                &state.db,
                acquisition_id,
                "failed",
                "No suitable copy found",
                None,
            )
            .await
            .ok();
            // Keep looking: the intent survives the empty search result.
            crate::keep_looking::schedule_after_failure(&state.db, acquisition_id)
                .await
                .ok();
        }
        Selection::NeedsSelection => {
            acquisition::transition(
                &state.db,
                acquisition_id,
                AcquisitionStatus::NeedsSelection,
                Some(json!({ "candidates": evaluated.len() })),
            )
            .await?;
            crate::notifications::for_requesters(
                &state.db,
                acquisition_id,
                "needs_selection",
                "Choose a version",
                None,
            )
            .await
            .ok();
            tracing::info!(acquisition_id, "acquisition.selection.required");
        }
        Selection::Auto { index } => {
            queue_release(state, acquisition_id, &evaluated[index]).await?;
        }
    }

    Ok(())
}

pub async fn select_candidate(
    state: &AppState,
    acquisition_id: &str,
    index: usize,
) -> Result<(), AppError> {
    let Some(acquisition) = acquisition::get(&state.db, acquisition_id).await? else {
        return Err(AppError::NotFound("acquisition not found".to_string()));
    };

    if acquisition.status()? != AcquisitionStatus::NeedsSelection {
        return Err(AppError::Conflict(
            "acquisition is not awaiting a selection".to_string(),
        ));
    }

    let Some(detail) = acquisition::latest_event_detail(
        &state.db,
        acquisition_id,
        "acquisition.candidates.evaluated",
    )
    .await?
    else {
        return Err(AppError::Conflict(
            "no candidate list is available for this acquisition".to_string(),
        ));
    };

    let evaluated = acquisition::evaluated_candidates(&detail);

    let Some(release) = evaluated.get(index) else {
        return Err(AppError::BadRequest(
            "candidate index is out of range".to_string(),
        ));
    };

    if release.rejected() {
        return Err(AppError::BadRequest(
            "the selected candidate was rejected by the evaluator".to_string(),
        ));
    }

    queue_release(state, acquisition_id, release).await
}

async fn queue_release(
    state: &AppState,
    acquisition_id: &str,
    release: &EvaluatedRelease,
) -> Result<(), AppError> {
    let magnet_url = match &release.candidate.method {
        Some(AcquisitionMethod::Torrent { magnet_url, .. }) => magnet_url.as_deref(),
        Some(AcquisitionMethod::Http { .. }) => {
            return Err(AppError::Unprocessable(
                "HTTP candidates use the direct retrieval path".to_string(),
            ));
        }
        Some(AcquisitionMethod::Nzb { .. }) => {
            return crate::nzb_acquisition::queue(state, acquisition_id, release).await;
        }
        // Historical candidate events predate the method field and must
        // remain selectable after an upgrade.
        None => release.candidate.magnet_url.as_deref(),
    };
    let Some(factory) = state.providers.as_ref() else {
        return Err(AppError::Unavailable(
            "acquisition providers are not configured".to_string(),
        ));
    };

    let Some(downloader) = factory.downloader(state).await? else {
        acquisition::fail(
            &state.db,
            acquisition_id,
            "integrations_not_configured",
            "the download client is not configured",
        )
        .await?;
        return Ok(());
    };

    let category = match state
        .settings
        .get_string(settings::QBITTORRENT_CATEGORY, DEFAULT_CATEGORY)
        .await?
        .trim()
        .to_string()
    {
        value if value.is_empty() => DEFAULT_CATEGORY.to_string(),
        value => value,
    };
    let tag = format!("{TAG_PREFIX}{acquisition_id}");

    acquisition::set_selected(&state.db, acquisition_id, release).await?;
    acquisition::log_event(
        &state.db,
        acquisition_id,
        "acquisition.release.selected",
        Some(json!({
            "releaseName": release.candidate.title,
            "indexer": release.candidate.indexer,
            "score": release.score,
            "confidence": release.confidence,
        })),
    )
    .await?;

    let magnet_hash = match magnet_url
        .map(str::trim)
        .filter(|magnet| !magnet.is_empty())
    {
        Some(magnet) => {
            let hash = magnet_hash(magnet);
            let source = DownloadSource::Magnet(magnet.to_string());
            if let Err(error) = downloader.add(source, &category, &tag).await {
                acquisition::fail(
                    &state.db,
                    acquisition_id,
                    "download_failed",
                    &format!("could not add the download: {error}"),
                )
                .await?;
                return Ok(());
            }
            hash
        }
        None => {
            let source_kind = release
                .candidate
                .source
                .as_ref()
                .map(|source| source.kind.as_str());
            let indexer = match source_kind {
                Some(kind @ ("prowlarr" | "torznab")) => factory.indexer_named(state, kind).await?,
                _ => factory.indexer(state).await?,
            };
            let Some(indexer) = indexer else {
                acquisition::fail(
                    &state.db,
                    acquisition_id,
                    "integrations_not_configured",
                    "no indexer is available to fetch the torrent file",
                )
                .await?;
                return Ok(());
            };

            let bytes = match indexer.fetch_torrent(&release.candidate).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    acquisition::fail(
                        &state.db,
                        acquisition_id,
                        "download_failed",
                        &format!("could not fetch the torrent file: {error}"),
                    )
                    .await?;
                    return Ok(());
                }
            };

            let source = DownloadSource::TorrentFile {
                bytes,
                filename: format!("{}.torrent", sanitize_filename(&release.candidate.title)),
            };

            if let Err(error) = downloader.add(source, &category, &tag).await {
                acquisition::fail(
                    &state.db,
                    acquisition_id,
                    "download_failed",
                    &format!("could not add the download: {error}"),
                )
                .await?;
                return Ok(());
            }

            None
        }
    };

    let provider_id = match magnet_hash {
        Some(hash) => Some(hash),
        None => downloader
            .find_by_tag(&tag)
            .await
            .ok()
            .and_then(|torrents| pick_torrent(&torrents, &release.candidate.title).cloned())
            .map(|torrent| torrent.hash),
    };

    acquisition::set_provider(
        &state.db,
        acquisition_id,
        downloader.name(),
        provider_id.as_deref(),
    )
    .await?;

    acquisition::log_event(
        &state.db,
        acquisition_id,
        "acquisition.download.queued",
        Some(json!({
            "provider": downloader.name(),
            "providerDownloadId": provider_id,
        })),
    )
    .await?;

    if let Err(error) = acquisition::transition(
        &state.db,
        acquisition_id,
        AcquisitionStatus::Queued,
        Some(json!({ "releaseName": release.candidate.title })),
    )
    .await
    {
        // The acquisition changed under us (typically a concurrent cancel
        // between the active check and the add): remove the download we just
        // created instead of leaving it running.
        if let Some(provider_id) = provider_id.as_deref() {
            match downloader.cancel_owned(provider_id, &category, false).await {
                Ok(_) => {}
                Err(cancel_error) => {
                    acquisition::mark_cancel_pending(&state.db, acquisition_id).await?;
                    tracing::warn!(
                        acquisition_id,
                        %cancel_error,
                        "acquisition.queue.cleanup_failed"
                    );
                }
            }
        } else {
            acquisition::mark_cancel_pending(&state.db, acquisition_id).await?;
        }
        return Err(error);
    }

    tracing::info!(acquisition_id, "acquisition.download.queued");
    Ok(())
}

/// Drops releases whose fingerprint has failed before. Kept separate so the
/// behaviour is unit-testable without a database.
pub(crate) fn filter_blocked(
    evaluated: Vec<bokhylle_acquisition::model::EvaluatedRelease>,
    blocked: &std::collections::HashSet<String>,
) -> Vec<bokhylle_acquisition::model::EvaluatedRelease> {
    if blocked.is_empty() {
        return evaluated;
    }
    evaluated
        .into_iter()
        .filter(|release| {
            !blocked.contains(&bokhylle_acquisition::evaluator::release_key(
                &release.candidate,
            ))
        })
        .collect()
}

pub async fn load_expected_book(
    state: &AppState,
    book_id: i64,
    acquisition: &acquisition::Acquisition,
) -> Result<ExpectedBook, AppError> {
    let book: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT title, language FROM books WHERE id = ?")
            .bind(book_id)
            .fetch_optional(&state.db)
            .await?;

    let Some((title, book_language)) = book else {
        return Err(AppError::NotFound("book not found".to_string()));
    };

    let authors: Vec<String> = sqlx::query_scalar(
        "SELECT a.name
         FROM book_authors ba
         JOIN authors a ON a.id = ba.author_id
         WHERE ba.book_id = ?
         ORDER BY ba.position, a.name",
    )
    .bind(book_id)
    .fetch_all(&state.db)
    .await?;

    let isbn: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(isbn13, isbn10)
         FROM editions
         WHERE book_id = ? AND (isbn13 IS NOT NULL OR isbn10 IS NOT NULL)
         ORDER BY id
         LIMIT 1",
    )
    .bind(book_id)
    .fetch_optional(&state.db)
    .await?
    .flatten();

    let year: Option<i64> =
        sqlx::query_scalar("SELECT min(publication_year) FROM editions WHERE book_id = ?")
            .bind(book_id)
            .fetch_one(&state.db)
            .await?;

    let series_number: Option<String> =
        sqlx::query_scalar("SELECT series_number FROM books WHERE id = ?")
            .bind(book_id)
            .fetch_one(&state.db)
            .await?;

    let preferred_format = match &acquisition.preferred_format {
        Some(format) if !format.trim().is_empty() => Some(format.clone()),
        _ => Some(
            state
                .settings
                .get_string(settings::PREFERRED_FORMAT, "epub")
                .await?,
        ),
    };

    let preferred_language = match &acquisition.preferred_language {
        Some(language) if !language.trim().is_empty() => Some(language.clone()),
        _ => {
            let configured = state
                .settings
                .get_string(settings::PREFERRED_LANGUAGE, "en")
                .await?;
            (!configured.trim().is_empty()).then_some(configured)
        }
    };

    // The accepted languages are frozen on the acquisition at creation, so a
    // retry executes the original request rather than today's profile. Legacy
    // rows (NULL) still resolve from the reader's current profile.
    let languages = match acquisition::stored_languages(acquisition) {
        Some(stored) => stored,
        None => {
            let mut legacy: Vec<String> = Vec::new();
            if let Some(user_id) = acquisition.user_id {
                let stored: Option<String> =
                    sqlx::query_scalar("SELECT preferred_languages FROM users WHERE id = ?")
                        .bind(user_id)
                        .fetch_optional(&state.db)
                        .await?
                        .flatten();
                if let Some(stored) = stored
                    && let Ok(values) = serde_json::from_str::<Vec<String>>(&stored)
                {
                    legacy = values
                        .into_iter()
                        .map(|value| value.trim().to_ascii_lowercase())
                        .filter(|value| !value.is_empty())
                        .collect();
                }
            }
            if legacy.is_empty() {
                legacy.extend(preferred_language.clone());
            }
            legacy
        }
    };

    Ok(ExpectedBook {
        languages,
        title,
        authors,
        year: year.map(|year| year as i32),
        isbn,
        language: preferred_language.or(book_language),
        preferred_format,
        series_number,
    })
}

async fn still_active(state: &AppState, acquisition_id: &str) -> Result<bool, AppError> {
    let Some(acquisition) = acquisition::get(&state.db, acquisition_id).await? else {
        return Ok(false);
    };
    Ok(!acquisition.status()?.is_terminal())
}

fn pick_torrent<'a>(torrents: &'a [TorrentInfo], title: &str) -> Option<&'a TorrentInfo> {
    torrents
        .iter()
        .find(|torrent| torrent.name == title)
        .or_else(|| torrents.first())
}

fn magnet_hash(magnet: &str) -> Option<String> {
    let position = magnet.find("xt=urn:btih:")?;
    let rest = &magnet[position + "xt=urn:btih:".len()..];
    let hash: String = rest
        .chars()
        .take_while(|character| character.is_ascii_hexdigit())
        .collect();

    (hash.len() == 40).then(|| hash.to_ascii_lowercase())
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod filter_blocked_tests {
    use super::*;
    use bokhylle_acquisition::model::ReleaseCandidate;

    fn release(id: &str, indexer: &str) -> bokhylle_acquisition::model::EvaluatedRelease {
        let candidate = ReleaseCandidate {
            source: None,
            method: None,
            id: id.to_string(),
            title: format!("Release {id}"),
            indexer: Some(indexer.to_string()),
            ..Default::default()
        };
        bokhylle_acquisition::evaluator::rank(
            &bokhylle_acquisition::model::ExpectedBook::default(),
            &[candidate],
        )
        .into_iter()
        .next()
        .expect("one evaluated release")
    }

    #[test]
    fn blocked_fingerprints_are_excluded() {
        let mut blocked = std::collections::HashSet::new();
        blocked.insert("indexer a::bad-guid".to_string());
        let kept = filter_blocked(
            vec![
                release("bad-guid", "Indexer A"),
                release("good-guid", "Indexer A"),
            ],
            &blocked,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].candidate.id, "good-guid");
    }

    #[test]
    fn same_guid_from_another_indexer_is_not_blocked() {
        let mut blocked = std::collections::HashSet::new();
        blocked.insert("indexer a::shared-guid".to_string());
        let kept = filter_blocked(vec![release("shared-guid", "Indexer B")], &blocked);
        assert_eq!(kept.len(), 1);
    }
}
