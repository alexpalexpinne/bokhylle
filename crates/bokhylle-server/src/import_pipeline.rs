use std::path::{Path, PathBuf};

use bokhylle_acquisition::qbittorrent::DEFAULT_CATEGORY;
use bokhylle_acquisition::state::AcquisitionStatus;
use bokhylle_core::BookFormat;
use bokhylle_importer::{self, ExpectedImport, ImportCandidate, Inspection, Limits, ReviewReason};
use serde_json::json;

use crate::AppState;
use crate::acquisition;
use crate::acquisition_pipeline;
use crate::error::AppError;
use crate::library;
use crate::settings;

pub fn spawn(state: &AppState, acquisition_id: String) {
    let key = format!("import-{acquisition_id}");
    if !state.imports.try_acquire(&key) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(error) = run(&state, &acquisition_id).await {
            tracing::warn!(
                acquisition_id = %acquisition_id,
                %error,
                "import.pipeline.failed"
            );
            mark_failed(&state, &acquisition_id, &error).await;
        }
        state.imports.release(&key);
    });
}

async fn mark_failed(state: &AppState, acquisition_id: &str, error: &AppError) {
    let Ok(Some(acquisition)) = acquisition::get(&state.db, acquisition_id).await else {
        return;
    };
    let Ok(status) = acquisition.status() else {
        return;
    };

    if !matches!(
        status,
        AcquisitionStatus::Downloaded
            | AcquisitionStatus::Inspecting
            | AcquisitionStatus::Identified
            | AcquisitionStatus::Importing
    ) {
        return;
    }

    if let Err(failure) = acquisition::fail_import(
        &state.db,
        acquisition_id,
        "import_failed",
        &error.to_string(),
    )
    .await
    {
        tracing::warn!(
            acquisition_id,
            %failure,
            "import.failed_state_not_recorded"
        );
    }
}

pub async fn recover(state: &AppState) {
    let rows: Vec<(String, String, Option<String>)> = match sqlx::query_as(
        "SELECT id, status, content_path
         FROM acquisitions
         WHERE status IN ('DOWNLOADED', 'INSPECTING', 'IDENTIFIED', 'IMPORTING')
         ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(%error, "import.recovery.failed");
            return;
        }
    };

    for (id, _, content_path) in rows {
        if content_path.is_none() && !resolve_content_path(state, &id).await.unwrap_or(false) {
            tracing::info!(acquisition_id = %id, "import.recovery.awaiting_content");
            continue;
        }

        tracing::info!(acquisition_id = %id, "import.recovered");
        spawn(state, id);
    }
}

