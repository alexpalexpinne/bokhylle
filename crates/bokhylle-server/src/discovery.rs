use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod ranking;

use serde::Serialize;
use sqlx::SqlitePool;

use bokhylle_core::identity::normalize_text;
use bokhylle_metadata::{MetadataProvider, MetadataQuery, MetadataResult};

use crate::AppState;
use crate::error::AppError;

pub const SEARCH_TTL_SECONDS: i64 = 24 * 60 * 60;
pub const BOOK_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
/// Enrichment-only providers (Google Books) are cached, not persisted as
/// catalogue knowledge, so their records expire much sooner.
pub const EPHEMERAL_SEARCH_TTL_SECONDS: i64 = 6 * 60 * 60;
pub const EPHEMERAL_BOOK_TTL_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    Any,
    Title,
    Author,
    Isbn,
    Subject,
}

impl SearchKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "any" => Some(Self::Any),
            "title" => Some(Self::Title),
            "author" => Some(Self::Author),
            "isbn" => Some(Self::Isbn),
            "subject" => Some(Self::Subject),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Title => "title",
            Self::Author => "author",
            Self::Isbn => "isbn",
            Self::Subject => "subject",
        }
    }

    fn query(self, text: &str, limit: usize) -> MetadataQuery {
        match self {
            Self::Any
                if text.trim().chars().count() <= 2
                    && text.trim().chars().all(char::is_alphabetic) =>
            {
                MetadataQuery {
                    // Provider full-text search treats short common words such as
                    // "It" as stop words. The title field keeps that intent usable.
                    title: Some(text.to_string()),
                    limit,
                    ..Default::default()
                }
            }
            Self::Any => MetadataQuery {
                free_text: Some(text.to_string()),
                limit,
                ..Default::default()
            },
            Self::Title => MetadataQuery {
                title: Some(text.to_string()),
                limit,
                ..Default::default()
            },
            Self::Author => MetadataQuery {
                author: Some(text.to_string()),
                limit,
                ..Default::default()
            },
            Self::Isbn => MetadataQuery {
                isbn: Some(text.to_string()),
                limit,
                ..Default::default()
            },
            Self::Subject => MetadataQuery {
                free_text: Some(format!("subject:\"{}\"", text.trim())),
                limit,
                ..Default::default()
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiscoveryStatus {
    InLibrary,
    Downloading,
    #[default]
    NotInLibrary,
}

#[derive(Debug, Clone, Serialize, Default, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResult {
    pub provider: String,
    pub provider_key: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub language: Option<String>,
    /// Complete language set when the provider exposes one.
    #[serde(default)]
    pub languages: Vec<String>,
    pub isbn10: Option<String>,
    pub isbn13: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
    pub cover_id: Option<String>,
    /// Ranking signals; missing values simply contribute no bonus.
    #[serde(default)]
    pub rating_average: Option<f64>,
    #[serde(default)]
    pub rating_count: Option<i64>,
    #[serde(default)]
    pub edition_count: Option<i64>,
    #[serde(default)]
    pub popularity: Option<i64>,
    pub status: DiscoveryStatus,
    pub owned_book_id: Option<i64>,
    pub owned_file_id: Option<i64>,
    pub on_shelf: bool,
}

pub async fn search(
    state: &AppState,
    kind: SearchKind,
    text: &str,
    limit: usize,
    viewer_id: i64,
) -> Result<Vec<DiscoveryResult>, AppError> {
    Ok(search_page(state, kind, text, limit, viewer_id, None)
        .await?
        .0)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CachedPage {
    items: Vec<MetadataResult>,
    next: Option<String>,
}

fn parse_cached_page(value: &str) -> Result<CachedPage, AppError> {
    // Older cache rows stored a bare array; accept both shapes.
    if value.trim_start().starts_with('[') {
        let items: Vec<MetadataResult> = serde_json::from_str(value)
            .map_err(|error| AppError::Unprocessable(error.to_string()))?;
        return Ok(CachedPage { items, next: None });
    }
    serde_json::from_str(value).map_err(|error| AppError::Unprocessable(error.to_string()))
}

/// One provider's page, through the cache. The provider name is part of the
/// cache key, so a fallback result never masquerades as a primary one.
async fn provider_page(
    state: &AppState,
    provider: &Arc<dyn MetadataProvider>,
    kind: SearchKind,
    text: &str,
    limit: usize,
    continuation: Option<&str>,
) -> Result<CachedPage, AppError> {
    let mut query = kind.query(text, limit);
    query.continuation = continuation.map(str::to_string);
    let cache_key = format!(
        "search:{}:{}:{}:{}",
        provider.name(),
        query.cache_fragment(),
        limit,
        continuation.unwrap_or("")
    );

    let cached = read_cache(&state.db, &cache_key).await?;
    let fresh = cached
        .as_ref()
        .is_some_and(|(_, expires_at)| *expires_at > now_epoch());
    if fresh {
        let (value, _) = cached.expect("cached value checked above");
        tracing::info!(cache = "hit", "discovery.search.completed");
        return parse_cached_page(&value);
    }

    match provider.search_page(&query).await {
        Ok(mut page) => {
            // Provider indexes choke on subtitles and punctuation
            // ("Title: A Subtitle"), so an empty title search retries
            // with the core title before giving up.
            if page.items.is_empty() && continuation.is_none() && matches!(kind, SearchKind::Title)
            {
                let core = bokhylle_core::identity::core_title(text);
                let core = core.trim().to_string();
                if !core.is_empty() && core != text.trim() {
                    let mut fallback = kind.query(&core, limit);
                    fallback.continuation = None;
                    if let Ok(fallback_page) = provider.search_page(&fallback).await {
                        page = fallback_page;
                    }
                }
            }
            let cached_page = CachedPage {
                items: page.items,
                next: page.next,
            };
            let serialized = serde_json::to_string(&cached_page)
                .map_err(|error| AppError::Unprocessable(error.to_string()))?;
            let ttl = if provider.capabilities().persistent_metadata {
                SEARCH_TTL_SECONDS
            } else {
                EPHEMERAL_SEARCH_TTL_SECONDS
            };
            write_cache(&state.db, &cache_key, &serialized, ttl).await?;
            tracing::info!(
                results = cached_page.items.len(),
                provider = provider.name(),
                cache = "miss",
                "discovery.search.completed"
            );
            Ok(cached_page)
        }
        Err(error) => match cached {
            Some((value, _)) => {
                tracing::warn!(%error, "discovery.cache.stale");
                parse_cached_page(&value)
            }
            None => {
                tracing::warn!(%error, provider = provider.name(), "discovery.search.failed");
                Err(AppError::Unavailable(
                    "the metadata provider is currently unavailable".to_string(),
                ))
            }
        },
    }
}

/// Continuation tokens are prefixed with the provider that produced them, so
/// loading more of a fallback page never re-queries the primary.
const CONTINUATION_SEPARATOR: char = '\u{1f}';

fn split_tagged_continuation(continuation: &str) -> Option<(&str, &str)> {
    continuation.split_once(CONTINUATION_SEPARATOR)
}

fn pinned_provider(
    state: &AppState,
    continuation: &str,
) -> Option<(Arc<dyn MetadataProvider>, String)> {
    let (name, token) = split_tagged_continuation(continuation)?;
    state
        .registry
        .provider(name)
        .map(|provider| (provider, token.to_string()))
}

fn tag_continuation(provider: &str, next: Option<String>) -> Option<String> {
    next.map(|token| format!("{provider}{CONTINUATION_SEPARATOR}{token}"))
}

const POOL_CURSOR_PREFIX: &str = "pool:";

fn pool_continuation(provider: &str, offset: usize) -> Option<String> {
    tag_continuation(provider, Some(format!("{POOL_CURSOR_PREFIX}{offset}")))
}

/// Which provider(s) a search may contact. Explicit selection is
/// authoritative: no hidden fallback is ever queried behind it.
#[derive(Debug, Clone, Copy)]
pub enum ProviderSelection<'a> {
    /// The configured primary, with the existing Automatic gap-filling.
    Automatic,
    /// Exactly this provider (resolved from the registry).
    Only(&'a str),
}

/// Pageable discovery search. Automatic mode fills gaps: the primary answers
/// normally, and the configured fallback is only asked when the primary has
/// nothing for this query.
pub type SearchResponse = (Vec<DiscoveryResult>, Option<String>, &'static str);

pub async fn search_page(
    state: &AppState,
    kind: SearchKind,
    text: &str,
    limit: usize,
    viewer_id: i64,
    continuation: Option<&str>,
) -> Result<SearchResponse, AppError> {
    search_page_selected(
        state,
        kind,
        text,
        limit,
        viewer_id,
        continuation,
        ProviderSelection::Automatic,
    )
    .await
}

/// Rebuilds the bounded first candidate pool from cached provider metadata.
/// Pool cursors replay this ordering so every fetched candidate is reachable
/// before advancing the provider's own cursor.
async fn ranked_first_pool(
    state: &AppState,
    provider: &Arc<dyn MetadataProvider>,
    page: &CachedPage,
    kind: SearchKind,
    text: &str,
    viewer_id: i64,
) -> Result<Vec<DiscoveryResult>, AppError> {
    let mut results = page.items.clone();
    let query_title = normalize_text(text);
    let provider_leads_with_exact_title = results.first().is_some_and(|item| {
        let title = normalize_text(&item.title);
        ranking::without_leading_article(&title) == ranking::without_leading_article(&query_title)
    });
    // A series name can index novels that do not repeat it in their titles.
    // Preserve an exact work already leading the provider's results.
    if kind == SearchKind::Any
        && provider.name() == "openlibrary"
        && !provider_leads_with_exact_title
        && (2..=6).contains(&text.split_whitespace().count())
        && text.len() <= 80
    {
        let series_text = format!("series:\"{}\"", text.replace('"', ""));
        if let Ok(series_page) =
            provider_page(state, provider, SearchKind::Any, &series_text, 6, None).await
        {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for item in &series_page.items {
                if let Some(author) = item.authors.first() {
                    *counts.entry(normalize_text(author)).or_default() += 1;
                }
            }
            let dominant = counts
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .filter(|(_, count)| *count >= 2)
                .map(|(author, _)| author);
            if let Some(dominant) = dominant {
                for mut item in series_page.items {
                    if !item
                        .authors
                        .iter()
                        .any(|author| normalize_text(author) == dominant)
                    {
                        continue;
                    }
                    if let Some(existing) = results
                        .iter_mut()
                        .find(|existing| existing.provider_key == item.provider_key)
                    {
                        if existing.series.is_none() {
                            existing.series = Some(text.to_string());
                        }
                    } else {
                        item.series.get_or_insert_with(|| text.to_string());
                        results.push(item);
                    }
                }
            }
        }
    }
    ranking::rank_metadata(&mut results, kind, text);
    let mut resolved = resolve_owned(&state.db, results, viewer_id).await?;
    match local_matches(state, kind, text, viewer_id).await {
        Ok(local) if !local.is_empty() => {
            let provider_ids: HashSet<i64> = resolved
                .iter()
                .filter_map(|item| item.owned_book_id)
                .collect();
            let missing: Vec<DiscoveryResult> = local
                .into_iter()
                .filter(|item| {
                    item.owned_book_id
                        .is_none_or(|id| !provider_ids.contains(&id))
                })
                .collect();
            resolved.splice(0..0, missing);
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "discovery.local_matches_failed"),
    }
    ranking::rank_books(&mut resolved, kind, text);
    Ok(resolved)
}

pub async fn search_page_selected(
    state: &AppState,
    kind: SearchKind,
    text: &str,
    limit: usize,
    viewer_id: i64,
    continuation: Option<&str>,
    selection: ProviderSelection<'_>,
) -> Result<SearchResponse, AppError> {
    let limit = limit.clamp(1, 50);
    tracing::info!(kind = kind.as_str(), "discovery.search.started");

    let explicit = match selection {
        ProviderSelection::Automatic => None,
        ProviderSelection::Only(name) => {
            Some(state.registry.provider(name).ok_or_else(|| {
                AppError::BadRequest(format!("unknown metadata provider '{name}'"))
            })?)
        }
    };
    // The first page needs more candidates than the UI shows so Bokhylle's own
    // ranking can lift books the provider ordered lower.
    let provider_limit = if continuation.is_none() {
        limit.max(ranking::FIRST_PAGE_CANDIDATES)
    } else {
        limit
    };

    if continuation.is_some_and(|value| pinned_provider(state, value).is_none()) {
        return Err(AppError::BadRequest(
            "invalid search continuation".to_string(),
        ));
    }

    if let Some((provider, token)) = continuation.and_then(|value| pinned_provider(state, value)) {
        if explicit
            .as_ref()
            .is_some_and(|selected| selected.name() != provider.name())
        {
            return Err(AppError::BadRequest(
                "invalid search continuation".to_string(),
            ));
        }
        let first_page = provider_page(
            state,
            &provider,
            kind,
            text,
            limit.max(ranking::FIRST_PAGE_CANDIDATES),
            None,
        )
        .await?;
        let first_pool =
            ranked_first_pool(state, &provider, &first_page, kind, text, viewer_id).await?;
        if let Some(raw_offset) = token.strip_prefix(POOL_CURSOR_PREFIX) {
            let offset = raw_offset
                .parse::<usize>()
                .ok()
                .filter(|value| *value > 0 && *value < first_pool.len())
                .ok_or_else(|| AppError::BadRequest("invalid search continuation".to_string()))?;
            let end = (offset + limit).min(first_pool.len());
            let next = if end < first_pool.len() {
                pool_continuation(provider.name(), end)
            } else {
                tag_continuation(provider.name(), first_page.next)
            };
            return Ok((first_pool[offset..end].to_vec(), next, provider.name()));
        }

        let page =
            provider_page(state, &provider, kind, text, provider_limit, Some(&token)).await?;
        let mut results = page.items;
        ranking::rank_metadata(&mut results, kind, text);
        let mut resolved = resolve_owned(&state.db, results, viewer_id).await?;
        let first_keys: HashSet<_> = first_pool
            .iter()
            .map(|item| (&item.provider, &item.provider_key))
            .collect();
        let first_owned_ids: HashSet<_> = first_pool
            .iter()
            .filter_map(|item| item.owned_book_id)
            .collect();
        resolved.retain(|item| {
            !first_keys.contains(&(&item.provider, &item.provider_key))
                && item
                    .owned_book_id
                    .is_none_or(|id| !first_owned_ids.contains(&id))
        });
        resolved.truncate(limit);
        return Ok((
            resolved,
            tag_continuation(provider.name(), page.next),
            provider.name(),
        ));
    }

    let (active, page) = if let Some(provider) = explicit {
        let page =
            provider_page(state, &provider, kind, text, provider_limit, continuation).await?;
        (provider, page)
    } else {
        let primary = state.registry.metadata().clone();
        let mut active = primary.clone();
        let mut page =
            match provider_page(state, &primary, kind, text, provider_limit, continuation).await {
                Ok(page) => page,
                Err(primary_error) => match state.registry.fallback() {
                    Some(fallback) => {
                        active = fallback.clone();
                        provider_page(state, fallback, kind, text, provider_limit, continuation)
                            .await
                            .map_err(|_| primary_error)?
                    }
                    None => return Err(primary_error),
                },
            };

        if page.items.is_empty()
            && let Some(fallback) = state.registry.fallback()
        {
            match provider_page(state, fallback, kind, text, provider_limit, continuation).await {
                Ok(fallback_page) => {
                    if !fallback_page.items.is_empty() {
                        active = fallback.clone();
                        page = fallback_page;
                    }
                }
                Err(error) => tracing::warn!(%error, "discovery.search.fallback_failed"),
            }
        }
        (active, page)
    };

    let mut resolved = ranked_first_pool(state, &active, &page, kind, text, viewer_id).await?;
    let next = if resolved.len() > limit {
        pool_continuation(active.name(), limit)
    } else {
        tag_continuation(active.name(), page.next)
    };
    resolved.truncate(limit);
    Ok((resolved, next, active.name()))
}

/// Provider-only results for the request catalogue: metadata and covers, no
/// household ownership, availability or release information.
pub async fn external_results(
    state: &AppState,
    kind: SearchKind,
    text: &str,
    limit: usize,
) -> Result<Vec<DiscoveryResult>, AppError> {
    let limit = limit.clamp(1, 40);
    let primary = state.registry.metadata().clone();
    let page = provider_page(state, &primary, kind, text, limit, None).await?;
    let mut results = page.items;
    ranking::rank_metadata(&mut results, kind, text);
    let mut mapped: Vec<DiscoveryResult> = results
        .into_iter()
        .map(|result| DiscoveryResult {
            provider: result.provider,
            provider_key: result.provider_key,
            title: result.title,
            authors: result.authors,
            year: result.year,
            language: result.language,
            languages: result.languages,
            isbn10: result.isbn10,
            isbn13: result.isbn13,
            series: result.series,
            series_number: result.series_number,
            cover_id: result.cover_id,
            rating_average: result.rating_average,
            rating_count: result.rating_count,
            edition_count: result.edition_count,
            popularity: result.popularity,
            status: DiscoveryStatus::NotInLibrary,
            owned_book_id: None,
            owned_file_id: None,
            on_shelf: false,
        })
        .collect();
    ranking::rank_books(&mut mapped, kind, text);
    Ok(mapped)
}

/// Catalogue-only matches for the local fast path (no provider call).
pub async fn local_books(
    state: &AppState,
    kind: SearchKind,
    text: &str,
    viewer_id: i64,
) -> Result<Vec<DiscoveryResult>, AppError> {
    local_matches(state, kind, text, viewer_id).await
}

/// Durable catalogue matches for a Discover query, annotated with ownership,
/// so known books surface before provider results. Only the first page carries
/// them; continuation pages stay provider-only.
async fn local_matches(
    state: &AppState,
    kind: SearchKind,
    text: &str,
    viewer_id: i64,
) -> Result<Vec<DiscoveryResult>, AppError> {
    type LocalRow = (
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    );

    let rows: Vec<LocalRow> = match kind {
        SearchKind::Isbn => {
            let isbn = text.trim();
            sqlx::query_as(
                "SELECT b.id, b.title,
                        COALESCE((SELECT group_concat(a.name, ', ')
                                  FROM book_authors ba JOIN authors a ON a.id = ba.author_id
                                  WHERE ba.book_id = b.id), ''),
                        b.language, b.series, b.series_number,
                        (SELECT publication_year FROM editions
                         WHERE book_id = b.id AND publication_year IS NOT NULL
                         ORDER BY id LIMIT 1)
                 FROM editions e JOIN books b ON b.id = e.book_id
                 WHERE e.isbn13 = ? OR e.isbn10 = ?
                 ORDER BY b.created_at DESC, b.id DESC
                 LIMIT 12",
            )
            .bind(isbn)
            .bind(isbn)
            .fetch_all(&state.db)
            .await?
        }
        SearchKind::Subject => Vec::new(),
        _ => {
            let column = match kind {
                SearchKind::Title => Some("title"),
                SearchKind::Author => Some("author"),
                _ => None,
            };
            let fts = crate::library::queries::fts_query_in(text, column);
            if fts.is_empty() {
                Vec::new()
            } else {
                sqlx::query_as(
                    "SELECT b.id, b.title,
                            COALESCE((SELECT group_concat(a.name, ', ')
                                      FROM book_authors ba JOIN authors a ON a.id = ba.author_id
                                      WHERE ba.book_id = b.id), ''),
                            b.language, b.series, b.series_number,
                            (SELECT publication_year FROM editions
                             WHERE book_id = b.id AND publication_year IS NOT NULL
                             ORDER BY id LIMIT 1)
                     FROM books b
                     WHERE b.id IN (SELECT rowid FROM books_fts WHERE books_fts MATCH ?)
                     ORDER BY b.created_at DESC, b.id DESC
                     LIMIT 12",
                )
                .bind(fts)
                .fetch_all(&state.db)
                .await?
            }
        }
    };

    let mut output = Vec::with_capacity(rows.len());
    let durable = state.registry.durable_providers();
    for (id, title, authors, language, series, series_number, publication_year) in rows {
        let (provider, provider_key) =
            match crate::external_ids::book_provider(&state.db, id, &durable).await? {
                Some((provider, provider_key)) => (Some(provider), Some(provider_key)),
                None => (None, None),
            };
        let has_files = has_files(&state.db, id).await?;
        let work = provider.as_deref() == Some("openlibrary")
            && provider_key
                .as_deref()
                .is_some_and(|key| key.starts_with("/works/"));
        let (language, languages, publication_year) = if has_files {
            let file_editions: Vec<(Option<String>, Option<i64>)> = sqlx::query_as(
                "SELECT e.language, e.publication_year
                 FROM book_files f JOIN editions e ON e.id = f.edition_id
                 WHERE e.book_id = ? ORDER BY f.id",
            )
            .bind(id)
            .fetch_all(&state.db)
            .await?;
            let mut languages = Vec::new();
            for (language, _) in &file_editions {
                if let Some(language) = language
                    && !languages.contains(language)
                {
                    languages.push(language.clone());
                }
            }
            let year = file_editions.iter().filter_map(|(_, year)| *year).min();
            (languages.first().cloned(), languages, year)
        } else if work {
            let languages: Vec<String> = sqlx::query_scalar(
                "SELECT language FROM book_available_languages WHERE book_id = ? ORDER BY language",
            )
            .bind(id)
            .fetch_all(&state.db)
            .await?;
            (None, languages, None)
        } else {
            (
                language.clone(),
                language.into_iter().collect(),
                publication_year,
            )
        };
        let demo_pending = state.demo.is_some()
            && sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM demo_gets
                 WHERE user_id = ? AND book_id = ? AND completed_at IS NULL)",
            )
            .bind(viewer_id)
            .bind(id)
            .fetch_one(&state.db)
            .await?;
        let status = if demo_pending {
            DiscoveryStatus::Downloading
        } else if has_files {
            DiscoveryStatus::InLibrary
        } else if has_active_acquisition(&state.db, id).await? {
            DiscoveryStatus::Downloading
        } else {
            DiscoveryStatus::NotInLibrary
        };
        output.push(DiscoveryResult {
            provider: provider.unwrap_or_else(|| "local".to_string()),
            provider_key: provider_key.unwrap_or_else(|| format!("local:{id}")),
            title,
            authors: authors
                .split(", ")
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect(),
            year: publication_year.map(|year| year as i32),
            languages,
            language,
            isbn10: None,
            isbn13: None,
            series,
            series_number,
            cover_id: None,
            status,
            owned_book_id: Some(id),
            owned_file_id: if has_files {
                primary_file_id(&state.db, id).await?
            } else {
                None
            },
            on_shelf: crate::user_books::contains(&state.db, viewer_id, id).await?,
            ..Default::default()
        });
    }

    let needle = normalize_text(&bokhylle_core::identity::core_title(text));
    output.sort_by_key(|item| {
        let title = normalize_text(&bokhylle_core::identity::core_title(&item.title));
        if title == needle {
            0
        } else if title.starts_with(&needle) {
            1
        } else {
            2
        }
    });
    Ok(output)
}

async fn resolve_owned(
    pool: &SqlitePool,
    results: Vec<MetadataResult>,
    viewer_id: i64,
) -> Result<Vec<DiscoveryResult>, AppError> {
    let mut seen_books: Vec<i64> = Vec::new();
    let mut output = Vec::with_capacity(results.len());
    let mut dropped = 0usize;

    for result in results {
        let book_id = find_owned_book(pool, &result).await?;
        let mut status = DiscoveryStatus::NotInLibrary;
        let mut owned_file_id = None;

        if let Some(book_id) = book_id {
            if seen_books.contains(&book_id) {
                dropped += 1;
                continue;
            }
            seen_books.push(book_id);

            if has_files(pool, book_id).await? {
                status = DiscoveryStatus::InLibrary;
                owned_file_id = primary_file_id(pool, book_id).await?;
            } else if has_active_acquisition(pool, book_id).await? {
                status = DiscoveryStatus::Downloading;
            }
        }

        output.push(DiscoveryResult {
            provider: result.provider,
            provider_key: result.provider_key,
            title: result.title,
            authors: result.authors,
            year: result.year,
            language: result.language,
            languages: result.languages,
            isbn10: result.isbn10,
            isbn13: result.isbn13,
            series: result.series,
            series_number: result.series_number,
            cover_id: result.cover_id,
            rating_average: result.rating_average,
            rating_count: result.rating_count,
            edition_count: result.edition_count,
            popularity: result.popularity,
            status,
            owned_book_id: book_id,
            owned_file_id,
            on_shelf: match book_id {
                Some(book_id) => crate::user_books::contains(pool, viewer_id, book_id).await?,
                None => false,
            },
        });
    }

    if dropped > 0 {
        tracing::info!(dropped, "discovery.deduplicated");
    }

    Ok(output)
}

async fn find_owned_book(
    pool: &SqlitePool,
    result: &MetadataResult,
) -> Result<Option<i64>, AppError> {
    // Books added through Discover keep their provider link, which is the
    // only reliable match when Open Library titles carry subtitles. The
    // identity table is authoritative; editions remain the fallback.
    let provider_key = result.provider_key.trim();
    if !provider_key.is_empty()
        && let Some(book_id) =
            crate::external_ids::book_by_provider(pool, &result.provider, provider_key).await?
    {
        return Ok(Some(book_id));
    }
    if !provider_key.is_empty()
        && let Some(book_id) = sqlx::query_scalar(
            "SELECT book_id FROM editions
             WHERE provider = ? AND provider_key = ? LIMIT 1",
        )
        .bind(&result.provider)
        .bind(provider_key)
        .fetch_optional(pool)
        .await?
    {
        return Ok(Some(book_id));
    }

    if let Some(isbn13) = &result.isbn13
        && let Some(book_id) = sqlx::query_scalar("SELECT book_id FROM editions WHERE isbn13 = ?")
            .bind(isbn13)
            .fetch_optional(pool)
            .await?
    {
        return Ok(Some(book_id));
    }

    if let Some(isbn10) = &result.isbn10
        && let Some(book_id) = sqlx::query_scalar("SELECT book_id FROM editions WHERE isbn10 = ?")
            .bind(isbn10)
            .fetch_optional(pool)
            .await?
    {
        return Ok(Some(book_id));
    }

    let title = normalize_text(&result.title);
    if title.is_empty() || result.authors.is_empty() {
        return Ok(None);
    }

    let mut candidates: Vec<i64> = Vec::new();
    for author in &result.authors {
        let normalized = normalize_text(author);
        if normalized.is_empty() {
            continue;
        }

        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT b.id
             FROM books b
             JOIN book_authors ba ON ba.book_id = b.id
             JOIN authors a ON a.id = ba.author_id
             WHERE b.normalized_title = ? AND a.normalized_name = ?",
        )
        .bind(&title)
        .bind(&normalized)
        .fetch_all(pool)
        .await?;
        candidates.extend(ids);
    }

    candidates.sort_unstable();
    candidates.dedup();
    Ok(candidates.first().copied())
}

/// Best local file for the book (EPUB before PDF), used by Discover so owned
/// entries can open their sheet from local data without an Open Library call.
async fn primary_file_id(pool: &SqlitePool, book_id: i64) -> Result<Option<i64>, AppError> {
    Ok(sqlx::query_scalar(
        "SELECT f.id
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ?
         ORDER BY CASE f.format WHEN 'epub' THEN 0 ELSE 1 END, f.id
         LIMIT 1",
    )
    .bind(book_id)
    .fetch_optional(pool)
    .await?)
}

async fn has_files(pool: &SqlitePool, book_id: i64) -> Result<bool, AppError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM book_files f
            JOIN editions e ON e.id = f.edition_id
            WHERE e.book_id = ?
         )",
    )
    .bind(book_id)
    .fetch_one(pool)
    .await?;
    Ok(exists != 0)
}

