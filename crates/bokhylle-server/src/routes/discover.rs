use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AppState;
use crate::auth::AuthUser;

use super::image_cache;
use crate::discovery::{self, DiscoveryResult, SearchKind};
use crate::error::AppError;
use crate::library::import_metadata;
use crate::routes::library::AuthorHit;
use crate::routes::responses::StatusJson;

#[derive(Serialize, schemars::JsonSchema)]
pub struct DiscoveryPage {
    query: String,
    authors: DiscoveryAuthors,
    books: Vec<DiscoveryResult>,
    next: Option<String>,
    local: bool,
    provider: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DiscoveryAuthors {
    local: Vec<AuthorHit>,
    external: Vec<AuthorHit>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct SearchPage {
    items: Vec<DiscoveryResult>,
    next: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LikedDiscovery {
    book_id: i64,
    preference: &'static str,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateFromDiscovery {
    pub ask_before_download: Option<bool>,
    pub release_key: Option<String>,
    pub sharing: Option<crate::services::sharing::BookSharing>,
    pub provider: String,
    pub provider_key: String,
    pub preferred_format: Option<String>,
    pub preferred_language: Option<String>,
    pub send_to_reader: Option<bool>,
    pub target_id: Option<i64>,
}

pub async fn create_acquisition(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateFromDiscovery>,
) -> Result<StatusJson<crate::services::books::CatalogueAcquisitionOutcome, 202>, AppError> {
    let result = crate::services::books::add_catalogue_with_release(
        &state,
        &user,
        &body.provider,
        &body.provider_key,
        body.preferred_format,
        body.preferred_language,
        body.send_to_reader.unwrap_or(false),
        body.sharing,
        body.ask_before_download,
        body.release_key,
        body.target_id,
    )
    .await?;
    Ok(StatusJson(result))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    pub q: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub limit: Option<usize>,
}

pub async fn search(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<DiscoveryResult>>, AppError> {
    let query = params.q.unwrap_or_default();
    let query = query.trim();
    if query.is_empty() {
        return Err(AppError::BadRequest(
            "search query must not be empty".to_string(),
        ));
    }

    let kind = SearchKind::parse(params.kind.as_deref().unwrap_or("any"))
        .ok_or_else(|| AppError::BadRequest("invalid search type".to_string()))?;

    let results =
        discovery::search(&state, kind, query, params.limit.unwrap_or(24), user.id).await?;
    Ok(Json(results))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchPageParams {
    pub q: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub limit: Option<usize>,
    pub continuation: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnifiedParams {
    pub q: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub limit: Option<usize>,
    pub continuation: Option<String>,
    /// `local` answers books and authors from the durable catalogue only.
    pub source: Option<String>,
    /// Explicit provider choice: only that provider is queried, with no
    /// hidden fallback. Omitted means the configured Automatic behaviour.
    pub provider: Option<String>,
}

/// One discovery response: annotated books plus local/external authors, with
/// the intent ranking applied server-side so every client renders the same
/// interpretation. `source=local` is the fast path that never waits on the
/// metadata provider.
pub async fn page(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<UnifiedParams>,
) -> Result<Json<DiscoveryPage>, AppError> {
    let query = params.q.unwrap_or_default();
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err(AppError::BadRequest(
            "search query must not be empty".to_string(),
        ));
    }
    let kind = SearchKind::parse(params.kind.as_deref().unwrap_or("any"))
        .ok_or_else(|| AppError::BadRequest("invalid search type".to_string()))?;
    let local_only = params.source.as_deref() == Some("local");
    let limit = params.limit.unwrap_or(24);
    let selection = match params.provider.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => discovery::ProviderSelection::Only(name),
        _ => discovery::ProviderSelection::Automatic,
    };

    // Continuation pages only carry books; the frontend keeps the authors it
    // already has, so author work (including the provider call) is skipped.
    let wants_authors =
        matches!(kind, SearchKind::Any | SearchKind::Author) && params.continuation.is_none();

    let books_future = async {
        if local_only {
            Ok((
                discovery::local_books(&state, kind, &query, user.id).await?,
                None,
                None,
            ))
        } else {
            discovery::search_page_selected(
                &state,
                kind,
                &query,
                limit,
                user.id,
                params.continuation.as_deref(),
                selection,
            )
            .await
            .map(|(books, next, provider)| (books, next, Some(provider)))
        }
    };
    let authors_future = async {
        if wants_authors {
            crate::routes::library::author_hits(&state, user.id, &query, local_only).await
        } else {
            Ok((Vec::new(), Vec::new()))
        }
    };
    let (book_result, author_result) = tokio::join!(books_future, authors_future);
    let (books, next, answered_by) = book_result?;
    let (mut local_authors, mut external_authors) = author_result?;
    if wants_authors {
        // The provider author search matches names, so the actual author of
        // the matching books may never have been a candidate; derive them
        // from the book results.
        append_derived_book_authors(
            &state,
            user.id,
            &books,
            &query,
            &mut local_authors,
            &mut external_authors,
        )
        .await?;
    }
    // An Anywhere query that reads as a title must not promote a provider-only
    // author; an explicit author search keeps provider results.
    let external_authors = if kind == SearchKind::Any {
        suppress_author_noise(&books, &query, external_authors)
    } else {
        external_authors
    };

    Ok(Json(DiscoveryPage {
        query,
        authors: DiscoveryAuthors {
            local: local_authors,
            external: external_authors,
        },
        books,
        next,
        local: local_only,
        provider: (!local_only).then(|| {
            answered_by
                .unwrap_or(state.registry.metadata().name())
                .to_string()
        }),
    }))
}

/// Authors shared by the title-matching books (J. K. Rowling for "Harry
/// Potter"). Durable rows are attached to the local list; unknown authors
/// become transient hits that can be followed by name.
#[allow(clippy::too_many_arguments)]
async fn append_derived_book_authors(
    state: &AppState,
    user_id: i64,
    books: &[DiscoveryResult],
    query: &str,
    local_authors: &mut Vec<AuthorHit>,
    external_authors: &mut Vec<AuthorHit>,
) -> Result<(), AppError> {
    let needle =
        bokhylle_core::identity::normalize_text(&bokhylle_core::identity::core_title(query));
    if needle.is_empty() {
        return Ok(());
    }

    let mut counts: Vec<(String, usize, String)> = Vec::new();
    let mut matching = 0usize;
    for book in books
        .iter()
        .filter(|book| {
            bokhylle_core::identity::normalize_text(&bokhylle_core::identity::core_title(
                &book.title,
            ))
            .starts_with(&needle)
        })
        .take(8)
    {
        matching += 1;
        for author in &book.authors {
            let normalized = bokhylle_core::identity::normalize_text(author);
            if normalized.is_empty() {
                continue;
            }
            match counts.iter_mut().find(|(name, _, _)| *name == normalized) {
                Some((_, count, _)) => *count += 1,
                None => counts.push((normalized, 1, author.clone())),
            }
        }
    }
    if matching == 0 {
        return Ok(());
    }
    // Two matching books must agree on the author; a lone match still names
    // its own author.
    let threshold = if matching >= 2 { 2 } else { 1 };
    counts.retain(|(_, count, _)| *count >= threshold);
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.2.cmp(&right.2)));

    let known: Vec<String> = local_authors
        .iter()
        .chain(external_authors.iter())
        .map(|author| bokhylle_core::identity::normalize_text(&author.name))
        .collect();

    for (normalized, _, display) in counts {
        if known.contains(&normalized) {
            continue;
        }
        let row: Option<(i64, String, i64, i64)> = sqlx::query_as(
            "SELECT a.id, a.name,
                    (SELECT COUNT(*) FROM author_follows f
                     WHERE f.author_id = a.id AND f.user_id = ?),
                    (SELECT COUNT(DISTINCT ba.book_id) FROM book_authors ba
                     JOIN books b ON b.id = ba.book_id
                     WHERE ba.author_id = a.id AND (b.sharing_managed = 0 OR EXISTS (
                        SELECT 1 FROM book_access access WHERE access.book_id = b.id
                        AND (access.user_id = ? OR access.sharing = 'shared'))))
             FROM authors a WHERE a.normalized_name = ?",
        )
        .bind(user_id)
        .bind(user_id)
        .bind(&normalized)
        .fetch_optional(&state.db)
        .await?;
        match row {
            Some((id, name, following, book_count)) => local_authors.push(AuthorHit {
                author_id: Some(id),
                name,
                following: following > 0,
                book_count,
                provider: None,
                provider_key: None,
            }),
            None => external_authors.push(AuthorHit {
                author_id: None,
                name: display,
                following: false,
                book_count: 0,
                provider: Some(state.registry.metadata().name().to_string()),
                provider_key: None,
            }),
        }
    }
    Ok(())
}

fn suppress_author_noise(
    books: &[DiscoveryResult],
    query: &str,
    external: Vec<AuthorHit>,
) -> Vec<AuthorHit> {
    let needle =
        bokhylle_core::identity::normalize_text(&bokhylle_core::identity::core_title(query));
    let strong_title_matches = needle.len() >= 2
        && books
            .iter()
            .filter(|book| {
                bokhylle_core::identity::normalize_text(&bokhylle_core::identity::core_title(
                    &book.title,
                ))
                .starts_with(&needle)
            })
            .count()
            >= 2;
    if !strong_title_matches {
        return external;
    }
    // An author of the matching books stays relevant (J. K. Rowling for
    // "Harry Potter"); a provider-only namesake does not.
    let book_authors: std::collections::HashSet<String> = books
        .iter()
        .filter(|book| {
            bokhylle_core::identity::normalize_text(&bokhylle_core::identity::core_title(
                &book.title,
            ))
            .starts_with(&needle)
        })
        .flat_map(|book| book.authors.iter())
        .map(|author| bokhylle_core::identity::normalize_text(author))
        .collect();
    external
        .into_iter()
        .filter(|author| {
            let name = bokhylle_core::identity::normalize_text(&author.name);
            book_authors.contains(&name) || author.book_count > 0 || author.following
        })
        .collect()
}

pub async fn search_page(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<SearchPageParams>,
) -> Result<Json<SearchPage>, AppError> {
    let query = params.q.unwrap_or_default();
    let query = query.trim();
    if query.is_empty() {
        return Err(AppError::BadRequest(
            "search query must not be empty".to_string(),
        ));
    }

    let kind = SearchKind::parse(params.kind.as_deref().unwrap_or("any"))
        .ok_or_else(|| AppError::BadRequest("invalid search type".to_string()))?;

    let (items, next, _provider) = discovery::search_page(
        &state,
        kind,
        query,
        params.limit.unwrap_or(24),
        user.id,
        params.continuation.as_deref(),
    )
    .await?;
    Ok(Json(SearchPage { items, next }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseParams {
    pub provider_key: String,
    pub provider: Option<String>,
    pub format: Option<String>,
}

pub use crate::services::releases::ReleasesResponse;

pub async fn releases(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<ReleaseParams>,
) -> Result<Json<ReleasesResponse>, AppError> {
    Ok(Json(
        crate::services::releases::preview(
            &state,
            &user,
            params.provider.as_deref(),
            &params.provider_key,
            params.format,
        )
        .await?,
    ))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookParams {
    pub provider_key: String,
    pub provider: Option<String>,
}

pub async fn book(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<BookParams>,
) -> Result<Json<discovery::DiscoveryDetail>, AppError> {
    let Some(mut detail) = discovery::detail_visible(
        &state,
        params.provider.as_deref(),
        &params.provider_key,
        user.id,
    )
    .await?
    else {
        return Err(AppError::NotFound("book not found".to_string()));
    };
    if let Some(book_id) = detail.owned_book_id {
        detail.on_shelf = crate::user_books::contains(&state.db, user.id, book_id).await?;
        detail.liked = crate::user_books::preference(&state.db, user.id, book_id).await?
            == Some("liked".to_string());
    }
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LikeInput {
    pub provider_key: String,
    pub provider: Option<String>,
}

/// Records a taste signal for a provider book without owning or acquiring it:
/// the metadata is upserted locally and the preference row stays off-shelf.
pub async fn like(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<LikeInput>,
) -> Result<Json<LikedDiscovery>, AppError> {
    let Some(metadata) =
        discovery::resolve_metadata(&state, body.provider.as_deref(), &body.provider_key).await?
    else {
        return Err(AppError::NotFound("book not found".to_string()));
    };
    let book_id = import_metadata::upsert_book_from_metadata(&state.db, &metadata).await?;
    crate::user_books::set_preference(&state.db, user.id, book_id, Some("liked")).await?;
    Ok(Json(LikedDiscovery {
        book_id,
        preference: "liked",
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CoverParams {
    pub title: Option<String>,
    pub provider: Option<String>,
    pub size: Option<String>,
}

pub async fn cover(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(cover_id): Path<String>,
    Query(params): Query<CoverParams>,
) -> Result<Response, AppError> {
    if cover_id.is_empty() {
        return Err(AppError::BadRequest("invalid cover id".to_string()));
    }

    // Cover identifiers are provider-specific: Open Library uses numeric ids,
    // Google Books uses full thumbnail URLs. The registry applies the
    // capability policy: artwork may only come from a metadata provider.
    let provider = params
        .provider
        .as_deref()
        .unwrap_or_else(|| state.registry.default_cover_provider());
    let Some(client) = state.registry.covers(provider) else {
        return Ok(placeholder_response(params.title.as_deref()));
    };

    let large = params.size.as_deref() == Some("large");
    let cached_path = provider_cover_cache_path(&state, provider, &cover_id, large);
    let missing_path = cached_path.with_extension("missing");

    if let Ok(bytes) = tokio::fs::read(&cached_path).await
        && bokhylle_library::covers::usable_cover(&bytes)
    {
        return Ok(image_response(bytes));
    }
    if image_cache::recently_missing(&missing_path).await {
        return Ok(placeholder_response(params.title.as_deref()));
    }

    let fetched = if large {
        client.fetch_cover(&cover_id).await
    } else {
        client.fetch_cover_thumbnail(&cover_id).await
    };
    match fetched {
        Ok(Some(bytes)) if bokhylle_library::covers::usable_cover(&bytes) => {
            if let Err(error) = image_cache::write_atomic(&cached_path, &bytes).await {
                tracing::warn!(%error, cover_id, "discovery.cover.cache_write_failed");
            }
            Ok(image_response(bytes))
        }
        Ok(_) => {
            if let Err(error) = image_cache::mark_missing(&missing_path).await {
                tracing::warn!(%error, cover_id, "discovery.cover.negative_cache_write_failed");
            }
            Ok(placeholder_response(params.title.as_deref()))
        }
        Err(error) => {
            tracing::warn!(%error, cover_id, "discovery.cover.fetch_failed");
            Ok(placeholder_response(params.title.as_deref()))
        }
    }
}

pub(crate) fn provider_cover_cache_path(
    state: &AppState,
    provider: &str,
    cover_id: &str,
    large: bool,
) -> std::path::PathBuf {
    let mut provider_tag: String = provider
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '_' || *character == '-'
        })
        .take(24)
        .collect();
    if provider_tag.is_empty() {
        provider_tag = "unknown".into();
    }
    let digest = hex::encode(Sha256::digest(cover_id.as_bytes()));
    let variant = if large { "large" } else { "medium" };
    state
        .paths
        .config_dir
        .join("cache")
        .join("provider-covers")
        .join(format!("cover-{provider_tag}-{digest}-{variant}"))
}

pub(crate) fn image_extension(bytes: &[u8]) -> &'static str {
    match image_content_type(bytes) {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "jpg",
    }
}

fn image_content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "image/jpeg"
    }
}

fn image_response(bytes: Vec<u8>) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(image_content_type(&bytes)),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=604800"),
            ),
        ],
        bytes,
    )
        .into_response()
}

fn placeholder_response(title: Option<&str>) -> Response {
    let svg = bokhylle_library::covers::placeholder_svg(title.unwrap_or("Unknown"), &[]);
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("image/svg+xml"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        svg,
    )
        .into_response()
}