pub async fn run(state: &AppState, acquisition_id: &str) -> Result<(), AppError> {
    let Some(acquisition) = acquisition::get(&state.db, acquisition_id).await? else {
        return Ok(());
    };

    let status = acquisition.status()?;
    if !matches!(
        status,
        AcquisitionStatus::Downloaded
            | AcquisitionStatus::Inspecting
            | AcquisitionStatus::Identified
            | AcquisitionStatus::Importing
    ) {
        return Ok(());
    }

    if status == AcquisitionStatus::Downloaded {
        if !resolve_content_path(state, acquisition_id).await? {
            acquisition::fail_import(
                &state.db,
                acquisition_id,
                "content_missing",
                "the downloaded content could not be located",
            )
            .await?;
            return Ok(());
        }

        acquisition::transition(
            &state.db,
            acquisition_id,
            AcquisitionStatus::Inspecting,
            None,
        )
        .await?;
    }

    let acquisition = acquisition::get(&state.db, acquisition_id)
        .await?
        .expect("acquisition exists");

    let Some(artifact) = crate::artifact::StagedArtifact::from_acquisition(state, &acquisition)
    else {
        acquisition::fail_import(
            &state.db,
            acquisition_id,
            "content_missing",
            "the downloaded content is missing or outside the downloads directory",
        )
        .await?;
        return Ok(());
    };
    let content_path = artifact.path.to_string_lossy().into_owned();

    let expected_book =
        acquisition_pipeline::load_expected_book(state, acquisition.book_id, &acquisition).await?;
    let expected = ExpectedImport {
        title: expected_book.title,
        authors: expected_book.authors,
        isbn: expected_book.isbn,
        language: expected_book.language,
    };

    if status == AcquisitionStatus::Importing && finalize_pending(state, acquisition_id).await? {
        return Ok(());
    }

    let staging = state.paths.config_dir.join("staging").join(acquisition_id);

    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }

    let inspection = {
        let content = artifact.path.clone();
        let staging = staging.clone();
        let expected = expected.clone();
        // Archive inspection and extraction are heavy synchronous work.
        tokio::task::spawn_blocking(move || importer_inspect(&content, &staging, &expected))
            .await
            .map_err(|error| AppError::Unavailable(error.to_string()))??
    };

    match inspection {
        Inspection::Selected(candidate) => {
            acquisition::log_event(
                &state.db,
                acquisition_id,
                "import.candidates.evaluated",
                Some(json!({
                    "contentPath": content_path,
                    "selected": candidate.path,
                    "confidence": candidate.confidence,
                })),
            )
            .await?;

            let resumed = acquisition::get(&state.db, acquisition_id)
                .await?
                .map(|current| current.status())
                .transpose()?
                == Some(AcquisitionStatus::Importing);

            if !resumed {
                transition_if(state, acquisition_id, AcquisitionStatus::Identified).await?;
            }

            import_candidate(state, acquisition_id, acquisition.book_id, &candidate).await?;
        }
        Inspection::NeedsReview { reason, candidates } => {
            review(
                state,
                acquisition_id,
                &content_path,
                ReviewReasonView::from(reason),
                &candidates,
            )
            .await?;
        }
        Inspection::Empty { reason } => {
            review(
                state,
                acquisition_id,
                &content_path,
                ReviewReasonView::NoFiles,
                &[],
            )
            .await?;
            tracing::info!(acquisition_id, reason, "import.empty");
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum ReviewReasonView {
    LowConfidence,
    MultipleCandidates,
    NoFiles,
    Duplicate,
}

impl ReviewReasonView {
    fn as_str(self) -> &'static str {
        match self {
            Self::LowConfidence => "low_confidence",
            Self::MultipleCandidates => "multiple_candidates",
            Self::NoFiles => "no_supported_files",
            Self::Duplicate => "duplicate_of_other_book",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::LowConfidence => "The downloaded files could not be confidently identified",
            Self::MultipleCandidates => "Several possible books were found in the download",
            Self::NoFiles => "No supported ebook files were found in the download",
            Self::Duplicate => "This file already exists for a different book",
        }
    }
}

impl From<ReviewReason> for ReviewReasonView {
    fn from(reason: ReviewReason) -> Self {
        match reason {
            ReviewReason::LowConfidence => Self::LowConfidence,
            ReviewReason::MultipleCandidates => Self::MultipleCandidates,
        }
    }
}

async fn review(
    state: &AppState,
    acquisition_id: &str,
    content_path: &str,
    reason: ReviewReasonView,
    candidates: &[ImportCandidate],
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE acquisitions SET error_code = ?, error_message = ?, updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(reason.as_str())
    .bind(reason.message())
    .bind(acquisition_id)
    .execute(&state.db)
    .await?;

    acquisition::log_event(
        &state.db,
        acquisition_id,
        "import.candidates.evaluated",
        Some(json!({
            "contentPath": content_path,
            "reason": reason.as_str(),
            "candidates": candidates,
        })),
    )
    .await?;

    transition_if(state, acquisition_id, AcquisitionStatus::NeedsReview).await?;
    tracing::info!(
        acquisition_id,
        reason = reason.as_str(),
        "import.needs_review"
    );
    Ok(())
}

pub async fn import_candidate(
    state: &AppState,
    acquisition_id: &str,
    book_id: i64,
    candidate: &ImportCandidate,
) -> Result<(), AppError> {
    let source = candidate.path.clone();
    let (digest, size, mtime) = tokio::task::spawn_blocking(move || {
        let digest = library::hash_file(&source)?;
        let metadata = std::fs::metadata(&source)?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64);
        Ok::<_, AppError>((digest, metadata.len() as i64, mtime))
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))??;

    let existing: Option<(i64, i64)> = sqlx::query_as(
        "SELECT bf.id, e.book_id
         FROM book_files bf
         JOIN editions e ON e.id = bf.edition_id
         WHERE bf.sha256 = ?",
    )
    .bind(&digest)
    .fetch_optional(&state.db)
    .await?;

    if let Some((existing_file_id, existing_book_id)) = existing {
        if existing_book_id == book_id {
            transition_if(state, acquisition_id, AcquisitionStatus::Importing).await?;
            acquisition::log_event(
                &state.db,
                acquisition_id,
                "import.duplicate",
                Some(json!({ "reason": "same_file", "fileId": existing_file_id })),
            )
            .await?;
            // The file already exists, so a requested send must still resolve it.
            finish_ready(state, acquisition_id, Some(existing_file_id)).await?;
            return Ok(());
        }

        review(
            state,
            acquisition_id,
            candidate.path.to_string_lossy().as_ref(),
            ReviewReasonView::Duplicate,
            std::slice::from_ref(candidate),
        )
        .await?;
        return Ok(());
    }

    transition_if(state, acquisition_id, AcquisitionStatus::Importing).await?;

    let format = BookFormat::from_extension(&candidate.format).unwrap_or(BookFormat::Epub);
    let canonical = canonical_path(state, book_id, &candidate.title, format).await?;

    // Journal the placement first so recovery can finish the DB record if we
    // die between placing the file and inserting it.
    record_pending(
        state,
        acquisition_id,
        book_id,
        &canonical,
        &digest,
        size,
        format,
        Some(candidate.path.to_string_lossy().as_ref()),
    )
    .await?;

    place_file(state, &candidate.path, &canonical, &digest).await?;

    let file_id = library::add_file_to_library(
        &state.db,
        book_id,
        &canonical,
        format,
        size,
        &digest,
        mtime,
        Some(&candidate.path.to_string_lossy()),
    )
    .await?;

    clear_pending(state, acquisition_id).await?;

    acquisition::log_event(
        &state.db,
        acquisition_id,
        "import.completed",
        Some(json!({
            "fileId": file_id,
            "path": canonical,
            "format": candidate.format,
        })),
    )
    .await?;

    tracing::info!(acquisition_id, path = %canonical.display(), "import.completed");
    finish_ready(state, acquisition_id, Some(file_id)).await?;
    Ok(())
}

