use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use bokhylle_acquisition::model::EvaluatedRelease;
use bokhylle_acquisition::qbittorrent::DEFAULT_CATEGORY;

use crate::AppState;
use crate::acquisition;
use crate::acquisition_pipeline;
use crate::auth::{AdminUser, AuthUser, Role};
use crate::error::AppError;
use crate::routes::responses::StatusJson;
use crate::settings;

#[derive(Serialize, schemars::JsonSchema)]
pub struct AcquisitionStart {
    id: String,
    status: bokhylle_acquisition::state::AcquisitionStatus,
    duplicate: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct AcquisitionAccepted {
    id: String,
    status: bokhylle_acquisition::state::AcquisitionStatus,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CandidateView {
    index: usize,
    method: String,
    format: Option<String>,
    language: Option<String>,
    size_bytes: i64,
    is_collection: bool,
    confidence: f32,
    rejected: bool,
    release_name: String,
    indexer: Option<String>,
    seeders: Option<i64>,
    leechers: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score_reasons: Option<Vec<bokhylle_acquisition::model::ScoreReason>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rejection_reasons: Option<Vec<bokhylle_acquisition::model::RejectionReason>>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticAcquisition {
    id: String,
    status: bokhylle_acquisition::state::AcquisitionStatus,
    progress: f64,
    preferred_format: Option<String>,
    preferred_language: Option<String>,
    selected_release_name: Option<String>,
    selected_release_indexer: Option<String>,
    selected_release_score: Option<i64>,
    selected_release_confidence: Option<f64>,
    download_provider: Option<String>,
    provider_download_id: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    content_path: Option<String>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    event: String,
    /// Event details are an append-only journal of several event-specific shapes.
    detail: Option<Value>,
    created_at: i64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct MissingProviderDownload {
    missing: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct ProviderError {
    error: String,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum ProviderDiagnostic {
    Download(bokhylle_acquisition::qbittorrent::TorrentInfo),
    Nzb(bokhylle_acquisition::sabnzbd::NzbStatus),
    Missing(MissingProviderDownload),
    Error(ProviderError),
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionDiagnostics {
    acquisition: DiagnosticAcquisition,
    events: Vec<DiagnosticEvent>,
    candidates: Vec<EvaluatedRelease>,
    provider_state: Option<ProviderDiagnostic>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewImportCandidate {
    path: String,
    format: String,
    size: u64,
    score: i32,
    confidence: f32,
    reasons: Vec<String>,
    title: Option<String>,
    authors: Vec<String>,
    isbn: Option<String>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCandidates {
    content_path: String,
    reason: String,
    candidates: Vec<ReviewImportCandidate>,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSelected {
    content_path: String,
    selected: String,
    confidence: f32,
}

#[derive(Deserialize, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum ReviewDetail {
    Candidates(ReviewCandidates),
    Selected(ReviewSelected),
    /// Older journal entries may predate today's candidate fields.
    Legacy(serde_json::Map<String, Value>),
}

async fn ensure_can_manage(
    state: &AppState,
    user: &crate::auth::User,
    id: &str,
) -> Result<(), AppError> {
    let Some(acquisition) = acquisition::get(&state.db, id).await? else {
        return Err(AppError::NotFound("acquisition not found".to_string()));
    };
    if user.role == Role::Admin || acquisition.user_id == Some(user.id) {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

/// Read access follows the shared-request model: admin, creator or requester.
async fn ensure_can_view(
    state: &AppState,
    user: &crate::auth::User,
    id: &str,
) -> Result<(), AppError> {
    let Some(acquisition) = acquisition::get(&state.db, id).await? else {
        return Err(AppError::NotFound("acquisition not found".to_string()));
    };
    if user.role == Role::Admin || acquisition.user_id == Some(user.id) {
        return Ok(());
    }
    let member: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM acquisition_requests
         WHERE acquisition_id = ? AND user_id = ? LIMIT 1",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;
    if member.is_some() {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn load_evaluated(
    state: &AppState,
    id: &str,
) -> Result<Option<Vec<EvaluatedRelease>>, AppError> {
    let Some(detail) =
        acquisition::latest_event_detail(&state.db, id, "acquisition.candidates.evaluated").await?
    else {
        return Ok(None);
    };

    Ok(Some(acquisition::evaluated_candidates(&detail)))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAcquisition {
    /// Overrides the profile default for this new acquisition only.
    pub ask_before_download: Option<bool>,
    pub sharing: Option<crate::services::sharing::BookSharing>,
    pub preferred_format: Option<String>,
    pub preferred_language: Option<String>,
    pub send_to_reader: Option<bool>,
}

pub async fn create(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(book_id): Path<i64>,
    Json(body): Json<CreateAcquisition>,
) -> Result<StatusJson<AcquisitionStart, 202>, AppError> {
    if !crate::auth::can_acquire(&state.db, &user).await? {
        return Err(AppError::Forbidden);
    }
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("book not found".to_string()));
    }

    crate::services::sharing::require_access(&state.db, user.id, book_id).await?;
    crate::services::sharing::choose(&state.db, user.id, book_id, body.sharing).await?;
    let (profile_languages, profile_format) =
        crate::updates::user_preferences(&state, user.id).await?;
    // An explicitly chosen per-request language narrows the policy to it;
    // otherwise the profile's ordered languages are frozen onto the request.
    let languages = match body.preferred_language.as_deref() {
        Some(language) if !language.trim().is_empty() => {
            vec![language.trim().to_string()]
        }
        _ => profile_languages,
    };
    let preferred_format = body.preferred_format.or(profile_format);
    let (acquisition, duplicate) = acquisition::create_with_languages(
        &state.db,
        book_id,
        Some(user.id),
        preferred_format,
        languages,
        body.send_to_reader.unwrap_or(false),
        body.ask_before_download
            .unwrap_or(user.acquisition_mode == "ask"),
    )
    .await?;

    if !duplicate {
        acquisition_pipeline::spawn(&state, acquisition.id.clone());
    }

    Ok(StatusJson(AcquisitionStart {
        id: acquisition.id.clone(),
        status: acquisition.status()?,
        duplicate,
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateHttpAcquisition {
    pub sharing: Option<crate::services::sharing::BookSharing>,
    pub url: String,
    pub format: Option<String>,
    pub send_to_reader: Option<bool>,
}

pub async fn create_http(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(book_id): Path<i64>,
    Json(body): Json<CreateHttpAcquisition>,
) -> Result<StatusJson<AcquisitionStart, 202>, AppError> {
    if !crate::auth::can_acquire(&state.db, &user).await? {
        return Err(AppError::Forbidden);
    }
    crate::services::sharing::require_access(&state.db, user.id, book_id).await?;
    let url = crate::remote_http::parse_url(&body.url)?;
    let format = body.format.or_else(|| {
        url.path_segments()
            .and_then(|mut segments| segments.next_back())
            .and_then(|filename| filename.rsplit_once('.').map(|(_, ext)| ext.to_string()))
    });
    let format = format.unwrap_or_default().to_ascii_lowercase();
    crate::services::sharing::choose(&state.db, user.id, book_id, body.sharing).await?;
    let (acquisition, duplicate) = start_http(
        &state,
        &user,
        book_id,
        url.as_str(),
        &format,
        "direct",
        "Direct link",
        "manual",
        None,
        body.send_to_reader.unwrap_or(false),
    )
    .await?;
    Ok(StatusJson(AcquisitionStart {
        id: acquisition.id.clone(),
        status: acquisition.status()?,
        duplicate,
    }))
}

#[allow(clippy::too_many_arguments)]
pub async fn start_http(
    state: &AppState,
    user: &crate::auth::User,
    book_id: i64,
    url: &str,
    format: &str,
    source_kind: &str,
    source_name: &str,
    source_key: &str,
    trusted_origin: Option<&str>,
    send_to_reader: bool,
) -> Result<(acquisition::Acquisition, bool), AppError> {
    if !crate::auth::can_acquire(&state.db, user).await? {
        return Err(AppError::Forbidden);
    }
    if !matches!(format, "epub" | "pdf" | "cbz") {
        return Err(AppError::BadRequest(
            "format must be epub, pdf, or cbz".to_string(),
        ));
    }
    let url = crate::remote_http::parse_url(url)?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("book not found".to_string()));
    }
    let (languages, preferred_format) = crate::updates::user_preferences(state, user.id).await?;
    let (acquisition, duplicate) = acquisition::create_http_with_languages(
        &state.db,
        book_id,
        user.id,
        preferred_format.or_else(|| Some(format.to_string())),
        languages,
        send_to_reader,
        url.as_str(),
        format,
        source_kind,
        source_name,
        source_key,
        trusted_origin,
    )
    .await?;
    if !duplicate {
        acquisition_pipeline::spawn(state, acquisition.id.clone());
    }
    Ok((acquisition, duplicate))
}

pub async fn get(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<acquisition::AcquisitionView>, AppError> {
    ensure_can_view(&state, &user, &id).await?;
    let view = acquisition::view(&state.db, user.id, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListParams {
    pub limit: Option<i64>,
    /// Admin-only `scope=household`; the default is the caller's own requests.
    pub scope: Option<String>,
}

pub async fn list(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<acquisition::AcquisitionView>>, AppError> {
    let household = params.scope.as_deref() == Some("household") && user.role == Role::Admin;
    let views = acquisition::list(
        &state.db,
        user.id,
        params.limit.unwrap_or(50),
        household,
        user.role == Role::Admin,
    )
    .await?;
    Ok(Json(views))
}

pub async fn retry(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<acquisition::AcquisitionView>, AppError> {
    ensure_can_manage(&state, &user, &id).await?;
    // Reopening and the manual retry bookkeeping commit together; the manual
    // attempt continues the backoff series rather than resetting it.
    acquisition::retry(&state.db, &id).await?;
    crate::acquisition_pipeline::spawn(&state, id.clone());
    let view = acquisition::view(&state.db, user.id, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct KeepLookingInput {
    pub enabled: bool,
}

/// Stop or resume automatic retries ("Keep looking").
pub async fn set_keep_looking(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<KeepLookingInput>,
) -> Result<Json<acquisition::AcquisitionView>, AppError> {
    ensure_can_manage(&state, &user, &id).await?;
    crate::keep_looking::set_keep_looking(&state.db, &id, body.enabled).await?;
    let view = acquisition::view(&state.db, user.id, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;
    Ok(Json(view))
}

pub async fn inspect_files(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<acquisition::AcquisitionView>, AppError> {
    acquisition::inspect_files(&state.db, &id).await?;
    crate::acquisition_pipeline::spawn(&state, id.clone());
    let view = acquisition::view(&state.db, admin.id, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;
    Ok(Json(view))
}

pub async fn cancel(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<acquisition::AcquisitionView>, AppError> {
    ensure_can_manage(&state, &user, &id).await?;

    let cancelled = acquisition::cancel(&state.db, &id).await?;

    if cancelled.download_provider.as_deref() == Some("http") {
        let directory = state.paths.downloads_dir.join("http").join(&id);
        if crate::paths::is_within(&state.paths.downloads_dir, &directory) {
            let _ = tokio::fs::remove_file(directory.join(".download.partial")).await;
            if let Some(path) = cancelled.content_path.as_deref() {
                let path = std::path::Path::new(path);
                if crate::http_acquisition::owned_file(&state, &id, path) {
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
            let _ = tokio::fs::remove_dir(&directory).await;
        }
    }

    if cancelled.download_provider.as_deref() == Some("sabnzbd") {
        if let Err(error) = crate::nzb_acquisition::reconcile_cancel(&state, &id).await {
            tracing::warn!(acquisition_id = %id, %error, "acquisition.cancel.provider_failed");
        }
        let view = acquisition::view(&state.db, user.id, &id)
            .await?
            .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;
        return Ok(Json(view));
    }

    if let (Some(factory), Some(provider_id)) = (
        state.providers.as_ref(),
        cancelled.provider_download_id.as_deref(),
    ) {
        match factory.downloader(&state).await {
            Ok(Some(downloader)) => {
                let configured = state
                    .settings
                    .get_string(settings::QBITTORRENT_CATEGORY, DEFAULT_CATEGORY)
                    .await
                    .unwrap_or_else(|_| DEFAULT_CATEGORY.to_string());
                let category = match configured.trim() {
                    "" => DEFAULT_CATEGORY,
                    value => value,
                };

                match downloader.cancel_owned(provider_id, category, false).await {
                    Ok(_) => acquisition::clear_cancel_pending(&state.db, &id).await?,
                    Err(error) => {
                        // The download may still be running: keep a durable
                        // flag so the tracker retries and Needs Attention can
                        // surface a persistent failure.
                        acquisition::mark_cancel_pending(&state.db, &id).await?;
                        tracing::warn!(
                            acquisition_id = %id,
                            provider_id,
                            %error,
                            "acquisition.cancel.provider_failed"
                        );
                    }
                }
            }
            Ok(None) => acquisition::clear_cancel_pending(&state.db, &id).await?,
            Err(error) => {
                acquisition::mark_cancel_pending(&state.db, &id).await?;
                tracing::warn!(acquisition_id = %id, %error, "acquisition.cancel.provider_failed");
            }
        }
    }

    let view = acquisition::view(&state.db, user.id, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CandidateParams {
    #[serde(default)]
    pub technical: bool,
}

pub async fn candidates(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<CandidateParams>,
) -> Result<Json<Vec<CandidateView>>, AppError> {
    if params.technical && user.role != Role::Admin {
        return Err(AppError::Forbidden);
    }

    ensure_can_view(&state, &user, &id).await?;

    let Some(evaluated) = load_evaluated(&state, &id).await? else {
        return Err(AppError::NotFound(
            "no candidate list is available for this acquisition".to_string(),
        ));
    };

    let views: Vec<CandidateView> = evaluated
        .iter()
        .enumerate()
        .map(|(index, release)| CandidateView {
            index,
            method: release
                .candidate
                .method
                .as_ref()
                .map(|method| method.kind())
                .unwrap_or("torrent")
                .to_string(),
            format: release.candidate.detected_format.clone(),
            language: release.candidate.detected_language.clone(),
            size_bytes: release.candidate.size_bytes,
            is_collection: release.candidate.is_collection,
            confidence: release.confidence,
            rejected: release.rejected(),
            release_name: release.candidate.title.clone(),
            indexer: release.candidate.indexer.clone(),
            seeders: release.candidate.seeders,
            leechers: release.candidate.leechers,
            score: params.technical.then_some(release.score),
            score_reasons: params.technical.then(|| release.score_reasons.clone()),
            rejection_reasons: params.technical.then(|| release.rejection_reasons.clone()),
        })
        .collect();

    Ok(Json(views))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SelectCandidate {
    pub index: usize,
}

pub async fn select(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SelectCandidate>,
) -> Result<StatusJson<AcquisitionAccepted, 202>, AppError> {
    ensure_can_manage(&state, &user, &id).await?;
    acquisition_pipeline::select_candidate(&state, &id, body.index).await?;

    let view = acquisition::view(&state.db, user.id, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;

    Ok(StatusJson(AcquisitionAccepted {
        id: view.id,
        status: view.status,
    }))
}

pub async fn diagnostics(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<AcquisitionDiagnostics>, AppError> {
    let Some(acquisition) = acquisition::get(&state.db, &id).await? else {
        return Err(AppError::NotFound("acquisition not found".to_string()));
    };

    let events = acquisition::events(&state.db, &id).await?;
    let candidates = load_evaluated(&state, &id).await?.unwrap_or_default();

    let mut provider_state = None;
    if let (Some(factory), Some(provider_id)) = (
        state.providers.as_ref(),
        acquisition.provider_download_id.as_deref(),
    ) {
        if acquisition.download_provider.as_deref() == Some("sabnzbd") {
            provider_state = match factory.nzb_downloader(&state).await {
                Ok(Some(client)) => Some(match client.status(provider_id).await {
                    Ok(status) => ProviderDiagnostic::Nzb(status),
                    Err(error) => ProviderDiagnostic::Error(ProviderError {
                        error: error.to_string(),
                    }),
                }),
                Ok(None) => None,
                Err(error) => Some(ProviderDiagnostic::Error(ProviderError {
                    error: error.to_string(),
                })),
            };
        } else if acquisition.download_provider.as_deref() != Some("http")
            && let Ok(Some(downloader)) = factory.downloader(&state).await
        {
            provider_state = match downloader.status(provider_id).await {
                Ok(Some(info)) => Some(ProviderDiagnostic::Download(info)),
                Ok(None) => Some(ProviderDiagnostic::Missing(MissingProviderDownload {
                    missing: true,
                })),
                Err(error) => Some(ProviderDiagnostic::Error(ProviderError {
                    error: error.to_string(),
                })),
            };
        }
    }

    let event_views: Vec<DiagnosticEvent> = events
        .into_iter()
        .map(|(event, detail, created_at)| DiagnosticEvent {
            event,
            detail: detail.and_then(|detail| serde_json::from_str::<Value>(&detail).ok()),
            created_at,
        })
        .collect();

    let status = acquisition.status()?;
    Ok(Json(AcquisitionDiagnostics {
        acquisition: DiagnosticAcquisition {
            id: acquisition.id,
            status,
            progress: acquisition.progress,
            preferred_format: acquisition.preferred_format,
            preferred_language: acquisition.preferred_language,
            selected_release_name: acquisition.selected_release_name,
            selected_release_indexer: acquisition.selected_release_indexer,
            selected_release_score: acquisition.selected_release_score,
            selected_release_confidence: acquisition.selected_release_confidence,
            download_provider: acquisition.download_provider,
            provider_download_id: acquisition.provider_download_id,
            error_code: acquisition.error_code,
            error_message: acquisition.error_message,
            content_path: acquisition.content_path,
            created_at: acquisition.created_at,
            updated_at: acquisition.updated_at,
        },
        events: event_views,
        candidates,
        provider_state,
    }))
}

pub async fn review(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Option<ReviewDetail>>, AppError> {
    let Some(_acquisition) = acquisition::get(&state.db, &id).await? else {
        return Err(AppError::NotFound("acquisition not found".to_string()));
    };

    let detail = crate::import_pipeline::review_candidates(&state, &id).await?;
    let detail = detail
        .map(|detail| {
            serde_json::from_value(detail).map_err(|_| {
                AppError::Unprocessable(
                    "stored review candidates have an unsupported format".to_string(),
                )
            })
        })
        .transpose()?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum ReviewResolve {
    Choose { path: String },
    Retry,
    Ignore,
}

pub async fn resolve_review(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReviewResolve>,
) -> Result<StatusJson<AcquisitionAccepted, 202>, AppError> {
    let action = match body {
        ReviewResolve::Choose { path } => crate::import_pipeline::ReviewAction::Choose { path },
        ReviewResolve::Retry => crate::import_pipeline::ReviewAction::Retry,
        ReviewResolve::Ignore => crate::import_pipeline::ReviewAction::Ignore,
    };

    crate::import_pipeline::resolve_review(&state, &id, action).await?;

    let view = acquisition::view(&state.db, admin.id, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;

    Ok(StatusJson(AcquisitionAccepted {
        id: view.id,
        status: view.status,
    }))
}

#[cfg(test)]
mod review_contract_tests {
    use super::ReviewDetail;

    #[test]
    fn earlier_review_journal_fields_remain_readable() {
        let legacy = serde_json::json!({
            "contentPath": "/downloads/example",
            "reason": "multiple_candidates",
            "candidates": [{ "path": "book.epub" }]
        });
        let detail: ReviewDetail = serde_json::from_value(legacy.clone()).unwrap();
        assert!(matches!(detail, ReviewDetail::Legacy(_)));
        assert_eq!(serde_json::to_value(detail).unwrap(), legacy);
    }
}
