use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};

use crate::AppState;
use crate::auth::{AuthUser, User};
use crate::discovery::{self, SearchKind};
use crate::error::AppError;
use crate::library::relevance::{self, CatalogueExclusions, Seed, Taste};
use crate::services::recommendations as engine;

pub const SOURCES_KEY: &str = "home.spotlight_sources";

#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpotlightItem {
    pub(crate) source: String,
    pub(crate) ownership: String,
    pub(crate) reason_type: String,
    pub(crate) reason_label: String,
    pub(crate) title: String,
    pub(crate) authors: Vec<String>,
    pub(crate) blurb: Option<String>,
    pub(crate) language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) languages: Option<Vec<String>>,
    pub(crate) subjects: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) series: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rating: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rating_count: Option<Option<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rating_source: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) book_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cover_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cover_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) year: Option<Option<i32>>,
    pub(crate) cta: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) recommendation_key: Option<String>,
}

#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpotlightQuery {
    /// Return local books and saved suggestions without waiting for catalogues.
    #[serde(default)]
    cached_only: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct SpotlightResponse {
    pub(crate) items: Vec<SpotlightItem>,
    pub(crate) recommendations: Vec<SpotlightItem>,
}

pub async fn direct_activity(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<crate::updates::DirectActivityResponse>, AppError> {
    Ok(Json(
        crate::updates::direct_outcomes(&state, user.id).await?,
    ))
}

pub async fn updates(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<crate::updates::UpdatesResponse>, AppError> {
    Ok(Json(crate::updates::list(&state, user.id).await?))
}
const DEFAULT_SOURCES: [&str; 3] = ["shelf", "household", "discover"];
const SPOTLIGHT_SIZE: usize = 5;
const DISCOVERY_TTL_SECONDS: i64 = 15 * 60;
const DISCOVERY_MAX_AGE_SECONDS: i64 = 24 * 60 * 60;
// Bound catalogue work across Home requests, including overlapping profiles.
static DISCOVERY_LOOKUPS: Semaphore = Semaphore::const_new(2);
type RefreshLocks = HashMap<String, Weak<AsyncMutex<()>>>;
static REFRESH_LOCKS: LazyLock<Mutex<RefreshLocks>> = LazyLock::new(|| Mutex::new(HashMap::new()));

type SpotlightRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    Option<f64>,
    Option<i64>,
    Option<String>,
    i64,
    i64,
    String,
    i64,
    Option<String>,
);

// Saved catalogue works have available languages; owned files have actual
// edition languages. Never use a work's arbitrary primary language to
// describe a downloaded file.
const LOCAL_LANGUAGES_SQL: &str = "CASE WHEN EXISTS (
    SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id
) THEN COALESCE((
    SELECT group_concat(DISTINCT e.language)
    FROM editions e JOIN book_files f ON f.edition_id = e.id WHERE e.book_id = b.id
), '') ELSE COALESCE((
    SELECT group_concat(language) FROM book_available_languages WHERE book_id = b.id
), '') END";

fn fallback_reason(source: &str) -> &'static str {
    match source {
        "shelf" => "From your shelf",
        _ => "From your household library · matches your interests",
    }
}

fn reason_for(source: &str, taste: &Taste, matches_subject: i64, matches_author: i64) -> String {
    if let Some(subject) = taste
        .subjects
        .iter()
        .find(|s| s.id != 0 && s.id == matches_subject)
        && subject.weight >= 3
    {
        return format!("Because you like {}", subject.name);
    }
    if let Some(author) = taste.authors.iter().find(|a| a.id == matches_author)
        && (author.followed || author.weight >= 3)
    {
        return format!("More from {}", author.name);
    }
    fallback_reason(source).to_string()
}