async fn finish_ready(
    state: &AppState,
    acquisition_id: &str,
    file_id: Option<i64>,
) -> Result<(), AppError> {
    acquisition::transition(
        &state.db,
        acquisition_id,
        AcquisitionStatus::Ready,
        file_id.map(|file_id| json!({ "fileId": file_id })),
    )
    .await?;

    deliver_if_requested(state, acquisition_id, file_id).await;
    crate::notifications::for_requesters(
        &state.db,
        acquisition_id,
        "ready",
        "Added to your library",
        None,
    )
    .await
    .ok();

    let http_owned: i64 = sqlx::query_scalar(
        "SELECT COALESCE(download_provider = 'http', 0) FROM acquisitions WHERE id = ?",
    )
    .bind(acquisition_id)
    .fetch_one(&state.db)
    .await?;
    if http_owned != 0
        || state
            .settings
            .get_bool(settings::CLEANUP_DOWNLOADS, false)
            .await
            .unwrap_or(false)
    {
        cleanup_download(state, acquisition_id).await;
    }

    Ok(())
}

async fn latest_file_id(state: &AppState, book_id: i64) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT f.id FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ?
         ORDER BY f.id DESC
         LIMIT 1",
    )
    .bind(book_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
}

async fn deliver_if_requested(state: &AppState, acquisition_id: &str, file_id: Option<i64>) {
    let Ok(Some(acquisition)) = acquisition::get(&state.db, acquisition_id).await else {
        return;
    };
    let Some(file_id) = file_id else {
        return;
    };

    let users =
        match crate::acquisition_requests::pending_deliveries(&state.db, acquisition_id).await {
            Ok(users) => users,
            Err(_) => return,
        };

    for user_id in users {
        match crate::delivery::deliver(state, user_id, acquisition.book_id, file_id, None).await {
            Ok(record) => {
                // A delivery record exists, so the intent is satisfied even
                // when the send itself failed.
                let _ =
                    crate::acquisition_requests::consume(&state.db, acquisition_id, user_id).await;
                let failed = record.status == "FAILED";
                acquisition::log_event(
                    &state.db,
                    acquisition_id,
                    if failed {
                        "acquisition.delivery.failed"
                    } else {
                        "acquisition.delivery.queued"
                    },
                    Some(json!({
                        "deliveryId": record.id,
                        "status": record.status,
                        "error": record.error_message,
                        "userId": user_id
                    })),
                )
                .await
                .ok();
                crate::notifications::create_for_book(
                    &state.db,
                    user_id,
                    Some(acquisition.book_id),
                    if failed { "failed" } else { "sent" },
                    if failed {
                        "Could not send to your reader"
                    } else {
                        "Sent to your reader"
                    },
                    Some(record.error_message.as_deref().unwrap_or(&record.address)),
                    Some(acquisition_id),
                )
                .await
                .ok();
                if failed {
                    tracing::warn!(
                        acquisition_id,
                        user_id,
                        delivery_id = record.id,
                        "acquisition.delivery.failed"
                    );
                } else {
                    tracing::info!(
                        acquisition_id,
                        user_id,
                        delivery_id = record.id,
                        "acquisition.delivery.queued"
                    );
                }
            }
            Err(error) => {
                // No record was created: keep the intent so a retry can send.
                acquisition::log_event(
                    &state.db,
                    acquisition_id,
                    "acquisition.delivery.failed",
                    Some(json!({ "error": error.to_string(), "userId": user_id })),
                )
                .await
                .ok();
                crate::notifications::create_for_book(
                    &state.db,
                    user_id,
                    Some(acquisition.book_id),
                    "failed",
                    "Could not send to your reader",
                    Some(&error.to_string()),
                    Some(acquisition_id),
                )
                .await
                .ok();
                tracing::warn!(acquisition_id, user_id, %error, "acquisition.delivery.failed");
            }
        }
    }
}

