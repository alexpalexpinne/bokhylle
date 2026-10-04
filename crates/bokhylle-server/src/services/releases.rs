//! Read-only version previews. Choosing one records its identity; the normal
//! acquisition search revalidates it before any download can start.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bokhylle_acquisition::evaluator;
use bokhylle_acquisition::model::{EvaluatedRelease, ExpectedBook, RejectionReason};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::auth::User;
use crate::discovery;
use crate::error::AppError;

const RELEASE_TTL: Duration = Duration::from_secs(300);
const RELEASE_CACHE_CAP: usize = 256;
type ReleaseCache = HashMap<String, (Instant, Vec<EvaluatedRelease>)>;
static RELEASE_CACHE: OnceLock<Mutex<ReleaseCache>> = OnceLock::new();
static RELEASE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    OnceLock::new();

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseView {
    method: String,
    format: Option<String>,
    language: Option<String>,
    size_bytes: i64,
    seeders: Option<i64>,
    is_collection: bool,
    rejected: bool,
    recommended: bool,
    unavailable_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    leechers: Option<Option<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    indexer: Option<Option<String>>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct ReleasesResponse {
    pub releases: Vec<ReleaseView>,
}

/// Hash the operational fingerprint so indexer URLs or private GUIDs are
/// never exposed as a selection identifier in the browser.
pub fn selection_key(release: &EvaluatedRelease) -> String {
    hex::encode(Sha256::digest(
        evaluator::release_key(&release.candidate).as_bytes(),
    ))
}

pub fn validate_choice(key: Option<&str>) -> Result<(), AppError> {
    if key.is_some_and(|key| key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit())) {
        return Err(AppError::BadRequest("invalid version selection".into()));
    }
    Ok(())
}

pub async fn record_choice(
    state: &AppState,
    acquisition_id: &str,
    key: Option<&str>,
) -> Result<(), AppError> {
    if let Some(key) = key {
        crate::acquisition::log_event(
            &state.db,
            acquisition_id,
            "acquisition.release.requested",
            Some(serde_json::json!({ "selectionKey": key })),
        )
        .await?;
    }
    Ok(())
}