async fn has_active_acquisition(pool: &SqlitePool, book_id: i64) -> Result<bool, AppError> {
    let query = format!(
        "SELECT EXISTS (
             SELECT 1 FROM acquisitions
             WHERE book_id = ? AND status IN ({})
         )",
        bokhylle_acquisition::state::AcquisitionStatus::protected_states_sql()
    );
    let exists: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
        .bind(book_id)
        .fetch_one(pool)
        .await?;
    Ok(exists != 0)
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryDetail {
    pub provider: String,
    pub provider_key: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub language: Option<String>,
    pub languages: Vec<String>,
    pub isbn10: Option<String>,
    pub isbn13: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
    pub cover_id: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub status: DiscoveryStatus,
    pub owned_book_id: Option<i64>,
    pub owned_file_id: Option<i64>,
    pub on_shelf: bool,
    pub liked: bool,
}

/// Provider metadata for one discovery key, cached in the local store. The
/// provider is resolved through the registry, so a fallback result stays a
/// fallback result through detail, likes and acquisition.
pub async fn resolve_metadata(
    state: &AppState,
    provider: Option<&str>,
    provider_key: &str,
) -> Result<Option<MetadataResult>, AppError> {
    let provider_key = provider_key.trim();
    if provider_key.is_empty() {
        return Ok(None);
    }
    let client = match provider {
        Some(name) => state
            .registry
            .provider(name)
            .ok_or_else(|| AppError::BadRequest("unknown metadata provider".to_string()))?,
        None => state.registry.metadata().clone(),
    };

    match cached_book(&state.db, client.name(), provider_key).await? {
        Some(metadata) => Ok(Some(metadata)),
        None => {
            let Some(metadata) = client.get_book(provider_key).await.map_err(|error| {
                tracing::warn!(%error, "discovery.book_fetch_failed");
                AppError::Unavailable("the metadata provider is currently unavailable".to_string())
            })?
            else {
                return Ok(None);
            };
            store_book(state, &metadata).await?;
            Ok(Some(metadata))
        }
    }
}