async fn cleanup_download(state: &AppState, acquisition_id: &str) {
    let Ok(Some(acquisition)) = acquisition::get(&state.db, acquisition_id).await else {
        return;
    };
    if acquisition.download_provider.as_deref() == Some("http") {
        if let Some(path) = acquisition.content_path.as_deref() {
            let path = Path::new(path);
            if crate::http_acquisition::owned_file(state, acquisition_id, path) {
                let _ = tokio::fs::remove_file(path).await;
                if let Some(parent) = path.parent() {
                    let _ = tokio::fs::remove_dir(parent).await;
                }
            }
        }
        return;
    }
    if acquisition.download_provider.as_deref() == Some("sabnzbd") {
        // SABnzbd owns its completed files and history. The torrent cleanup
        // setting must not send a SAB job id to qBittorrent.
        return;
    }
    let Some(provider_id) = acquisition.provider_download_id.as_deref() else {
        return;
    };
    let Some(factory) = state.providers.as_ref() else {
        return;
    };
    let Ok(Some(downloader)) = factory.downloader(state).await else {
        return;
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

    match downloader.cancel_owned(provider_id, category, true).await {
        Ok(_) => tracing::info!(acquisition_id, "import.cleanup.completed"),
        Err(error) => {
            tracing::warn!(acquisition_id, %error, "import.cleanup.failed");
        }
    }
}

pub async fn resolve_review(
    state: &AppState,
    acquisition_id: &str,
    action: ReviewAction,
) -> Result<(), AppError> {
    let Some(acquisition) = acquisition::get(&state.db, acquisition_id).await? else {
        return Err(AppError::NotFound("acquisition not found".to_string()));
    };

    if acquisition.status()? != AcquisitionStatus::NeedsReview {
        return Err(AppError::Conflict(
            "acquisition is not awaiting review".to_string(),
        ));
    }

    match action {
        ReviewAction::Choose { path } => {
            let candidate = find_candidate(state, acquisition_id, &path).await?;
            transition_if(state, acquisition_id, AcquisitionStatus::Importing).await?;
            import_candidate(state, acquisition_id, acquisition.book_id, &candidate).await?;
        }
        ReviewAction::Retry => {
            transition_if(state, acquisition_id, AcquisitionStatus::Inspecting).await?;
            spawn(state, acquisition_id.to_string());
        }
        ReviewAction::Ignore => {
            acquisition::transition(
                &state.db,
                acquisition_id,
                AcquisitionStatus::Cancelled,
                Some(json!({ "reason": "review_ignored" })),
            )
            .await?;
            tracing::info!(acquisition_id, "import.review.ignored");
        }
    }

    Ok(())
}

pub enum ReviewAction {
    Choose { path: String },
    Retry,
    Ignore,
}

pub async fn review_candidates(
    state: &AppState,
    acquisition_id: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    acquisition::latest_event_detail(&state.db, acquisition_id, "import.candidates.evaluated").await
}

async fn find_candidate(
    state: &AppState,
    acquisition_id: &str,
    path: &str,
) -> Result<ImportCandidate, AppError> {
    let Some(detail) = review_candidates(state, acquisition_id).await? else {
        return Err(AppError::Conflict(
            "no review candidates are available".to_string(),
        ));
    };

    let candidates: Vec<ImportCandidate> = serde_json::from_value(
        detail
            .get("candidates")
            .cloned()
            .unwrap_or(serde_json::Value::Array(vec![])),
    )
    .map_err(|error| AppError::Unprocessable(format!("invalid review candidates: {error}")))?;

    candidates
        .into_iter()
        .find(|candidate| candidate.path.to_string_lossy() == path)
        .ok_or_else(|| AppError::BadRequest("unknown review candidate".to_string()))
}

async fn transition_if(
    state: &AppState,
    acquisition_id: &str,
    status: AcquisitionStatus,
) -> Result<(), AppError> {
    let Some(acquisition) = acquisition::get(&state.db, acquisition_id).await? else {
        return Ok(());
    };
    if acquisition.status()? == status {
        return Ok(());
    }
    acquisition::transition(&state.db, acquisition_id, status, None).await?;
    Ok(())
}

async fn resolve_content_path(state: &AppState, acquisition_id: &str) -> Result<bool, AppError> {
    let Some(acquisition) = acquisition::get(&state.db, acquisition_id).await? else {
        return Ok(false);
    };

    if let Some(path) = acquisition.content_path.as_deref()
        && crate::paths::is_within(&state.paths.downloads_dir, Path::new(path))
    {
        return Ok(true);
    }

    if matches!(
        acquisition.download_provider.as_deref(),
        Some("http" | "sabnzbd")
    ) {
        return Ok(false);
    }

    let Some(factory) = state.providers.as_ref() else {
        return Ok(false);
    };
    let Ok(Some(downloader)) = factory.downloader(state).await else {
        return Ok(false);
    };

    let info = match acquisition.provider_download_id.as_deref() {
        Some(provider_id) => downloader.status(provider_id).await.ok().flatten(),
        None => {
            let tag = format!(
                "{}{}",
                bokhylle_acquisition::qbittorrent::TAG_PREFIX,
                acquisition_id
            );
            downloader
                .find_by_tag(&tag)
                .await
                .ok()
                .and_then(|torrents| torrents.into_iter().next())
        }
    };

    let Some(info) = info else {
        return Ok(false);
    };

    let downloaded = |path: &Path| crate::paths::is_within(&state.paths.downloads_dir, path);
    let path = info
        .content_path
        .clone()
        .filter(|path| downloaded(Path::new(path)))
        .or_else(|| {
            let candidate = Path::new(&info.save_path).join(&info.name);
            downloaded(&candidate).then(|| candidate.to_string_lossy().into_owned())
        });

    let Some(path) = path else {
        return Ok(false);
    };

    sqlx::query("UPDATE acquisitions SET content_path = ?, updated_at = unixepoch() WHERE id = ?")
        .bind(&path)
        .bind(acquisition_id)
        .execute(&state.db)
        .await?;

    Ok(true)
}

async fn canonical_path(
    state: &AppState,
    book_id: i64,
    fallback_title: &Option<String>,
    format: BookFormat,
) -> Result<PathBuf, AppError> {
    let (title, author): (String, Option<String>) = sqlx::query_as(
        "SELECT b.title, (
             SELECT a.name FROM book_authors ba
             JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = b.id
             ORDER BY ba.position LIMIT 1
         )
         FROM books b WHERE b.id = ?",
    )
    .bind(book_id)
    .fetch_one(&state.db)
    .await?;

    let title = if title.trim().is_empty() {
        fallback_title
            .clone()
            .unwrap_or_else(|| "Untitled".to_string())
    } else {
        title
    };

    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        canonical_path_from(&state, &title, author.as_deref(), format)
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))?
}