fn slide(row: SpotlightRow, source: &str, ownership: &str, reason: String) -> SpotlightItem {
    let (
        id,
        title,
        authors,
        language,
        description,
        subjects,
        _added_at,
        rating,
        rating_count,
        rating_source,
        _matches_subject,
        _matches_author,
        available_languages,
        _affinity,
        series,
    ) = row;
    SpotlightItem {
        source: source.to_string(),
        ownership: ownership.to_string(),
        reason_type: source.to_string(),
        reason_label: reason,
        title,
        authors: authors
            .split(", ")
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect(),
        blurb: description,
        language,
        languages: Some(
            available_languages
                .split(',')
                .filter(|language| !language.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        subjects: serde_json::from_str(&subjects).unwrap_or_default(),
        series,
        rating: Some(rating),
        rating_count: Some(rating_count),
        rating_source: Some(rating_source),
        book_id: Some(id),
        provider: None,
        provider_key: None,
        cover_id: None,
        cover_provider: None,
        year: None,
        cta: "explore".to_string(),
        recommendation_key: None,
    }
}

/// Catalogue candidates from one subject or author seed.
#[derive(Default, Deserialize, Serialize)]
struct SeedMatches {
    #[serde(default)]
    retrieved: usize,
    #[serde(default)]
    filtered: std::collections::BTreeMap<String, usize>,
    #[serde(default)]
    cached: bool,
    heroes: Vec<SpotlightItem>,
    recommendations: Vec<SpotlightItem>,
}

async fn seed_search(
    state: &AppState,
    seed: &Seed,
    kind: SearchKind,
    preferred_languages: &[String],
    reason: String,
    exclusions: &CatalogueExclusions,
    taste: &Taste,
) -> SeedMatches {
    let _permit = DISCOVERY_LOOKUPS
        .acquire()
        .await
        .expect("Spotlight semaphore stays open");
    // Fetch the provider pool before applying display eligibility. A user's
    // downloaded books must not consume a twelve-result recommendation budget.
    let results = discovery::external_results(state, kind, &seed.name, 50)
        .await
        .unwrap_or_default();
    let retrieved = results.len();
    let mut filtered = std::collections::BTreeMap::new();
    let mut candidates: Vec<_> = results
        .into_iter()
        .filter(|r| {
            let reason = if !relevance::language_matches(
                preferred_languages,
                &r.languages,
                r.language.as_deref(),
            ) {
                Some("language")
            } else if kind == SearchKind::Author
                && !r
                    .authors
                    .iter()
                    .any(|a| bokhylle_core::identity::normalize_text(a) == seed.concept)
            {
                Some("author_mismatch")
            } else {
                exclusions.rejection_reason(
                    &r.provider,
                    &r.provider_key,
                    &r.title,
                    &r.authors,
                    &r.subjects,
                )
            };
            if let Some(reason) = reason {
                *filtered.entry(reason.to_string()).or_insert(0) += 1;
            }
            reason.is_none()
        })
        .collect();
    candidates.sort_by_key(|r| r.cover_id.is_none());
    let mut recommendations: Vec<_> = candidates
        .into_iter()
        .map(|r| SpotlightItem {
            source: "discover".into(),
            ownership: "discover".into(),
            reason_type: if seed.followed { "follow" } else { "taste" }.into(),
            reason_label: reason.clone(),
            title: r.title.clone(),
            authors: r.authors.clone(),
            blurb: None,
            language: r.language.clone(),
            languages: Some(r.languages.clone()),
            subjects: r.subjects.clone(),
            series: r.series.clone(),
            rating: None,
            rating_count: None,
            rating_source: None,
            book_id: None,
            provider: Some(r.provider.clone()),
            provider_key: Some(r.provider_key.clone()),
            cover_id: Some(r.cover_id.clone()),
            cover_provider: None,
            year: Some(r.year),
            cta: "discover".into(),
            recommendation_key: None,
        })
        .collect();
    recommendations.sort_by_key(|item| {
        std::cmp::Reverse(crate::services::recommendations::affinity(item, taste).affinity)
    });
    let author_matches = |item: &SpotlightItem| {
        kind != SearchKind::Author
            || item
                .authors
                .iter()
                .any(|name| bokhylle_core::identity::normalize_text(name) == seed.concept)
    };
    let mut heroes = Vec::new();
    for item in recommendations.iter_mut().take(8) {
        if heroes.len() >= 2 {
            break;
        }
        let detail = match discovery::resolve_metadata(
            state,
            item.provider.as_deref(),
            item.provider_key
                .as_deref()
                .expect("catalogue candidate key"),
        )
        .await
        {
            Ok(Some(detail)) => detail,
            _ => continue,
        };
        for subject in detail.subjects {
            if !item.subjects.contains(&subject) {
                item.subjects.push(subject);
            }
        }
        let languages = item.languages.get_or_insert_with(Vec::new);
        for language in detail.languages {
            if !languages.contains(&language) {
                languages.push(language);
            }
        }
        item.language = detail.language.or(item.language.take());
        item.title = detail.title;
        item.authors = detail.authors;
        item.series = detail.series.or(item.series.take());
        item.cover_id = Some(detail.cover_id);
        item.cover_provider = Some(detail.provider);
        item.year = Some(detail.year.or(item.year.flatten()));
        if !catalogue_permits(item, preferred_languages, exclusions) || !author_matches(item) {
            continue;
        }
        let Some(blurb) = detail.description.filter(|b| !b.trim().is_empty()) else {
            continue;
        };
        let mut hero = item.clone();
        hero.blurb = Some(blurb);
        heroes.push(hero);
    }
    recommendations.retain(|item| {
        let allowed =
            catalogue_permits(item, preferred_languages, exclusions) && author_matches(item);
        if !allowed {
            *filtered.entry("detail_metadata".to_string()).or_insert(0) += 1;
        }
        allowed
    });
    SeedMatches {
        retrieved,
        filtered,
        cached: false,
        heroes,
        recommendations,
    }
}

fn catalogue_rejection(
    item: &SpotlightItem,
    preferred_languages: &[String],
    exclusions: &CatalogueExclusions,
) -> Option<&'static str> {
    if !relevance::language_matches(
        preferred_languages,
        item.languages.as_deref().unwrap_or_default(),
        item.language.as_deref(),
    ) {
        return Some("language");
    }
    let Some((provider, key)) = item.provider.as_ref().zip(item.provider_key.as_ref()) else {
        return Some("missing_identity");
    };
    exclusions.rejection_reason(provider, key, &item.title, &item.authors, &item.subjects)
}

fn catalogue_permits(
    item: &SpotlightItem,
    preferred_languages: &[String],
    exclusions: &CatalogueExclusions,
) -> bool {
    catalogue_rejection(item, preferred_languages, exclusions).is_none()
}

#[derive(Deserialize, Serialize)]
struct SavedDiscovery {
    fingerprint: String,
    matches: SeedMatches,
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn refresh_lock(state: &AppState, cache_key: &str) -> Arc<AsyncMutex<()>> {
    let key = format!("{}:{cache_key}", state.paths.config_dir.display());
    let mut locks = REFRESH_LOCKS.lock().expect("Spotlight refresh locks");
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(AsyncMutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

async fn saved_discovery(
    state: &AppState,
    cache_key: &str,
    fingerprint: &str,
) -> Result<Option<(SeedMatches, bool)>, AppError> {
    let cached: Option<(String, i64, i64)> =
        sqlx::query_as("SELECT value, fetched_at, expires_at FROM metadata_cache WHERE key = ?")
            .bind(cache_key)
            .fetch_optional(&state.db)
            .await?;
    let Some((value, fetched_at, expires_at)) = cached else {
        return Ok(None);
    };
    let now = now_epoch();
    let Ok(saved) = serde_json::from_str::<SavedDiscovery>(&value) else {
        return Ok(None);
    };
    if saved.fingerprint != fingerprint || now - fetched_at > DISCOVERY_MAX_AGE_SECONDS {
        return Ok(None);
    }
    let mut matches = saved.matches;
    matches.cached = true;
    Ok(Some((matches, expires_at > now)))
}

async fn discover_matches(
    state: &AppState,
    user_id: i64,
    child: bool,
    taste: &Taste,
    preferred_languages: &[String],
    exclusions: &CatalogueExclusions,
    cached_only: bool,
) -> Result<SeedMatches, AppError> {
    let mut seeds = Vec::new();
    let day = now_epoch() / 86_400;
    // Keep the strongest seed, then rotate through the remaining interests and
    // authors. Every follow and explicit interest can receive a turn.
    let select = |pool: &[Seed]| {
        let mut chosen = pool.iter().take(1).cloned().collect::<Vec<_>>();
        if pool.len() > 1 {
            let start = (day as usize + user_id as usize) % (pool.len() - 1);
            for offset in 0..2.min(pool.len() - 1) {
                chosen.push(pool[1 + (start + offset) % (pool.len() - 1)].clone());
            }
        }
        chosen
    };
    let subjects = select(&taste.subjects);
    let authors = select(&taste.authors);
    for index in 0..3 {
        if let Some(subject) = subjects.get(index) {
            seeds.push((
                subject,
                SearchKind::Subject,
                format!("Because you like {}", subject.name),
            ));
        }
        if let Some(author) = authors.get(index) {
            seeds.push((
                author,
                SearchKind::Author,
                format!("More from {}", author.name),
            ));
        }
    }
    if seeds.is_empty() {
        return Ok(SeedMatches::default());
    }
    let seed_identity: Vec<_> = seeds
        .iter()
        .map(|(seed, kind, _)| {
            (
                &seed.concept,
                seed.weight,
                seed.followed,
                seed.explicit,
                kind.as_str(),
            )
        })
        .collect();
    let taste_identity = |pool: &[Seed]| {
        pool.iter()
            .map(|seed| {
                (
                    seed.concept.clone(),
                    seed.weight,
                    seed.followed,
                    seed.explicit,
                )
            })
            .collect::<Vec<_>>()
    };
    let identity = serde_json::to_vec(&(
        child,
        day,
        preferred_languages,
        seed_identity,
        taste_identity(&taste.subjects),
        taste_identity(&taste.authors),
        state.metadata.name(),
        state
            .metadata_fallback
            .as_ref()
            .map(|provider| provider.name()),
    ))
    .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    let fingerprint = hex::encode(Sha256::digest(identity));
    // One disposable snapshot per profile. Local shelf/household data is never
    // saved here; it is queried with current permissions on every request.
    let cache_key = format!("spotlight-discovery:v3:{user_id}");
    let cached = saved_discovery(state, &cache_key, &fingerprint).await?;
    if cached_only || cached.as_ref().is_some_and(|(_, fresh)| *fresh) {
        return Ok(cached.map(|(matches, _)| matches).unwrap_or_default());
    }

    // Coalesce repeated refreshes for the same profile. A cache-only request
    // never acquires this lock and can always return immediately.
    let lock = refresh_lock(state, &cache_key);
    let _refresh = lock.lock().await;
    if let Some((matches, true)) = saved_discovery(state, &cache_key, &fingerprint).await? {
        return Ok(matches);
    }
    let mut batches = Vec::new();
    // Preserve seed order even when the second catalogue lookup finishes first.
    // Two independent searches run together; the global permit also bounds
    // simultaneous catalogue work across profiles.
    for pair in seeds.chunks(2) {
        let (seed, kind, reason) = &pair[0];
        let first = seed_search(
            state,
            seed,
            *kind,
            preferred_languages,
            reason.clone(),
            exclusions,
            taste,
        );
        let results = if let Some((seed, kind, reason)) = pair.get(1) {
            let second = seed_search(
                state,
                seed,
                *kind,
                preferred_languages,
                reason.clone(),
                exclusions,
                taste,
            );
            let (first, second) = tokio::join!(first, second);
            vec![first, second]
        } else {
            vec![first.await]
        };
        batches.extend(results);
    }
    let mut hero_queues = Vec::new();
    let mut recommendation_queues = Vec::new();
    let mut retrieved = 0;
    let mut filtered = std::collections::BTreeMap::new();
    for batch in batches {
        retrieved += batch.retrieved;
        for (reason, count) in batch.filtered {
            *filtered.entry(reason).or_insert(0) += count;
        }
        hero_queues.push(batch.heroes);
        recommendation_queues.push(batch.recommendations);
    }
    let matches = SeedMatches {
        retrieved,
        filtered,
        cached: false,
        heroes: interleave(hero_queues),
        recommendations: interleave(recommendation_queues),
    };
    // A failed/empty catalogue pass must not wipe a usable saved selection.
    if matches.heroes.is_empty()
        && matches.recommendations.is_empty()
        && let Some((saved, _)) = cached
    {
        return Ok(saved);
    }
    let saved = SavedDiscovery {
        fingerprint,
        matches,
    };
    let value = serde_json::to_string(&saved)
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    let now = now_epoch();
    let ttl = if saved.matches.heroes.is_empty() && saved.matches.recommendations.is_empty() {
        60
    } else {
        DISCOVERY_TTL_SECONDS
    };
    sqlx::query(
        "INSERT INTO metadata_cache (key, value, fetched_at, expires_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value,
             fetched_at = excluded.fetched_at, expires_at = excluded.expires_at",
    )
    .bind(cache_key)
    .bind(value)
    .bind(now)
    .bind(now + ttl)
    .execute(&state.db)
    .await?;
    Ok(saved.matches)
}

/// Deliberately mixed recommendations: the user's shelf, the household
/// library, and discoverable not-owned books seeded by their strongest
/// taste signals. Sources are admin-controlled and may all be disabled.
pub async fn spotlight(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<SpotlightQuery>,
) -> Result<Json<SpotlightResponse>, AppError> {
    selection(user, state, query.cached_only, 18, false)
        .await
        .map(Json)
}

pub(crate) async fn selection(
    user: User,
    state: AppState,
    cached_only: bool,
    limit: usize,
    include_featured: bool,
) -> Result<SpotlightResponse, AppError> {
    let started = std::time::Instant::now();
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let mut sources: Vec<String> = state
        .settings
        .raw(SOURCES_KEY)
        .await?
        .and_then(|value| value.as_array().cloned())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| {
            DEFAULT_SOURCES
                .iter()
                .map(|value| value.to_string())
                .collect()
        });
    if child {
        let can_discover = crate::services::requests::may_discover(&state, user.id).await?;
        sources.retain(|source| source == "shelf" || (can_discover && source == "discover"));
    }
    if sources.is_empty() {
        return Ok(SpotlightResponse {
            items: Vec::new(),
            recommendations: Vec::new(),
        });
    }

    let shelf_enabled = sources.iter().any(|source| source == "shelf");
    let household_enabled = sources.iter().any(|source| source == "household");
    let discover_enabled = sources.iter().any(|source| source == "discover");
    let (preferred_languages, _) = crate::updates::user_preferences(&state, user.id).await?;
    let preferred_json = serde_json::to_string(&preferred_languages)
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    let taste = relevance::taste(&state.db, user.id).await?;
    let taste_ms = started.elapsed().as_millis() as u64;
    let local_started = std::time::Instant::now();
    let subject_json = serde_json::to_string(&taste.subjects)
        .map_err(|e| AppError::Unprocessable(e.to_string()))?;
    let author_json = serde_json::to_string(&taste.authors)
        .map_err(|e| AppError::Unprocessable(e.to_string()))?;
    let taste_ctes = relevance::TASTE_CTES;
    let affinity = relevance::AFFINITY;
    let language_filter = relevance::LANGUAGE_FILTER;

    let visibility = crate::services::sharing::predicate("b.id", user.id);
    let exclusions = relevance::EXCLUSIONS;
    let personal = relevance::personal();
    let shelf_rows: Vec<SpotlightRow> = if shelf_enabled {
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "WITH viewer(id) AS (SELECT ?), preferred(language) AS (SELECT lower(value) FROM json_each(?)) {taste_ctes}
             SELECT b.id, b.title,
                    COALESCE((SELECT group_concat(a.name, ', ')
                              FROM book_authors ba JOIN authors a ON a.id = ba.author_id
                              WHERE ba.book_id = b.id), ''),
                    CASE WHEN EXISTS (
                        SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id
                    ) THEN (
                        SELECT e.language FROM book_files f JOIN editions e ON e.id = f.edition_id
                        WHERE e.book_id = b.id AND e.language IS NOT NULL ORDER BY f.id LIMIT 1
                    ) ELSE CASE WHEN EXISTS (
                        SELECT 1 FROM book_available_languages WHERE book_id = b.id
                    ) THEN NULL ELSE b.language END END,
                    b.description,
                    COALESCE((SELECT json_group_array(s.name)
                              FROM book_subjects bs JOIN subjects s ON s.id = bs.subject_id
                              WHERE bs.book_id = b.id), '[]'),
                    ub.added_at,
                    b.rating,
                    b.rating_count,
                    b.rating_source,
                    COALESCE((SELECT t.id FROM book_subjects bs2 JOIN taste_subjects t ON t.id = bs2.subject_id
                              WHERE bs2.book_id = b.id ORDER BY t.weight DESC, t.id LIMIT 1), 0),
                    COALESCE((SELECT t.id FROM book_authors ba2 JOIN taste_authors t ON t.id = ba2.author_id
                              WHERE ba2.book_id = b.id ORDER BY t.weight DESC, t.id LIMIT 1), 0),
                    {LOCAL_LANGUAGES_SQL}, {affinity}, COALESCE(CAST(b.series_id AS TEXT), b.series)
             FROM user_books ub
             JOIN books b ON b.id = ub.book_id
             WHERE ub.user_id = ? AND ub.on_shelf = 1
               AND {visibility}
               AND (ub.preference IS NULL OR ub.preference = 'liked')
               AND b.description IS NOT NULL AND length(trim(b.description)) > 120
               {exclusions}
               {language_filter}
             ORDER BY 14 DESC, ub.added_at DESC, b.id DESC
             LIMIT 40"
        )))
        .bind(user.id)
        .bind(&preferred_json)
        .bind(&subject_json)
        .bind(&author_json)
        .bind(user.id)
        .fetch_all(&state.db)
        .await?
    } else {
        Vec::new()
    };

    let household_rows: Vec<SpotlightRow> = if household_enabled {
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "WITH viewer(id) AS (SELECT ?), preferred(language) AS (SELECT lower(value) FROM json_each(?)) {taste_ctes}
             SELECT b.id, b.title,
                    COALESCE((SELECT group_concat(a.name, ', ')
                              FROM book_authors ba JOIN authors a ON a.id = ba.author_id
                              WHERE ba.book_id = b.id), ''),
                    CASE WHEN EXISTS (
                        SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id
                    ) THEN (
                        SELECT e.language FROM book_files f JOIN editions e ON e.id = f.edition_id
                        WHERE e.book_id = b.id AND e.language IS NOT NULL ORDER BY f.id LIMIT 1
                    ) ELSE CASE WHEN EXISTS (
                        SELECT 1 FROM book_available_languages WHERE book_id = b.id
                    ) THEN NULL ELSE b.language END END,
                    b.description,
                    COALESCE((SELECT json_group_array(s.name)
                              FROM book_subjects bs JOIN subjects s ON s.id = bs.subject_id
                              WHERE bs.book_id = b.id), '[]'),
                    b.created_at,
                    b.rating,
                    b.rating_count,
                    b.rating_source,
                    COALESCE((SELECT t.id FROM book_subjects bs2 JOIN taste_subjects t ON t.id = bs2.subject_id
                              WHERE bs2.book_id = b.id ORDER BY t.weight DESC, t.id LIMIT 1), 0),
                    COALESCE((SELECT t.id FROM book_authors ba2 JOIN taste_authors t ON t.id = ba2.author_id
                              WHERE ba2.book_id = b.id ORDER BY t.weight DESC, t.id LIMIT 1), 0),
                    {LOCAL_LANGUAGES_SQL}, {affinity}, COALESCE(CAST(b.series_id AS TEXT), b.series)
             FROM books b
             WHERE b.description IS NOT NULL AND length(trim(b.description)) > 120
               AND EXISTS (SELECT 1 FROM book_files f
                           JOIN editions e ON e.id = f.edition_id
                           WHERE e.book_id = b.id)
               AND NOT EXISTS (SELECT 1 FROM user_books ub
                               WHERE ub.user_id = ? AND ub.book_id = b.id AND ub.on_shelf = 1)
               AND {visibility}
               {personal}
               {exclusions}
               {language_filter}
             ORDER BY 14 DESC, b.created_at DESC, b.id DESC
             LIMIT 40"
        )))
        .bind(user.id)
        .bind(&preferred_json)
        .bind(&subject_json)
        .bind(&author_json)
        .bind(user.id)
        .fetch_all(&state.db)
        .await?
    } else {
        Vec::new()
    };

    // Demo suggestions use eligible, unshelved sample copies and the normal
    // personal filters. No external catalogue is consulted in demo mode.
    let mut recommendations = if discover_enabled && state.demo.is_some() {
        household_rows
            .iter()
            .cloned()
            .map(|row| {
                let reason = reason_for("household", &taste, row.10, row.11);
                slide(row, "household", "household", reason)
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut shelf_items: Vec<_> = shelf_rows
        .into_iter()
        .map(|row| {
            let reason = reason_for("shelf", &taste, row.10, row.11);
            slide(row, "shelf", "shelf", reason)
        })
        .collect();
    let mut household_items: Vec<_> = household_rows
        .into_iter()
        .map(|row| {
            let reason = reason_for("household", &taste, row.10, row.11);
            slide(row, "household", "household", reason)
        })
        .collect();
    let local_ms = local_started.elapsed().as_millis() as u64;
    let catalogue_started = std::time::Instant::now();
    let mut catalogue_retrieved = 0;
    let mut catalogue_cached = false;
    let mut filtered = std::collections::BTreeMap::new();
    let mut heroes = Vec::new();
    if discover_enabled && state.demo.is_none() {
        let exclusions = relevance::catalogue_exclusions(&state.db, user.id, child, true).await?;
        let matches = discover_matches(
            &state,
            user.id,
            child,
            &taste,
            &preferred_languages,
            &exclusions,
            cached_only,
        )
        .await?;
        catalogue_retrieved = matches.retrieved;
        catalogue_cached = matches.cached;
        filtered = matches.filtered;
        for item in &matches.recommendations {
            if let Some(reason) = catalogue_rejection(item, &preferred_languages, &exclusions) {
                *filtered.entry(format!("live_{reason}")).or_insert(0) += 1;
            }
        }
        let eligible =
            |item: &SpotlightItem| catalogue_permits(item, &preferred_languages, &exclusions);
        heroes = matches.heroes.into_iter().filter(eligible).collect();
        recommendations.extend(matches.recommendations.into_iter().filter(eligible));
    }
    let catalogue_ms = catalogue_started.elapsed().as_millis() as u64;
    let retrieved = catalogue_retrieved + shelf_items.len() + household_items.len();
    let before_rank =
        recommendations.len() + heroes.len() + shelf_items.len() + household_items.len();
    let ranking_started = std::time::Instant::now();
    let mut scores = engine::rank(&state, user.id, &taste, &mut shelf_items).await?;
    scores.extend(engine::rank(&state, user.id, &taste, &mut household_items).await?);
    scores.extend(engine::rank(&state, user.id, &taste, &mut heroes).await?);
    scores.extend(engine::rank(&state, user.id, &taste, &mut recommendations).await?);
    let after_rank =
        recommendations.len() + heroes.len() + shelf_items.len() + household_items.len();
    filtered.insert("dismissed".to_string(), before_rank - after_rank);
    let mut score_keys = HashSet::new();
    scores.retain(|score| score_keys.insert(score.key.clone()));
    let eligible = scores.len();
    let ranking_ms = ranking_started.elapsed().as_millis() as u64;
    let items = diverse(
        interleave(vec![shelf_items, household_items, heroes]),
        SPOTLIGHT_SIZE,
    );
    // Only the final five Spotlight books leave the catalogue rail. Local
    // books stay on their local shelves, including the demo's sample rail.
    let featured: HashSet<_> = items
        .iter()
        .filter_map(|i| i.provider.as_ref().zip(i.provider_key.as_ref()))
        .collect();
    recommendations.retain(|i| {
        include_featured
            || !i
                .provider
                .as_ref()
                .zip(i.provider_key.as_ref())
                .is_some_and(|key| featured.contains(&key))
    });
    let recommendations = diverse(recommendations, limit);
    let offered: Vec<_> = items.iter().chain(&recommendations).cloned().collect();
    engine::offer(&state, user.id, &offered).await?;
    let diagnostics = engine::RecommendationDiagnostics {
        retrieved,
        eligible,
        selected: offered
            .iter()
            .map(engine::identity)
            .collect::<HashSet<_>>()
            .len(),
        filtered,
        catalogue_cached,
        timings_ms: std::collections::BTreeMap::from([
            ("taste".into(), taste_ms),
            ("local_queries".into(), local_ms),
            ("catalogue".into(), catalogue_ms),
            ("ranking".into(), ranking_ms),
        ]),
        elapsed_ms: started.elapsed().as_millis() as u64,
        cached_only,
        scores,
    };
    engine::save_diagnostics(&state, user.id, &diagnostics).await?;
    Ok(SpotlightResponse {
        items,
        recommendations,
    })
}

fn interleave(queues: Vec<Vec<SpotlightItem>>) -> Vec<SpotlightItem> {
    let mut queues: Vec<_> = queues
        .into_iter()
        .map(std::collections::VecDeque::from)
        .collect();
    let mut items = Vec::new();
    loop {
        let mut added = false;
        for queue in &mut queues {
            if let Some(item) = queue.pop_front() {
                items.push(item);
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    items
}

fn diverse(candidates: Vec<SpotlightItem>, limit: usize) -> Vec<SpotlightItem> {
    let mut selected: Vec<SpotlightItem> = Vec::new();
    let mut deferred = Vec::new();
    let mut seen = HashSet::new();
    for item in candidates {
        let key = (
            bokhylle_core::identity::normalize_text(&item.title),
            item.authors
                .first()
                .map(|a| bokhylle_core::identity::normalize_text(a)),
        );
        if !seen.insert(key) {
            continue;
        }
        let repeated_author = item.authors.iter().any(|a| {
            selected
                .iter()
                .filter(|s| {
                    s.authors.iter().any(|other| {
                        bokhylle_core::identity::normalize_text(a)
                            == bokhylle_core::identity::normalize_text(other)
                    })
                })
                .count()
                >= 2
        });
        let repeated_series = item.series.as_ref().is_some_and(|series| {
            selected.iter().any(|s| {
                s.series.as_ref().is_some_and(|other| {
                    bokhylle_core::identity::normalize_text(series)
                        == bokhylle_core::identity::normalize_text(other)
                })
            })
        });
        if selected.len() < limit && !repeated_author && !repeated_series {
            selected.push(item);
        } else {
            deferred.push(item);
        }
    }
    selected.extend(
        deferred
            .into_iter()
            .take(limit.saturating_sub(selected.len())),
    );
    selected
}