pub async fn detail(
    state: &AppState,
    provider: Option<&str>,
    provider_key: &str,
) -> Result<Option<DiscoveryDetail>, AppError> {
    let Some(metadata) = resolve_metadata(state, provider, provider_key).await? else {
        return Ok(None);
    };

    let mut status = DiscoveryStatus::NotInLibrary;
    let mut owned_book_id = None;
    let mut owned_file_id = None;
    if let Some(book_id) = find_owned_book(&state.db, &metadata).await? {
        owned_book_id = Some(book_id);
        if has_files(&state.db, book_id).await? {
            status = DiscoveryStatus::InLibrary;
            owned_file_id = sqlx::query_scalar(
                "SELECT f.id FROM book_files f
                 JOIN editions e ON e.id = f.edition_id
                 WHERE e.book_id = ?
                 ORDER BY CASE f.format WHEN 'epub' THEN 0 ELSE 1 END, f.id
                 LIMIT 1",
            )
            .bind(book_id)
            .fetch_optional(&state.db)
            .await?;
        } else if has_active_acquisition(&state.db, book_id).await? {
            status = DiscoveryStatus::Downloading;
        }
    }

    Ok(Some(DiscoveryDetail {
        provider: metadata.provider,
        provider_key: metadata.provider_key,
        title: metadata.title,
        authors: metadata.authors,
        year: metadata.year,
        language: metadata.language,
        languages: metadata.languages,
        isbn10: metadata.isbn10,
        isbn13: metadata.isbn13,
        series: metadata.series,
        series_number: metadata.series_number,
        cover_id: metadata.cover_id,
        description: metadata.description,
        publisher: metadata.publisher,
        status,
        owned_book_id,
        owned_file_id,
        on_shelf: false,
        liked: false,
    }))
}