/// Library path for a file whose book record does not exist yet (used by the
/// import-existing-downloads job, where the following scan creates the record).
pub(crate) fn canonical_path_from(
    state: &AppState,
    title: &str,
    author: Option<&str>,
    format: BookFormat,
) -> Result<PathBuf, AppError> {
    let title = if title.trim().is_empty() {
        "Untitled".to_string()
    } else {
        title.trim().to_string()
    };
    let author = author.unwrap_or("Unknown Author");
    let extension = format.as_str();

    let directory = state
        .paths
        .library_root
        .join(sanitize_path_component(author))
        .join(sanitize_path_component(&title));
    let base = sanitize_path_component(&title);
    let mut candidate = crate::paths::library_target(
        &state.paths.library_root,
        &directory.join(format!("{base}.{extension}")),
        true,
    )?;
    let directory = candidate
        .parent()
        .expect("library target has a parent")
        .to_path_buf();
    let mut counter = 2;
    while candidate.exists() {
        candidate = directory.join(format!("{base} ({counter}).{extension}"));
        counter += 1;
    }

    Ok(candidate)
}

pub(crate) async fn place_file(
    state: &AppState,
    source: &Path,
    target: &Path,
    expected_digest: &str,
) -> Result<(), AppError> {
    let strategy = state
        .settings
        .get_string(settings::IMPORT_STRATEGY, "hardlink")
        .await?
        .trim()
        .to_ascii_lowercase();

    let source = source.to_path_buf();
    let target = target.to_path_buf();
    let expected_digest = expected_digest.to_string();
    let library_root = state.paths.library_root.clone();
    let downloads = state.paths.downloads_dir.clone();
    let staging = state.paths.config_dir.join("staging");
    tokio::task::spawn_blocking(move || {
        let target = crate::paths::library_target(&library_root, &target, false)?;
        if !crate::paths::is_within(&downloads, &source)
            && !crate::paths::is_within(&staging, &source)
        {
            return Err(AppError::Unprocessable(
                "import source is outside its staging area".into(),
            ));
        }
        place_file_blocking(&strategy, &source, &target, &expected_digest)
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))?
}

