//! Book intent: searching (library and catalogue) and turning "I want this
//! book" into the right outcome for the signed-in profile.

use serde::Serialize;

use crate::AppState;
use crate::auth::User;
use crate::book_requests;
use crate::discovery::{self, SearchKind};
use crate::error::AppError;
use crate::library::import_metadata;
use crate::library::queries::{self, BookDetail, BookFilters, BookSummary};
use bokhylle_acquisition::state::AcquisitionStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    /// The owned library: household for adults, the assigned shelf for children.
    Library,
    /// The public metadata catalogue.
    Catalogue,
    /// Library first, catalogue only when the library has nothing.
    Auto,
}

impl SearchScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "library" => Some(Self::Library),
            "catalogue" => Some(Self::Catalogue),
            "" | "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueHit {
    pub provider: String,
    pub provider_key: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
    pub cover_id: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchOutcome {
    pub library: Vec<BookSummary>,
    pub catalogue: Vec<CatalogueHit>,
}

pub async fn search(
    state: &AppState,
    user: &User,
    query: &str,
    scope: SearchScope,
    limit: i64,
) -> Result<SearchOutcome, AppError> {
    let limit = limit.clamp(1, 50);
    let query = query.trim();
    if query.is_empty() {
        return Ok(SearchOutcome {
            library: Vec::new(),
            catalogue: Vec::new(),
        });
    }

    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let mine = if child { Some(user.id) } else { None };

    let mut library = Vec::new();
    if matches!(scope, SearchScope::Library | SearchScope::Auto) {
        let filters = BookFilters {
            viewer_id: Some(user.id),
            mine,
            ..Default::default()
        };
        library = queries::search_books(&state.db, query, limit, &filters).await?;
    }

    let mut catalogue = Vec::new();
    let want_catalogue = matches!(scope, SearchScope::Catalogue)
        || (matches!(scope, SearchScope::Auto) && library.is_empty());
    if want_catalogue {
        // Either child catalogue permission allows metadata search. When both
        // are off, an explicit search is refused and `auto` stays on the shelf.
        if !crate::services::requests::may_browse_catalogue(state, user.id).await? {
            if matches!(scope, SearchScope::Catalogue) {
                return Err(AppError::Forbidden);
            }
            return Ok(SearchOutcome { library, catalogue });
        }
        let results =
            discovery::external_results(state, SearchKind::Any, query, limit as usize).await?;
        catalogue = results
            .into_iter()
            .map(|result| CatalogueHit {
                provider: result.provider,
                provider_key: result.provider_key,
                title: result.title,
                authors: result.authors,
                year: result.year,
                language: result.language,
                series: result.series,
                series_number: result.series_number,
                cover_id: result.cover_id,
            })
            .collect();
    }

    Ok(SearchOutcome { library, catalogue })
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProgressView {
    pub percentage: f64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookWithProgress {
    pub book: BookDetail,
    pub progress: Option<ProgressView>,
}

/// Book details plus the viewer's shelf, preference and reading progress.
/// Children only ever see books on their shelf.
pub async fn get(
    state: &AppState,
    user: &User,
    book_id: i64,
) -> Result<Option<BookWithProgress>, AppError> {
    if !crate::services::sharing::can_access(&state.db, user.id, book_id).await? {
        return Ok(None);
    }
    let Some(mut book) = queries::get_book(&state.db, book_id).await? else {
        return Ok(None);
    };
    // `queries::get_book` is viewer-agnostic; the shelf and preference are
    // per profile and belong here so every interface gets them.
    book.on_shelf = crate::user_books::contains(&state.db, user.id, book_id).await?;
    if crate::auth::profile_type(&state.db, user.id).await? == "child" && !book.on_shelf {
        return Ok(None);
    }
    book.preference = crate::user_books::preference(&state.db, user.id, book_id).await?;
    let access = crate::services::sharing::state(&state.db, user.id, book_id).await?;
    book.sharing = access.sharing;
    book.shared_in_household = access.shared_in_household;
    book.browser_file_id = sqlx::query_scalar(
        "SELECT p.book_file_id FROM browser_reading_positions p
         JOIN book_files f ON f.id = p.book_file_id AND f.sha256 = p.sha256
         JOIN editions e ON e.id = f.edition_id
         WHERE p.user_id = ? AND e.book_id = ? AND f.format IN ('epub', 'pdf', 'cbz')
         ORDER BY p.updated_at DESC, p.revision DESC LIMIT 1",
    )
    .bind(user.id)
    .bind(book_id)
    .fetch_optional(&state.db)
    .await?;
    let progress: Option<(f64, i64)> = sqlx::query_as(
        "SELECT percentage, updated_at FROM reading_progress
         WHERE user_id = ? AND book_id = ?
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(user.id)
    .bind(book_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Some(BookWithProgress {
        book,
        progress: progress.map(|(percentage, updated_at)| ProgressView {
            percentage,
            updated_at,
        }),
    }))
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentOutcome {
    pub book_id: i64,
    pub phase: String,
    pub message: String,
    pub request_id: Option<i64>,
    pub acquisition_id: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueAcquisitionOutcome {
    pub id: String,
    pub status: AcquisitionStatus,
    pub duplicate: bool,
    pub book_id: i64,
}

/// Acquire a catalogue identity for an adult profile. HTTP and MCP use the
/// same resolution, preference snapshot, duplicate handling and permission.
#[allow(clippy::too_many_arguments)]
pub async fn add_catalogue_with_sharing(
    state: &AppState,
    user: &User,
    provider: &str,
    provider_key: &str,
    preferred_format: Option<String>,
    preferred_language: Option<String>,
    send_to_reader: bool,
    sharing: Option<crate::services::sharing::BookSharing>,
    ask_before_download: Option<bool>,
) -> Result<CatalogueAcquisitionOutcome, AppError> {
    if !crate::auth::can_acquire(&state.db, user).await? {
        return Err(AppError::Forbidden);
    }
    let provider_key = provider_key.trim();
    if provider_key.is_empty() {
        return Err(AppError::BadRequest(
            "providerKey must not be empty".to_string(),
        ));
    }
    let metadata = discovery::resolve_metadata(state, Some(provider), provider_key)
        .await?
        .ok_or_else(|| {
            AppError::NotFound("the book was not found in the metadata catalogue".to_string())
        })?;
    discovery::store_book(state, &metadata).await?;
    let book_id = import_metadata::upsert_book_from_metadata(&state.db, &metadata).await?;
    crate::services::sharing::choose(&state.db, user.id, book_id, sharing).await?;
    let (profile_languages, profile_format) =
        crate::updates::user_preferences(state, user.id).await?;
    let languages = match preferred_language.as_deref() {
        Some(language) if !language.trim().is_empty() => vec![language.trim().to_string()],
        _ => profile_languages,
    };
    let (acquisition, duplicate) = crate::acquisition::create_with_languages(
        &state.db,
        book_id,
        Some(user.id),
        preferred_format.or(profile_format),
        languages,
        send_to_reader,
        ask_before_download.unwrap_or(user.acquisition_mode == "ask"),
    )
    .await?;
    if !duplicate {
        crate::acquisition_pipeline::spawn(state, acquisition.id.clone());
    }
    Ok(CatalogueAcquisitionOutcome {
        id: acquisition.id.clone(),
        status: acquisition.status()?,
        duplicate,
        book_id,
    })
}

fn phase_message(phase: &str) -> String {
    match phase {
        "ready" => "Ready on your shelf.".to_string(),
        "getting" => "Bokhylle found a suitable copy and is getting it.".to_string(),
        "looking" => "Bokhylle is looking for a suitable copy.".to_string(),
        "declined" => "This book was declined.".to_string(),
        _ => "No suitable copy right now; Bokhylle will keep looking.".to_string(),
    }
}

/// "I want this book." Children create a request for an adult to decide;
/// adults get an existing household copy added to their shelf, or an
/// acquisition when nothing suitable is owned.
pub async fn add(
    state: &AppState,
    user: &User,
    book_id: i64,
    send_to_reader: bool,
) -> Result<IntentOutcome, AppError> {
    crate::services::sharing::require_access(&state.db, user.id, book_id).await?;
    if queries::get_book(&state.db, book_id).await?.is_none() {
        return Err(AppError::NotFound("book not found".to_string()));
    }

    if crate::auth::profile_type(&state.db, user.id).await? == "child" {
        let outcome = crate::services::requests::create_for_book(state, user, book_id).await?;
        return Ok(IntentOutcome {
            book_id,
            phase: outcome.request.phase,
            message: "An administrator will decide on this request.".to_string(),
            request_id: Some(outcome.request.id),
            acquisition_id: None,
        });
    }

    let (languages, format) = crate::updates::user_preferences(state, user.id).await?;
    let preferred_format = format.or(user.preferred_format.clone());

    if queries::existing_file_in_languages(
        &state.db,
        book_id,
        preferred_format.as_deref(),
        &languages,
    )
    .await?
    .is_some()
    {
        crate::user_books::add(&state.db, user.id, book_id, "agent").await?;
        let mut message = "Already in the household library; added to your shelf.".to_string();
        if send_to_reader {
            crate::services::delivery::send_book(state, user, book_id, None).await?;
            message.push_str(" Sent to your reader.");
        }
        return Ok(IntentOutcome {
            book_id,
            phase: "ready".to_string(),
            message,
            request_id: None,
            acquisition_id: None,
        });
    }

    if !crate::auth::can_acquire(&state.db, user).await? {
        return Err(AppError::Forbidden);
    }

    let (acquisition, duplicate) = crate::acquisition::create_with_languages(
        &state.db,
        book_id,
        Some(user.id),
        preferred_format,
        languages,
        send_to_reader,
        false,
    )
    .await?;
    if !duplicate {
        crate::acquisition_pipeline::spawn(state, acquisition.id.clone());
    }
    let keep_looking: i64 = sqlx::query_scalar(
        "SELECT CASE WHEN retry_stopped = 0 AND next_retry_at IS NOT NULL THEN 1 ELSE 0 END
         FROM acquisitions WHERE id = ?",
    )
    .bind(&acquisition.id)
    .fetch_one(&state.db)
    .await?;
    let status = acquisition.status()?;
    let phase = book_requests::phase(
        "approved",
        Some(&acquisition.id),
        Some(status.as_str()),
        keep_looking != 0,
    );
    Ok(IntentOutcome {
        book_id,
        message: phase_message(&phase),
        phase,
        request_id: None,
        acquisition_id: Some(acquisition.id),
    })
}

pub async fn add_catalogue(
    state: &AppState,
    user: &User,
    provider: &str,
    provider_key: &str,
    preferred_format: Option<String>,
    preferred_language: Option<String>,
    send_to_reader: bool,
) -> Result<CatalogueAcquisitionOutcome, AppError> {
    add_catalogue_with_sharing(
        state,
        user,
        provider,
        provider_key,
        preferred_format,
        preferred_language,
        send_to_reader,
        None,
        None,
    )
    .await
}