pub async fn cached_book(
    pool: &SqlitePool,
    provider: &str,
    provider_key: &str,
) -> Result<Option<MetadataResult>, AppError> {
    let key = format!("book:{provider}:{provider_key}");
    let Some((value, _)) = read_cache(pool, &key).await? else {
        return Ok(None);
    };
    Ok(parse_cached_one(&value))
}

/// Persist provider metadata with a TTL that respects the provider's policy:
/// durable providers keep the long TTL, enrichment-only providers the short
/// one.
pub async fn store_book(state: &AppState, result: &MetadataResult) -> Result<(), AppError> {
    let key = format!("book:{}:{}", result.provider, result.provider_key);
    let serialized = serde_json::to_string(result)
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    let ttl = if state.registry.persistent_metadata(&result.provider) {
        BOOK_TTL_SECONDS
    } else {
        EPHEMERAL_BOOK_TTL_SECONDS
    };
    write_cache(&state.db, &key, &serialized, ttl).await
}

async fn read_cache(pool: &SqlitePool, key: &str) -> Result<Option<(String, i64)>, AppError> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT value, expires_at FROM metadata_cache WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

async fn write_cache(
    pool: &SqlitePool,
    key: &str,
    value: &str,
    ttl_seconds: i64,
) -> Result<(), AppError> {
    let now = now_epoch();
    sqlx::query(
        "INSERT INTO metadata_cache (key, value, fetched_at, expires_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(key) DO UPDATE
            SET value = excluded.value,
                fetched_at = excluded.fetched_at,
                expires_at = excluded.expires_at",
    )
    .bind(key)
    .bind(value)
    .bind(now)
    .bind(now + ttl_seconds)
    .execute(pool)
    .await?;
    Ok(())
}