/// The synchronous half of [`place_file`]: hardlink/copy/move plus digest
/// verification, all potentially heavy on a large file.
fn place_file_blocking(
    strategy: &str,
    source: &Path,
    target: &Path,
    expected_digest: &str,
) -> Result<(), AppError> {
    match strategy {
        "hardlink" | "link" | "" => {
            verified_placement(source, target, expected_digest, true, false)
        }
        "move" => verified_placement(source, target, expected_digest, true, true),
        "copy" => copy_file(source, target, expected_digest, false),
        other => Err(AppError::Unprocessable(format!(
            "unknown import strategy '{other}' (expected hardlink, move or copy)"
        ))),
    }
}

fn copy_file(
    source: &Path,
    target: &Path,
    expected_digest: &str,
    remove_source: bool,
) -> Result<(), AppError> {
    verified_placement(source, target, expected_digest, false, remove_source)
}

fn verified_placement(
    source: &Path,
    target: &Path,
    expected_digest: &str,
    try_link: bool,
    remove_source: bool,
) -> Result<(), AppError> {
    use std::io::Read;
    let metadata = std::fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() || metadata.len() > Limits::default().max_file_bytes {
        return Err(AppError::Unprocessable(
            "import source is not a supported regular file".into(),
        ));
    }
    let temp = temp_path(target)?;
    let result = (|| -> Result<(), AppError> {
        if !try_link || std::fs::hard_link(source, &temp).is_err() {
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW);
            }
            let input = options.open(source)?;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            let copied = std::io::copy(
                &mut input.take(Limits::default().max_file_bytes + 1),
                &mut output,
            )?;
            if copied > Limits::default().max_file_bytes {
                return Err(AppError::Unprocessable(
                    "import file exceeds the size limit".into(),
                ));
            }
            output.sync_all()?;
        }
        if !std::fs::symlink_metadata(&temp)?.file_type().is_file()
            || library::hash_file(&temp)? != expected_digest
        {
            return Err(AppError::Unprocessable(
                "the imported file does not match the inspected digest".into(),
            ));
        }
        std::fs::File::open(&temp)?.sync_all()?;
        publish_file(&temp, target)?;
        sync_directory(target.parent().expect("import target has a parent"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
        return result;
    }
    if remove_source && library::hash_file(source)? == expected_digest {
        std::fs::remove_file(source)?;
        sync_directory(source.parent().expect("import source has a parent"))?;
    }
    Ok(())
}

/// Atomically publish without replacing another import's destination.
fn publish_file(temp: &Path, target: &Path) -> Result<(), AppError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let from = std::ffi::CString::new(temp.as_os_str().as_bytes())
            .map_err(|_| AppError::Unprocessable("invalid staging path".into()))?;
        let to = std::ffi::CString::new(target.as_os_str().as_bytes())
            .map_err(|_| AppError::Unprocessable("invalid import target".into()))?;
        // Both C strings live through the call; NOREPLACE preserves an existing destination.
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP)
        ) {
            return Err(error.into());
        }
    }
    // The partial and destination share a filesystem. Linking is also atomic
    // and fails if the destination exists, including on older Unix systems.
    std::fs::hard_link(temp, target)?;
    std::fs::remove_file(temp)?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn temp_path(target: &Path) -> Result<PathBuf, AppError> {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::Unprocessable("invalid import target path".into()))?;
    Ok(target.with_file_name(format!(".{file_name}.{}.partial", uuid::Uuid::new_v4())))
}