pub async fn preview(
    state: &AppState,
    user: &User,
    provider: Option<&str>,
    provider_key: &str,
    format: Option<String>,
) -> Result<ReleasesResponse, AppError> {
    let can_choose = crate::auth::can_acquire(&state.db, user).await?;
    let (languages, profile_format) = crate::updates::user_preferences(state, user.id).await?;
    let format = format
        .or(profile_format)
        .unwrap_or_else(|| "epub".into())
        .to_ascii_lowercase();
    if !matches!(format.as_str(), "epub" | "pdf" | "cbz" | "any") {
        return Err(AppError::BadRequest("invalid book format".into()));
    }
    let (provider_name, mut book) = if provider == Some("local") {
        let book_id = provider_key
            .strip_prefix("local:")
            .and_then(|id| id.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .ok_or_else(|| AppError::BadRequest("invalid local book".into()))?;
        let local = super::books::get(state, user, book_id)
            .await?
            .ok_or_else(|| AppError::NotFound("book not found".into()))?
            .book;
        (
            "local".to_string(),
            ExpectedBook {
                title: local.title,
                authors: local.authors,
                year: local.publication_year.and_then(|year| year.try_into().ok()),
                isbn: local
                    .editions
                    .iter()
                    .find_map(|edition| edition.isbn13.clone().or(edition.isbn10.clone())),
                language: local.language,
                series_number: local.series_number,
                ..Default::default()
            },
        )
    } else {
        if !super::requests::may_browse_catalogue(state, user.id).await? {
            return Err(AppError::Forbidden);
        }
        let detail = discovery::detail_visible(state, provider, provider_key, user.id)
            .await?
            .ok_or_else(|| AppError::NotFound("book not found".into()))?;
        let work = detail.provider == "openlibrary" && detail.provider_key.starts_with("/works/");
        (
            detail.provider,
            ExpectedBook {
                title: detail.title,
                authors: detail.authors,
                year: if work { None } else { detail.year },
                isbn: if work {
                    None
                } else {
                    detail.isbn13.or(detail.isbn10)
                },
                language: detail.language,
                series_number: detail.series_number,
                ..Default::default()
            },
        )
    };
    book.preferred_format = Some(format);
    book.language = languages.first().cloned().or(book.language);
    book.languages = languages;
    // The evaluator depends on this reader's accepted languages and format.
    // Keep caches isolated between profiles and server installations too.
    let cache_key = format!(
        "{}|{}|{}|{}|{:?}",
        state.paths.config_dir.display(),
        user.id,
        provider_name,
        provider_key,
        book
    );
    let lock = {
        let mut locks = RELEASE_LOCKS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .expect("release locks");
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks.entry(cache_key.clone()).or_default().clone()
    };
    let _guard = lock.lock().await;
    let cached = RELEASE_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("release cache")
        .get(&cache_key)
        .filter(|(stored_at, _)| stored_at.elapsed() < RELEASE_TTL)
        .map(|(_, releases)| releases.clone());
    let evaluated = if let Some(cached) = cached {
        cached
    } else {
        let factory = state
            .providers
            .as_ref()
            .ok_or(AppError::IndexerNotConfigured)?;
        let indexer = factory
            .indexer(state)
            .await?
            .ok_or(AppError::IndexerNotConfigured)?;
        let outcome = indexer.search_book(&book).await.map_err(|error| {
            tracing::warn!(%error, "discovery.releases.search_failed");
            AppError::Unavailable("release search failed".into())
        })?;
        let evaluated: Vec<_> = evaluator::rank(&book, &outcome.candidates)
            .into_iter()
            .take(40)
            .collect();
        let mut cache = RELEASE_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .expect("release cache");
        cache.retain(|_, (stored_at, _)| stored_at.elapsed() < RELEASE_TTL);
        if cache.len() >= RELEASE_CACHE_CAP
            && let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
        cache.insert(cache_key, (Instant::now(), evaluated.clone()));
        evaluated
    };
    // Failed release fingerprints can change while a preview is cached.
    let blocked = crate::acquisition::blocked_release_keys(&state.db).await?;
    let evaluated = crate::acquisition_pipeline::filter_blocked(evaluated, &blocked);
    let selectable = |release: &EvaluatedRelease| {
        !release.rejected()
            && !release
                .candidate
                .method
                .as_ref()
                .is_some_and(|method| method.kind() == "http")
    };
    let available = evaluated
        .iter()
        .any(|release| selectable(release) && release.candidate.seeders != Some(0));
    let recommended = evaluated.iter().position(|release| {
        selectable(release) && (!available || release.candidate.seeders != Some(0))
    });
    Ok(ReleasesResponse {
        releases: evaluated
            .iter()
            .enumerate()
            .filter(|(_, release)| can_choose || !release.rejected())
            .map(|(index, release)| {
                let candidate = &release.candidate;
                let reason = release.rejection_reasons.first().map(|reason| {
                    match reason {
                        RejectionReason::LanguageMismatch => "Outside your accepted languages",
                        RejectionReason::Audiobook => "Audiobooks are not supported",
                        RejectionReason::ComicOrManga => "This file is not a supported book format",
                        RejectionReason::UnsupportedFormat => "Unsupported file format",
                        RejectionReason::UnrelatedTitle => "Does not match this book",
                        RejectionReason::OversizedRelease => "Exceeds the download size limit",
                    }
                    .to_string()
                });
                let direct = candidate
                    .method
                    .as_ref()
                    .is_some_and(|method| method.kind() == "http");
                ReleaseView {
                    method: candidate
                        .method
                        .as_ref()
                        .map(|method| method.kind())
                        .unwrap_or("torrent")
                        .into(),
                    format: candidate.detected_format.clone(),
                    language: candidate.detected_language.clone(),
                    size_bytes: candidate.size_bytes,
                    seeders: candidate.seeders,
                    is_collection: candidate.is_collection,
                    rejected: release.rejected() || direct,
                    recommended: recommended == Some(index) && !direct,
                    unavailable_reason: if direct {
                        Some("Use Import from URL for this source".into())
                    } else {
                        reason
                    },
                    selection_key: (can_choose && !release.rejected() && !direct)
                        .then(|| selection_key(release)),
                    release_name: can_choose.then(|| candidate.title.clone()),
                    leechers: can_choose.then_some(candidate.leechers),
                    indexer: can_choose.then(|| candidate.indexer.clone()),
                }
            })
            .collect(),
    })
}
