use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::image_cache;
use crate::AppState;
use crate::auth::{AdminUser, AuthUser, User};
use crate::error::AppError;
use crate::library::queries::{
    self, AuthorDetail, AuthorSummary, BookDetail, BookPage, BookSummary,
};
use crate::library::scan_state::{self, ScanStatus};
use crate::routes::responses::StatusJson;
use crate::services::reader::{
    self, BookCompletionState, BrowserPositionState, ReadingDirectionState, SaveBrowserPosition,
    SetBookCompletion, SetReadingDirection,
};

#[derive(Serialize, schemars::JsonSchema)]
pub struct ClaimedCount {
    claimed: u64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct HiddenSubjects {
    hidden: Vec<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthorId {
    author_id: i64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct FollowState {
    following: bool,
    #[serde(rename = "autoAcquire")]
    auto_acquire: bool,
    #[serde(rename = "deliveryTargetId")]
    delivery_target_id: Option<i64>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct ScanStarted {
    status: &'static str,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthorHit {
    pub author_id: Option<i64>,
    pub name: String,
    pub following: bool,
    pub book_count: i64,
    pub provider: Option<String>,
    pub provider_key: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct AuthorSearchResponse {
    pub local: Vec<AuthorHit>,
    pub external: Vec<AuthorHit>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthorCatalogueItem {
    title: String,
    year: Option<i32>,
    language: Option<String>,
    cover_id: Option<String>,
    provider: String,
    provider_key: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct AuthorCatalogue {
    items: Vec<AuthorCatalogueItem>,
    next: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthorProfileView {
    bio: Option<String>,
    birth_date: Option<String>,
    death_date: Option<String>,
    source_url: String,
}

async fn is_child(state: &AppState, user_id: i64) -> Result<bool, AppError> {
    // Authorization lookups fail closed: an error is not "adult".
    Ok(crate::auth::profile_type(&state.db, user_id).await? == "child")
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BooksQuery {
    pub sort: Option<String>,
    pub mine: Option<bool>,
    /// Administrators may browse a child's shelf by id; children stay self-scoped.
    pub user: Option<i64>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub format: Option<String>,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub subject: Option<String>,
    pub collection: Option<i64>,
    pub letter: Option<String>,
    /// `cover`, `description` or `language`: books an admin could fix.
    pub missing: Option<String>,
}

pub async fn list_books(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<BooksQuery>,
) -> Result<Json<BookPage>, AppError> {
    let sort = query.sort.unwrap_or_else(|| "recent".to_string());
    let filters = queries::BookFilters {
        mine: shelf_scope(&state, &user, query.mine, query.user).await?,
        kind: query.kind.filter(|value| !value.is_empty()),
        format: query.format.filter(|value| !value.is_empty()),
        language: query.language.filter(|value| !value.is_empty()),
        series: query.series.filter(|value| !value.is_empty()),
        letter: query
            .letter
            .as_deref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| value.len() == 1 && value.chars().all(|c| c.is_ascii_alphabetic())),
        subject: query.subject.filter(|value| !value.is_empty()),
        collection: query.collection,
        missing: query
            .missing
            .filter(|value| matches!(value.as_str(), "cover" | "description" | "language")),
    };
    let mut page = queries::list_books(
        &state.db,
        &sort,
        query.page.unwrap_or(1),
        query.page_size.unwrap_or(24),
        &filters,
    )
    .await?;
    page.letters = queries::book_letters(&state.db, &sort, &filters)
        .await
        .unwrap_or_default();
    Ok(Json(page))
}

pub async fn book_facets(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<queries::BookFacets>, AppError> {
    let mine = shelf_scope(
        &state,
        &user,
        Some(query.scope.as_deref() != Some("household")),
        query.user,
    )
    .await?;
    Ok(Json(queries::book_facets(&state.db, mine).await?))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComicShelfQuery {
    pub mine: Option<bool>,
    pub user: Option<i64>,
    pub sort: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

pub async fn comic_shelf(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ComicShelfQuery>,
) -> Result<Json<queries::ComicShelfPage>, AppError> {
    let mine = shelf_scope(&state, &user, Some(query.mine.unwrap_or(true)), query.user).await?;
    Ok(Json(
        queries::comic_shelf(
            &state.db,
            mine,
            query.sort.as_deref().unwrap_or("recent"),
            query.page.unwrap_or(1),
            query.page_size.unwrap_or(24),
        )
        .await?,
    ))
}

pub async fn comic_series(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<ComicShelfQuery>,
) -> Result<Json<queries::SeriesDetail>, AppError> {
    let mine = shelf_scope(&state, &user, Some(query.mine.unwrap_or(true)), query.user).await?;
    let detail = queries::comic_series(&state.db, id, mine, user.id)
        .await?
        .ok_or_else(|| AppError::NotFound("series not found".into()))?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub q: String,
    pub limit: Option<i64>,
    pub mine: Option<bool>,
    /// Administrators may search a child's shelf by id; children stay self-scoped.
    pub user: Option<i64>,
    pub format: Option<String>,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub subject: Option<String>,
    pub collection: Option<i64>,
    pub missing: Option<String>,
}

pub async fn search_books(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<BookSummary>>, AppError> {
    let filters = queries::BookFilters {
        mine: shelf_scope(&state, &user, query.mine, query.user).await?,
        kind: query.kind.filter(|value| !value.is_empty()),
        format: query.format.filter(|value| !value.is_empty()),
        language: query.language.filter(|value| !value.is_empty()),
        series: query.series.filter(|value| !value.is_empty()),
        subject: query.subject.filter(|value| !value.is_empty()),
        collection: query.collection,
        letter: None,
        missing: query
            .missing
            .filter(|value| matches!(value.as_str(), "cover" | "description" | "language")),
    };
    let books =
        queries::search_books(&state.db, &query.q, query.limit.unwrap_or(50), &filters).await?;
    Ok(Json(books))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LimitQuery {
    #[serde(default)]
    pub following: Option<bool>,
    pub limit: Option<i64>,
    /// Personal by default; `scope=household` is the explicit escape hatch.
    pub scope: Option<String>,
    /// Administrators may target a child's shelf by id; children stay self-scoped.
    pub user: Option<i64>,
}

fn scope_of(query: &LimitQuery, user_id: i64) -> Option<i64> {
    if query.scope.as_deref() == Some("household") {
        None
    } else {
        Some(user_id)
    }
}

/// Children read their own shelf. Adults can read their own or the shared
/// collection; administrators may also read a child's assigned shelf.
async fn shelf_scope(
    state: &AppState,
    user: &User,
    mine: Option<bool>,
    member: Option<i64>,
) -> Result<Option<i64>, AppError> {
    if is_child(state, user.id).await? {
        return Ok(Some(user.id));
    }
    if let Some(member_id) = member {
        if member_id == user.id
            || crate::routes::household::can_manage_child(state, user, member_id).await?
        {
            return Ok(Some(member_id));
        }
        return Err(AppError::Forbidden);
    }
    Ok(mine.and_then(|mine| mine.then_some(user.id)))
}

pub async fn recent_books(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<BookSummary>>, AppError> {
    let mine = if is_child(&state, user.id).await? {
        Some(user.id)
    } else {
        scope_of(&query, user.id)
    };
    let books = queries::recent_books(&state.db, query.limit.unwrap_or(12), mine).await?;
    Ok(Json(books))
}

pub async fn continue_reading(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<queries::ReadingProgress>>, AppError> {
    let shelf_only = is_child(&state, user.id).await?;
    let items = queries::continue_reading(&state.db, user.id, 12, shelf_only).await?;
    Ok(Json(items))
}

pub async fn highlights(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<BookSummary>>, AppError> {
    let mine = if is_child(&state, user.id).await? {
        Some(user.id)
    } else {
        scope_of(&query, user.id)
    };
    // Same user + same day = same highlights; a new day reshuffles gently.
    let day = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| (value.as_secs() / 86_400) as i64)
        .unwrap_or_default();
    let seed = user.id.wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64) ^ day;
    let books = queries::highlight_books(&state.db, query.limit.unwrap_or(12), mine, seed).await?;
    Ok(Json(books))
}

pub async fn get_book(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<BookDetail>, AppError> {
    let view = crate::services::books::get(&state, &user, id)
        .await?
        .ok_or_else(|| AppError::NotFound("book not found".to_string()))?;
    Ok(Json(view.book))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PreferenceUpdate {
    pub preference: Option<String>,
}

pub async fn set_preference(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PreferenceUpdate>,
) -> Result<StatusCode, AppError> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM books WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("book not found".to_string()));
    }
    // Children may like their assigned books, never the rest of the household,
    // and the child product only exposes Like.
    if is_child(&state, user.id).await? {
        if body.preference.as_deref() == Some("not_for_me") {
            return Err(AppError::Forbidden);
        }
        if !crate::user_books::contains(&state.db, user.id, id).await? {
            return Err(AppError::NotFound("book not found".to_string()));
        }
    }
    crate::user_books::set_preference(&state.db, user.id, id, body.preference.as_deref()).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_to_shelf(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM books WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("book not found".to_string()));
    }
    crate::user_books::add(&state.db, user.id, id, "manual").await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_from_shelf(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    crate::user_books::remove(&state.db, user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn claim_shelf(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ClaimedCount>, AppError> {
    let claimed = crate::user_books::claim_all(&state.db, user.id).await?;
    Ok(Json(ClaimedCount { claimed }))
}

pub async fn related_books(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<queries::RelatedBooks>, AppError> {
    if is_child(&state, user.id).await? {
        // The detail page omits related rails for children. Do not expose
        // household titles through a direct call to this auxiliary endpoint.
        crate::services::books::get(&state, &user, id)
            .await?
            .ok_or_else(|| AppError::NotFound("book not found".into()))?;
        return Ok(Json(queries::RelatedBooks {
            series: Vec::new(),
            author: Vec::new(),
            similar: Vec::new(),
        }));
    }
    Ok(Json(queries::related_books(&state.db, id).await?))
}

pub async fn home_rails(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<queries::HomeRail>>, AppError> {
    if is_child(&state, user.id).await? {
        return Ok(Json(queries::shelf_rails(&state.db, user.id).await?));
    }
    Ok(Json(queries::home_rails(&state.db, user.id).await?))
}

pub async fn hidden_subjects(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<HiddenSubjects>, AppError> {
    Ok(Json(HiddenSubjects {
        hidden: queries::hidden_subjects(&state.db, user.id).await?,
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SubjectHiddenUpdate {
    pub hidden: bool,
}

pub async fn set_subject_hidden(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(normalized): Path<String>,
    Json(body): Json<SubjectHiddenUpdate>,
) -> Result<StatusCode, AppError> {
    let normalized = normalized.trim().to_lowercase();
    if normalized.is_empty() {
        return Err(AppError::BadRequest("subject is required".to_string()));
    }
    let known: Option<i64> =
        sqlx::query_scalar("SELECT id FROM subjects WHERE normalized_name = ?")
            .bind(&normalized)
            .fetch_optional(&state.db)
            .await?;
    if known.is_none() {
        return Err(AppError::NotFound("unknown subject".to_string()));
    }
    queries::set_subject_hidden(&state.db, user.id, &normalized, body.hidden).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_authors(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<LimitQuery>,
) -> Result<Json<Vec<AuthorSummary>>, AppError> {
    let following_only = query.following.unwrap_or(false);
    let mine = if is_child(&state, user.id).await? {
        // Following is personal and scope-independent; children stay
        // shelf-scoped either way.
        Some(user.id)
    } else if following_only {
        None
    } else if query.user.is_some() {
        shelf_scope(&state, &user, Some(true), query.user).await?
    } else {
        scope_of(&query, user.id)
    };
    Ok(Json(
        queries::list_authors(&state.db, mine, following_only, user.id).await?,
    ))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AuthorSearchQuery {
    pub q: Option<String>,
    /// `local` answers from the durable catalogue only, without waiting for
    /// the metadata provider; the full response carries both lists.
    pub source: Option<String>,
}

/// Author results for Discover: exact/nearby local authors plus a provider
/// identity for external authors, so they can be followed without owning a
/// book. Book-author search remains untouched.
async fn cached_author_search(
    state: &AppState,
    query: &str,
    limit: usize,
) -> Vec<bokhylle_metadata::AuthorCandidate> {
    // Provider author records are stable, so results are persisted in SQLite
    // and shared across the household.
    let cache_key = format!(
        "authors:{}:{}:{}",
        state.metadata.name(),
        query.trim().to_lowercase(),
        limit
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default();
    let cached = sqlx::query_as::<_, (String, i64)>(
        "SELECT value, expires_at FROM metadata_cache WHERE key = ?",
    )
    .bind(&cache_key)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten();
    if let Some((value, expires_at)) = &cached
        && *expires_at > now
        && let Ok(results) = serde_json::from_str::<Vec<bokhylle_metadata::AuthorCandidate>>(value)
    {
        return results;
    }

    let results = match state.metadata.search_authors(query, limit).await {
        Ok(results) => results,
        Err(error) => {
            tracing::warn!(%error, "discovery.author_search.fetch_failed");
            return cached
                .and_then(|(value, _)| serde_json::from_str(&value).ok())
                .unwrap_or_default();
        }
    };
    if let Ok(serialized) = serde_json::to_string(&results) {
        let _ = sqlx::query(
            "INSERT INTO metadata_cache (key, value, fetched_at, expires_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET
                 value = excluded.value,
                 fetched_at = excluded.fetched_at,
                 expires_at = excluded.expires_at",
        )
        .bind(&cache_key)
        .bind(&serialized)
        .bind(now)
        .bind(
            now + if results.is_empty() {
                24 * 3600
            } else {
                30 * 24 * 3600
            },
        )
        .execute(&state.db)
        .await;
    }
    results
}

pub async fn discover_authors(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<AuthorSearchQuery>,
) -> Result<Json<AuthorSearchResponse>, AppError> {
    let text = query.q.unwrap_or_default();
    let text = text.trim();
    if text.len() < 2 {
        return Ok(Json(AuthorSearchResponse {
            local: Vec::new(),
            external: Vec::new(),
        }));
    }
    let local_only = query.source.as_deref() == Some("local");
    let (local, external) = author_hits(&state, user.id, text, local_only).await?;
    Ok(Json(AuthorSearchResponse { local, external }))
}

/// Shared by the author endpoint and the unified discovery page: durable
/// catalogue authors plus transient provider candidates.
pub(crate) async fn author_hits(
    state: &AppState,
    user_id: i64,
    text: &str,
    local_only: bool,
) -> Result<(Vec<AuthorHit>, Vec<AuthorHit>), AppError> {
    let normalized = bokhylle_core::identity::normalize_text(text);
    let like = format!("%{}%", normalized.replace('%', ""));

    let mut items: Vec<(i64, String, i64, i64)> = sqlx::query_as(
        "SELECT a.id, a.name,
                (SELECT COUNT(*) FROM author_follows f
                 WHERE f.author_id = a.id AND f.user_id = ?),
                (SELECT COUNT(DISTINCT ba.book_id) FROM book_authors ba
                 JOIN books b ON b.id = ba.book_id
                 WHERE ba.author_id = a.id
                   AND EXISTS (SELECT 1 FROM book_files f
                               JOIN editions e ON e.id = f.edition_id
                               WHERE e.book_id = b.id))
         FROM authors a
         WHERE a.normalized_name = ? OR a.normalized_name LIKE ?
         ORDER BY CASE WHEN a.normalized_name = ? THEN 0 ELSE 1 END, a.name
         LIMIT 5",
    )
    .bind(user_id)
    .bind(&normalized)
    .bind(&like)
    .bind(&normalized)
    .fetch_all(&state.db)
    .await?;

    // Exact names first, then followed, then the authors with more books.
    items.sort_by(|left, right| {
        let exact = |(_, name, _, _): &(i64, String, i64, i64)| {
            (bokhylle_core::identity::normalize_text(name) == normalized) as i64
        };
        exact(right)
            .cmp(&exact(left))
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| right.3.cmp(&left.3))
            .then_with(|| left.1.cmp(&right.1))
    });

    // Provider matches stay transient: a search hit is not a durable author.
    // They are promoted only by following (or by owning/durable discovery).
    // `source=local` answers from the catalogue without provider latency, so
    // the frontend can paint known authors first.
    let provider_name = state.registry.metadata().name().to_string();
    let mut transient: Vec<(String, String, String)> = Vec::new();
    if !local_only {
        let candidates = cached_author_search(state, text, 5).await;
        let had_candidates = !candidates.is_empty();
        for candidate in candidates {
            let display = bokhylle_core::identity::canonical_author_name(&candidate.name);
            let normalized_name = bokhylle_core::identity::normalize_text(&display);
            if normalized_name.is_empty()
                || items.iter().any(|(_, name, _, _)| {
                    bokhylle_core::identity::normalize_text(name) == normalized_name
                })
                || transient
                    .iter()
                    .any(|(_, existing, _)| *existing == normalized_name)
            {
                continue;
            }
            transient.push((display, normalized_name, candidate.provider_key));
        }
        if !had_candidates
            && state.registry.metadata().name() == "openlibrary"
            && let Ok(Some(olid)) = state.metadata.resolve_author_olid(text).await
        {
            let display = bokhylle_core::identity::canonical_author_name(text);
            let normalized_name = bokhylle_core::identity::normalize_text(&display);
            if !normalized_name.is_empty()
                && !items.iter().any(|(_, name, _, _)| {
                    bokhylle_core::identity::normalize_text(name) == normalized_name
                })
            {
                transient.push((display, normalized_name, olid));
            }
        }
    }
    // Keep the provider's relevance order, promoting an exact author name
    // ahead of composites such as "Ursula K. Le Guin and others".
    transient.sort_by_key(|(_, name, _)| {
        if *name == normalized {
            0
        } else if name.starts_with(&normalized) {
            1
        } else {
            2
        }
    });

    let mut local: Vec<AuthorHit> = items
        .into_iter()
        .map(|(id, name, following, book_count)| AuthorHit {
            author_id: Some(id),
            name,
            following: following > 0,
            book_count,
            provider: None,
            provider_key: None,
        })
        .collect();
    local.truncate(6);
    let mut external: Vec<AuthorHit> = transient
        .into_iter()
        .map(|(name, _, provider_key)| AuthorHit {
            author_id: None,
            name,
            following: false,
            book_count: 0,
            provider: Some(provider_name.clone()),
            provider_key: Some(provider_key),
        })
        .collect();
    external.truncate(6);
    Ok((local, external))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FollowAuthorInput {
    pub name: String,
    pub provider: Option<String>,
    pub provider_key: Option<String>,
}

/// Promoting a transient author hit: following it writes the durable author
/// row (with an Open Library id when that is the provider) and the follow.
pub async fn follow_discover_author(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<FollowAuthorInput>,
) -> Result<Json<AuthorId>, AppError> {
    let author_id = ensure_author(&state, &body).await?;
    crate::follows::set(&state.db, user.id, author_id, true).await?;
    Ok(Json(AuthorId { author_id }))
}

/// Materialize a transient author without following it, so opening an author
/// page does not require (or imply) a follow.
pub async fn ensure_discover_author(
    _user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<FollowAuthorInput>,
) -> Result<Json<AuthorId>, AppError> {
    let author_id = ensure_author(&state, &body).await?;
    Ok(Json(AuthorId { author_id }))
}

async fn ensure_author(state: &AppState, body: &FollowAuthorInput) -> Result<i64, AppError> {
    let name = bokhylle_core::identity::canonical_author_name(&body.name);
    let normalized_name = bokhylle_core::identity::normalize_text(&name);
    if normalized_name.is_empty() {
        return Err(AppError::BadRequest(
            "author name must not be empty".to_string(),
        ));
    }
    let provider = body.provider.as_deref().unwrap_or("openlibrary").trim();
    if provider.is_empty()
        || provider.len() > 32
        || !provider.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
    {
        return Err(AppError::BadRequest(
            "invalid metadata provider".to_string(),
        ));
    }
    let provider_key = body
        .provider_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty());

    // An existing provider identity wins over name matching.
    let mut author_id = match provider_key {
        Some(key) => crate::external_ids::author_by_provider(&state.db, provider, key).await?,
        None => None,
    };
    if author_id.is_none() {
        // Open Library keys keep the legacy `olid` column in sync during the
        // transition; every provider key is stored in the identity table.
        let olid = (provider == "openlibrary")
            .then(|| provider_key.map(str::to_string))
            .flatten();
        sqlx::query(
            "INSERT INTO authors (name, normalized_name, olid)
             VALUES (?, ?, ?)
             ON CONFLICT(normalized_name) DO UPDATE SET
                 olid = COALESCE(authors.olid, excluded.olid)",
        )
        .bind(&name)
        .bind(&normalized_name)
        .bind(&olid)
        .execute(&state.db)
        .await?;
        author_id = Some(
            sqlx::query_scalar("SELECT id FROM authors WHERE normalized_name = ?")
                .bind(&normalized_name)
                .fetch_one(&state.db)
                .await?,
        );
    }
    let author_id = author_id.expect("author id resolved above");
    if let Some(key) = provider_key {
        crate::external_ids::link_author(&state.db, author_id, provider, key).await?;
    }
    Ok(author_id)
}

/// Catalogue works for an author that the household does not own yet.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CatalogueQuery {
    pub page: Option<u32>,
    pub sort: Option<String>,
}

pub async fn author_catalogue(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<CatalogueQuery>,
) -> Result<Json<AuthorCatalogue>, AppError> {
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM authors WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    let Some(name) = name else {
        return Err(AppError::NotFound("author not found".to_string()));
    };
    // A bibliography must be sorted across provider pages, not per page:
    // fetch a few provider pages, merge, then sort and paginate locally.
    let requested_page = query.page.unwrap_or(1).max(1);
    let mut merged = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..4 {
        let (page_items, next, _provider) = crate::discovery::search_page(
            &state,
            crate::discovery::SearchKind::Author,
            &name,
            12,
            user.id,
            cursor.as_deref(),
        )
        .await
        .unwrap_or_default();
        if page_items.is_empty() {
            break;
        }
        merged.extend(page_items);
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }

    let mut results: Vec<_> = merged
        .into_iter()
        .filter(|result| result.owned_book_id.is_none())
        .filter(|result| {
            result
                .authors
                .iter()
                .any(|author| author.eq_ignore_ascii_case(&name))
        })
        .collect();
    results.sort_by(|left, right| match query.sort.as_deref() {
        Some("title") => left.title.to_lowercase().cmp(&right.title.to_lowercase()),
        _ => right
            .year
            .unwrap_or(0)
            .cmp(&left.year.unwrap_or(0))
            .then_with(|| left.title.cmp(&right.title)),
    });
    results.dedup_by(|left, right| left.provider_key == right.provider_key);

    let page_size = 12usize;
    let offset = (requested_page as usize - 1) * page_size;
    let items: Vec<AuthorCatalogueItem> = results
        .iter()
        .skip(offset)
        .take(page_size)
        .map(|result| AuthorCatalogueItem {
            title: result.title.clone(),
            year: result.year,
            language: result.language.clone(),
            cover_id: result.cover_id.clone(),
            provider: result.provider.clone(),
            provider_key: result.provider_key.clone(),
        })
        .collect();
    let next = (offset + page_size < results.len()).then(|| (requested_page + 1).to_string());
    Ok(Json(AuthorCatalogue { items, next }))
}

pub async fn follow_state(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<FollowState>, AppError> {
    let following = crate::follows::is_following(&state.db, user.id, id).await?;
    let (auto_acquire, delivery_target_id) =
        crate::follows::automation_for(&state.db, user.id, id).await?;
    Ok(Json(FollowState {
        following,
        auto_acquire,
        delivery_target_id,
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationInput {
    pub auto_acquire: bool,
    pub delivery_target_id: Option<i64>,
}

pub async fn set_author_automation(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AutomationInput>,
) -> Result<StatusCode, AppError> {
    if !crate::follows::is_following(&state.db, user.id, id).await? {
        return Err(AppError::BadRequest(
            "follow the author before enabling automation".to_string(),
        ));
    }

    if let Some(target) = body.delivery_target_id {
        // A target must belong to the configuring user and still be enabled.
        let owned: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM delivery_targets WHERE id = ? AND user_id = ? AND enabled = 1",
        )
        .bind(target)
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?;
        if owned.is_none() {
            return Err(AppError::BadRequest(
                "that reader does not belong to you".to_string(),
            ));
        }
    }

    if body.auto_acquire {
        if !crate::auth::can_acquire(&state.db, &user).await? {
            return Err(AppError::Forbidden);
        }
        // Arm only after a successful baseline snapshot.
        if !crate::updates::arm_automation(&state, user.id, id, body.delivery_target_id).await? {
            return Err(AppError::Unavailable(
                "the metadata provider is unavailable; automation was not enabled".to_string(),
            ));
        }
    } else {
        crate::follows::set_automation(&state.db, user.id, id, false, body.delivery_target_id)
            .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn follow_author(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    crate::follows::set(&state.db, user.id, id, true).await?;
    // Populate the author's discoveries promptly instead of waiting for the
    // next hourly refresh.
    let refresher = state.clone();
    tokio::spawn(async move {
        let _ = crate::updates::refresh_followed_authors(&refresher).await;
    });
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unfollow_author(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    crate::follows::set(&state.db, user.id, id, false).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn author_photo(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let row: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT name, photo_checked_at FROM authors WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;

    let Some((name, photo_checked_at)) = row else {
        return Err(AppError::NotFound("author not found".to_string()));
    };
    let mut olid = crate::external_ids::author_olid(&state.db, id).await?;

    const NEGATIVE_TTL_SECONDS: i64 = 30 * 24 * 3600;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0);
    let checked_recently =
        photo_checked_at.is_some_and(|checked| now - checked < NEGATIVE_TTL_SECONDS);

    if olid.is_none() {
        if checked_recently {
            return Ok(no_photo_response());
        }
        match state.metadata.resolve_author_olid(&name).await {
            Ok(Some(found)) => {
                sqlx::query("UPDATE authors SET olid = ? WHERE id = ?")
                    .bind(&found)
                    .bind(id)
                    .execute(&state.db)
                    .await?;
                crate::external_ids::link_author(&state.db, id, "openlibrary", &found).await?;
                olid = Some(found);
            }
            Ok(None) => {}
            Err(error) => {
                // A transient provider failure is not "no photo".
                tracing::warn!(%error, author_id = id, "library.author_photo.resolve_failed");
                return Ok(no_photo_response());
            }
        }
    }

    let Some(olid) = olid else {
        mark_author_photo_checked(&state, id).await;
        return Ok(no_photo_response());
    };

    let cache_dir = state.paths.config_dir.join("cache").join("authors");
    let cached_path = cache_dir.join(format!("ol-{olid}.jpg"));
    let missing_path = cache_dir.join(format!("ol-{olid}.missing"));

    if let Ok(bytes) = tokio::fs::read(&cached_path).await
        && bokhylle_library::covers::usable_cover(&bytes)
    {
        return Ok((
            [
                (header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg")),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=604800"),
                ),
            ],
            bytes,
        )
            .into_response());
    }

    if checked_recently || image_cache::recently_missing(&missing_path).await {
        return Ok(no_photo_response());
    }

    match state.metadata.fetch_author_photo(&olid).await {
        Ok(Some(bytes)) if bokhylle_library::covers::usable_cover(&bytes) => {
            if let Err(error) = image_cache::write_atomic(&cached_path, &bytes).await {
                tracing::warn!(%error, %olid, "library.author_photo.cache_write_failed");
            }
            sqlx::query("UPDATE authors SET photo_checked_at = NULL WHERE id = ?")
                .bind(id)
                .execute(&state.db)
                .await
                .ok();
            Ok((
                [
                    (header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg")),
                    (
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("public, max-age=604800"),
                    ),
                ],
                bytes,
            )
                .into_response())
        }
        Ok(_) => {
            mark_author_photo_checked(&state, id).await;
            if let Err(error) = image_cache::mark_missing(&missing_path).await {
                tracing::warn!(%error, %olid, "library.author_photo.negative_cache_write_failed");
            }
            Ok(no_photo_response())
        }
        Err(error) => {
            // Only a confirmed missing photo is negative-cached.
            tracing::warn!(%error, %olid, "library.author_photo.fetch_failed");
            Ok(no_photo_response())
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverAuthorPhotoQuery {
    pub provider_key: String,
}

/// Transient author search hits have a stable Open Library key before they
/// have a local author row. Serve their portraits through the same cache as
/// followed authors so the browser never needs to contact the provider.
pub async fn discover_author_photo(
    _user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<DiscoverAuthorPhotoQuery>,
) -> Result<Response, AppError> {
    let olid = query
        .provider_key
        .trim()
        .strip_prefix("/authors/")
        .unwrap_or(query.provider_key.trim());
    if !(4..=24).contains(&olid.len())
        || !olid.starts_with("OL")
        || !olid.ends_with('A')
        || !olid[2..olid.len().saturating_sub(1)]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(AppError::BadRequest("invalid author key".to_string()));
    }
    let Some(provider) = state.registry.provider("openlibrary") else {
        return Ok(no_photo_response());
    };
    let cache_dir = state.paths.config_dir.join("cache").join("authors");
    let cached_path = cache_dir.join(format!("ol-{olid}.jpg"));
    let missing_path = cache_dir.join(format!("ol-{olid}.missing"));
    if let Ok(bytes) = tokio::fs::read(&cached_path).await
        && bokhylle_library::covers::usable_cover(&bytes)
    {
        return Ok((
            [
                (header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg")),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=604800"),
                ),
            ],
            bytes,
        )
            .into_response());
    }
    if image_cache::recently_missing(&missing_path).await {
        return Ok(no_photo_response());
    }
    match provider.fetch_author_photo(olid).await {
        Ok(Some(bytes)) if bokhylle_library::covers::usable_cover(&bytes) => {
            if let Err(error) = async {
                tokio::fs::create_dir_all(&cache_dir).await?;
                tokio::fs::write(&cached_path, &bytes).await
            }
            .await
            {
                tracing::warn!(%error, %olid, "discovery.author_photo.cache_write_failed");
            }
            Ok((
                [
                    (header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg")),
                    (
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("public, max-age=604800"),
                    ),
                ],
                bytes,
            )
                .into_response())
        }
        Ok(_) => {
            if let Err(error) = image_cache::mark_missing(&missing_path).await {
                tracing::warn!(%error, %olid, "discovery.author_photo.negative_cache_write_failed");
            }
            Ok(no_photo_response())
        }
        Err(error) => {
            tracing::warn!(%error, %olid, "discovery.author_photo.fetch_failed");
            Ok(no_photo_response())
        }
    }
}

async fn mark_author_photo_checked(state: &AppState, id: i64) {
    sqlx::query("UPDATE authors SET photo_checked_at = unixepoch() WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .ok();
}

fn no_photo_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        (),
    )
        .into_response()
}

pub async fn get_author(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<AuthorDetail>, AppError> {
    // Author pages show everything the household has (a personal shelf scope
    // hides most of an author's books); children stay shelf-scoped.
    let mine = if is_child(&state, user.id).await? {
        Some(user.id)
    } else {
        None
    };
    let author = queries::get_author(&state.db, id, mine)
        .await?
        .ok_or_else(|| AppError::NotFound("author not found".to_string()))?;
    Ok(Json(author))
}

/// Optional author biography. Keep this separate from the local author page
/// response so an unavailable metadata provider cannot delay the books.
pub async fn author_profile(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Option<AuthorProfileView>>, AppError> {
    let author: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT name, photo_checked_at FROM authors WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (name, photo_checked_at) =
        author.ok_or_else(|| AppError::NotFound("author not found".to_string()))?;
    if is_child(&state, user.id).await? {
        let visible: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM book_authors ba
                JOIN user_books ub ON ub.book_id = ba.book_id
                JOIN editions e ON e.book_id = ba.book_id
                JOIN book_files f ON f.edition_id = e.id
                WHERE ba.author_id = ? AND ub.user_id = ? AND ub.on_shelf = 1
            )",
        )
        .bind(id)
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
        if !visible {
            return Err(AppError::NotFound("author not found".to_string()));
        }
    }

    let Some(provider) = state.registry.provider("openlibrary") else {
        return Ok(Json(None));
    };
    let raw_key = crate::external_ids::author_olid(&state.db, id).await?;
    let olid = if let Some(valid) = raw_key
        .as_deref()
        .and_then(bokhylle_metadata::open_library::valid_author_olid)
    {
        valid.to_string()
    } else {
        // Local scans know names first. Resolve only an exact Open Library
        // author-name match, as the photo route does, before fetching a bio.
        // A recent failed photo lookup doubles as a negative identity cache.
        let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
            .fetch_one(&state.db)
            .await?;
        if photo_checked_at.is_some_and(|checked| now - checked < 30 * 24 * 3600) {
            return Ok(Json(None));
        }
        let found = match provider.resolve_author_olid(&name).await {
            Ok(Some(found)) => found,
            Ok(None) => {
                mark_author_photo_checked(&state, id).await;
                return Ok(Json(None));
            }
            Err(error) => {
                tracing::warn!(%error, author_id = id, "library.author_profile.resolve_failed");
                return Ok(Json(None));
            }
        };
        let Some(valid) = bokhylle_metadata::open_library::valid_author_olid(&found) else {
            return Ok(Json(None));
        };
        let valid = valid.to_string();
        crate::external_ids::link_author(&state.db, id, "openlibrary", &valid).await?;
        sqlx::query("UPDATE authors SET olid = ? WHERE id = ?")
            .bind(&valid)
            .bind(id)
            .execute(&state.db)
            .await?;
        valid
    };
    let key = format!("author-profile:openlibrary:{olid}");
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&state.db)
        .await?;
    let cached: Option<(String, i64)> =
        sqlx::query_as("SELECT value, expires_at FROM metadata_cache WHERE key = ?")
            .bind(&key)
            .fetch_optional(&state.db)
            .await?;
    let cached_profile = cached.as_ref().and_then(|(value, _)| {
        serde_json::from_str::<Option<bokhylle_metadata::AuthorProfile>>(value).ok()
    });
    let profile = if cached.as_ref().is_some_and(|(_, expires)| *expires > now)
        && cached_profile.is_some()
    {
        cached_profile.unwrap_or(None)
    } else {
        match provider.get_author_profile(&olid).await {
            Ok(profile) => {
                let has_details = profile.as_ref().is_some_and(|value| {
                    value.bio.is_some() || value.birth_date.is_some() || value.death_date.is_some()
                });
                let ttl = if has_details {
                    7 * 24 * 3600
                } else {
                    24 * 3600
                };
                if let Ok(serialized) = serde_json::to_string(&profile) {
                    let _ = sqlx::query(
                        "INSERT INTO metadata_cache (key, value, fetched_at, expires_at)
                         VALUES (?, ?, ?, ?)
                         ON CONFLICT(key) DO UPDATE SET
                             value = excluded.value,
                             fetched_at = excluded.fetched_at,
                             expires_at = excluded.expires_at",
                    )
                    .bind(&key)
                    .bind(serialized)
                    .bind(now)
                    .bind(now + ttl)
                    .execute(&state.db)
                    .await;
                }
                profile
            }
            Err(error) => {
                tracing::warn!(%error, author_id = id, "library.author_profile.fetch_failed");
                cached_profile.unwrap_or(None)
            }
        }
    };
    Ok(Json(profile.map(|value| AuthorProfileView {
        bio: value.bio,
        birth_date: value.birth_date,
        death_date: value.death_date,
        source_url: format!("https://openlibrary.org/authors/{olid}"),
    })))
}

pub async fn download_file(
    _user: AuthUser,
    State(state): State<AppState>,
    Path((book_id, file_id)): Path<(i64, i64)>,
    request: Request,
) -> Result<Response, AppError> {
    let target = queries::get_download_target(&state.db, book_id, file_id)
        .await?
        .ok_or_else(|| AppError::NotFound("file not found".to_string()))?;

    if !tokio::fs::metadata(&target.path)
        .await
        .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(AppError::NotFound("file is missing on disk".to_string()));
    }

    let author = target.authors.first().cloned().unwrap_or_default();
    let filename = if author.is_empty() {
        format!("{}.{}", target.title, target.format)
    } else {
        format!("{} - {}.{}", target.title, author, target.format)
    };

    let service = ServeFile::new(&target.path);
    let response = service
        .oneshot(request)
        .await
        .expect("serve file is infallible");

    let (mut parts, body) = response.into_parts();
    parts
        .headers
        .insert(header::CONTENT_DISPOSITION, content_disposition(&filename));

    Ok(Response::from_parts(parts, axum::body::Body::new(body)))
}

pub async fn content_file(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((book_id, file_id)): Path<(i64, i64)>,
    request: Request,
) -> Result<Response, AppError> {
    let file = reader::readable_file(&state, &user, book_id, file_id).await?;
    let response = ServeFile::new(&file.path)
        .oneshot(request)
        .await
        .expect("serve file is infallible");
    let (mut parts, body) = response.into_parts();
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(match file.format {
            bokhylle_core::BookFormat::Epub => "application/epub+zip",
            bokhylle_core::BookFormat::Pdf => "application/pdf",
            bokhylle_core::BookFormat::Cbz => "application/vnd.comicbook+zip",
        }),
    );
    parts.headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline"),
    );
    parts.headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(Response::from_parts(parts, axum::body::Body::new(body)))
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComicManifest {
    pub pages: usize,
}

pub async fn cbz_manifest(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((book_id, file_id)): Path<(i64, i64)>,
) -> Result<Json<ComicManifest>, AppError> {
    let file = reader::readable_file(&state, &user, book_id, file_id).await?;
    if file.format != bokhylle_core::BookFormat::Cbz {
        return Err(AppError::NotFound("comic file not found".into()));
    }
    let pages =
        tokio::task::spawn_blocking(move || bokhylle_library::extract::cbz::pages(&file.path))
            .await
            .map_err(|error| AppError::Unavailable(error.to_string()))?
            .map_err(|_| AppError::Unprocessable("CBZ cannot be read".into()))?;
    Ok(Json(ComicManifest { pages: pages.len() }))
}

pub async fn cbz_page(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((book_id, file_id, page)): Path<(i64, i64, usize)>,
) -> Result<Response, AppError> {
    let file = reader::readable_file(&state, &user, book_id, file_id).await?;
    if file.format != bokhylle_core::BookFormat::Cbz {
        return Err(AppError::NotFound("comic file not found".into()));
    }
    let (bytes, mime) = tokio::task::spawn_blocking(move || {
        bokhylle_library::extract::cbz::read_page(&file.path, page)
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))?
    .map_err(|_| AppError::NotFound("comic page not found".into()))?;
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(mime)),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("inline"),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub async fn browser_position(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((book_id, file_id)): Path<(i64, i64)>,
) -> Result<Json<BrowserPositionState>, AppError> {
    Ok(Json(
        reader::position(&state, &user, book_id, file_id).await?,
    ))
}

pub async fn book_completion(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(book_id): Path<i64>,
) -> Result<Json<BookCompletionState>, AppError> {
    Ok(Json(reader::book_completion(&state, &user, book_id).await?))
}

pub async fn set_book_completion(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(book_id): Path<i64>,
    Json(update): Json<SetBookCompletion>,
) -> Result<Json<BookCompletionState>, AppError> {
    Ok(Json(
        reader::set_book_completion(&state, &user, book_id, update).await?,
    ))
}

pub async fn save_browser_position(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((book_id, file_id)): Path<(i64, i64)>,
    Json(update): Json<SaveBrowserPosition>,
) -> Result<Json<BrowserPositionState>, AppError> {
    Ok(Json(
        reader::save_position(&state, &user, book_id, file_id, update).await?,
    ))
}

pub async fn set_reading_direction(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((book_id, file_id)): Path<(i64, i64)>,
    Json(update): Json<SetReadingDirection>,
) -> Result<Json<ReadingDirectionState>, AppError> {
    Ok(Json(
        reader::set_direction(&state, &user, book_id, file_id, update).await?,
    ))
}

pub async fn get_cover(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    if is_child(&state, user.id).await?
        && !crate::user_books::contains(&state.db, user.id, id).await?
    {
        return Err(AppError::NotFound("book not found".to_string()));
    }
    let target = queries::get_cover_target(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("book not found".to_string()))?;

    if let Some(cover_path) = target.cover_path
        && let Ok(bytes) = tokio::fs::read(&cover_path).await
        && bokhylle_library::covers::usable_cover(&bytes)
    {
        let content_type = content_type_for(&cover_path);
        return Ok((
            [
                (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
                (
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=86400"),
                ),
            ],
            bytes,
        )
            .into_response());
    }

    // No usable embedded cover: try Open Library by ISBN and cache the result.
    if let Some(isbn) = target.isbn.as_deref() {
        let covers_dir = state.paths.config_dir.join("artwork").join("covers");
        let cached_path = covers_dir.join(format!("ol-isbn-{isbn}.jpg"));

        if let Ok(bytes) = tokio::fs::read(&cached_path).await
            && bokhylle_library::covers::usable_cover(&bytes)
        {
            return Ok((
                [
                    (header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg")),
                    (
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("public, max-age=86400"),
                    ),
                ],
                bytes,
            )
                .into_response());
        }

        match state.metadata.fetch_cover_by_isbn(isbn).await {
            Ok(Some(bytes)) if bokhylle_library::covers::usable_cover(&bytes) => {
                if let Err(error) = image_cache::write_atomic(&cached_path, &bytes).await {
                    tracing::warn!(%error, %isbn, "library.cover.cache_write_failed");
                }
                return Ok((
                    [
                        (header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg")),
                        (
                            header::CACHE_CONTROL,
                            HeaderValue::from_static("public, max-age=86400"),
                        ),
                    ],
                    bytes,
                )
                    .into_response());
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, %isbn, "library.cover.fetch_failed");
            }
        }
    }

    let placeholder = bokhylle_library::covers::placeholder_svg(&target.title, &target.authors);
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("image/svg+xml"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        placeholder,
    )
        .into_response())
}

pub async fn start_scan(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<StatusJson<ScanStarted, 202>, AppError> {
    if !scan_state::start_scan(&state) {
        return Err(AppError::Conflict("a scan is already running".to_string()));
    }

    Ok(StatusJson(ScanStarted { status: "started" }))
}

pub async fn scan_status(_user: AuthUser, State(state): State<AppState>) -> Json<ScanStatus> {
    Json(state.scan_state.status())
}

fn content_disposition(filename: &str) -> HeaderValue {
    let fallback: String = filename
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();

    let encoded = percent_encode(filename);

    HeaderValue::from_str(&format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        let character = *byte as char;
        if character.is_ascii_alphanumeric()
            || matches!(
                character,
                '!' | '#' | '$' | '&' | '+' | '-' | '.' | '^' | '_' | '`' | '|' | '~'
            )
        {
            encoded.push(character);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn content_type_for(path: &str) -> &'static str {
    match std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}