fn importer_inspect(
    content: &Path,
    staging: &Path,
    expected: &ExpectedImport,
) -> Result<Inspection, AppError> {
    if staging.exists() {
        let _ = std::fs::remove_dir_all(staging);
    }
    std::fs::create_dir_all(staging)?;

    bokhylle_importer::inspect(content, staging, expected, &Limits::default())
        .map_err(|error| AppError::Unprocessable(format!("import inspection failed: {error}")))
}

#[allow(clippy::too_many_arguments)]
async fn record_pending(
    state: &AppState,
    acquisition_id: &str,
    book_id: i64,
    target: &Path,
    digest: &str,
    size: i64,
    format: BookFormat,
    source_path: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO pending_imports
            (acquisition_id, book_id, target_path, sha256, size, format, source_path)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(acquisition_id) DO UPDATE SET
            book_id = excluded.book_id,
            target_path = excluded.target_path,
            sha256 = excluded.sha256,
            size = excluded.size,
            format = excluded.format,
            source_path = excluded.source_path",
    )
    .bind(acquisition_id)
    .bind(book_id)
    .bind(target.to_string_lossy().to_string())
    .bind(digest)
    .bind(size)
    .bind(format.as_str())
    .bind(source_path)
    .execute(&state.db)
    .await?;

    Ok(())
}

async fn clear_pending(state: &AppState, acquisition_id: &str) -> Result<(), AppError> {
    sqlx::query("DELETE FROM pending_imports WHERE acquisition_id = ?")
        .bind(acquisition_id)
        .execute(&state.db)
        .await?;
    Ok(())
}