fn parse_cached_one(value: &str) -> Option<MetadataResult> {
    serde_json::from_str(value).ok()
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(title: &str, authors: &[&str], series_number: Option<&str>) -> DiscoveryResult {
        DiscoveryResult {
            provider: "fake".to_string(),
            provider_key: title.to_lowercase().replace(' ', "-"),
            title: title.to_string(),
            authors: authors.iter().map(|author| author.to_string()).collect(),
            series: series_number.map(|_| "The Series".to_string()),
            series_number: series_number.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn continuation_tokens_keep_their_provider() {
        let tagged = tag_continuation("google_books", Some("cursor-2".to_string())).unwrap();
        assert_eq!(
            split_tagged_continuation(&tagged),
            Some(("google_books", "cursor-2"))
        );
        assert!(split_tagged_continuation("plain-cursor").is_none());
        assert_eq!(tag_continuation("openlibrary", None), None);
    }

    #[test]
    fn series_positions_order_equal_scores_through_the_book_ranker() {
        let mut results = vec![
            book("The Series Book Three", &["Author"], Some("3")),
            book("The Series Book One", &["Author"], Some("1")),
            book("The Series Book Two", &["Author"], Some("2")),
        ];
        ranking::rank_books(&mut results, SearchKind::Any, "the series book");
        let titles: Vec<&str> = results.iter().map(|book| book.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "The Series Book One",
                "The Series Book Two",
                "The Series Book Three"
            ]
        );
    }

    #[test]
    fn exact_core_title_matches_come_first() {
        let mut results = vec![
            book("嫌われる勇気", &[], None),
            book(
                "Complete Courage to Be Disliked Duology Boxed Set",
                &[],
                None,
            ),
            book("The Courage to Be Disliked", &[], None),
        ];
        ranking::rank_books(&mut results, SearchKind::Any, "the courage to be disliked");
        assert_eq!(results[0].title, "The Courage to Be Disliked");
        assert_eq!(results[1].title, "嫌われる勇気");
    }

    #[test]
    fn short_anywhere_query_uses_title_field() {
        let query = SearchKind::Any.query("It", 24);
        assert_eq!(query.title.as_deref(), Some("It"));
        assert!(query.free_text.is_none());
        let longer = SearchKind::Any.query("Stephen King", 24);
        assert_eq!(longer.free_text.as_deref(), Some("Stephen King"));
    }
}