async fn finalize_pending(state: &AppState, acquisition_id: &str) -> Result<bool, AppError> {
    let pending: Option<(i64, String, String, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT book_id, target_path, sha256, size, format, source_path
         FROM pending_imports
         WHERE acquisition_id = ?",
    )
    .bind(acquisition_id)
    .fetch_optional(&state.db)
    .await?;

    let Some((book_id, target_path, digest, size, format_value, source_path)) = pending else {
        return Ok(false);
    };

    let root = state.paths.library_root.clone();
    let expected = digest.clone();
    let (target, placed, mtime) = tokio::task::spawn_blocking(move || {
        let target = crate::paths::library_target(&root, Path::new(&target_path), false)?;
        let placed = target.exists() && library::hash_file(&target)? == expected;
        let mtime = std::fs::metadata(&target)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64);
        Ok::<_, AppError>((target, placed, mtime))
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))??;

    if !placed {
        tracing::warn!(
            acquisition_id,
            target = %target.display(),
            "import.pending.stale"
        );
        clear_pending(state, acquisition_id).await?;
        return Ok(false);
    }

    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM book_files WHERE sha256 = ?")
        .bind(&digest)
        .fetch_optional(&state.db)
        .await?;

    if existing.is_none() {
        let format = BookFormat::from_extension(&format_value).unwrap_or(BookFormat::Epub);
        library::add_file_to_library(
            &state.db,
            book_id,
            &target,
            format,
            size,
            &digest,
            mtime,
            source_path.as_deref(),
        )
        .await?;

        acquisition::log_event(
            &state.db,
            acquisition_id,
            "import.resumed",
            Some(json!({ "path": target })),
        )
        .await?;
    }

    clear_pending(state, acquisition_id).await?;
    transition_if(state, acquisition_id, AcquisitionStatus::Ready).await?;
    deliver_if_requested(state, acquisition_id, latest_file_id(state, book_id).await).await;
    tracing::info!(
        acquisition_id,
        path = %target.display(),
        "import.pending.finalized"
    );

    Ok(true)
}

fn sanitize_path_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();

    let trimmed = sanitized.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_file_is_atomic_and_verifies_digests() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.epub");
        std::fs::write(&source, b"book content").unwrap();
        let digest = library::hash_file(&source).unwrap();
        let target = dir.path().join("target.epub");

        copy_file(&source, &target, &digest, false).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"book content");
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "partial")
        }));

        let failing_target = dir.path().join("failing.epub");
        let error = copy_file(&source, &failing_target, "deadbeef", false).unwrap_err();
        assert!(error.to_string().contains("digest"));
        assert!(!failing_target.exists());
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "partial")
        }));
    }

    #[test]
    fn copy_file_can_move_by_copying_and_deleting() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.epub");
        std::fs::write(&source, b"book content").unwrap();
        let digest = library::hash_file(&source).unwrap();
        let target = dir.path().join("moved.epub");

        copy_file(&source, &target, &digest, true).unwrap();
        assert!(!source.exists());
        assert!(target.exists());
    }

    #[test]
    fn every_strategy_rejects_changed_bytes_without_publishing_or_losing_the_source() {
        for strategy in ["hardlink", "copy", "move"] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("source.epub");
            std::fs::write(&source, b"original inspected bytes").unwrap();
            let digest = library::hash_file(&source).unwrap();
            std::fs::write(&source, b"changed during import").unwrap();
            let target = dir.path().join("target.epub");
            assert!(place_file_blocking(strategy, &source, &target, &digest).is_err());
            assert!(
                !target.exists(),
                "{strategy} exposed an invalid publication"
            );
            assert_eq!(std::fs::read(&source).unwrap(), b"changed during import");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn every_strategy_preserves_an_existing_destination() {
        for strategy in ["hardlink", "copy", "move"] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("source.epub");
            let target = dir.path().join("target.epub");
            std::fs::write(&source, b"incoming").unwrap();
            std::fs::write(&target, b"existing library file").unwrap();
            let digest = library::hash_file(&source).unwrap();
            assert!(place_file_blocking(strategy, &source, &target, &digest).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), b"existing library file");
            assert!(source.exists());
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }

    #[cfg(unix)]
    #[test]
    fn target_creation_rejects_an_escaping_parent_before_creating_directories() {
        let library = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), library.path().join("Author")).unwrap();
        assert!(
            crate::paths::library_target(
                library.path(),
                &library.path().join("Author/Title/Title.epub"),
                true
            )
            .is_err()
        );
        assert!(!outside.path().join("Title").exists());
    }
}
