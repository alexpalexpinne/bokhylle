use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::AppState;
use crate::auth::AuthUser;
use crate::discovery::{self, SearchKind};
use crate::error::AppError;
use crate::library::relevance;

pub const SOURCES_KEY: &str = "home.spotlight_sources";

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpotlightItem {
    source: String,
    ownership: String,
    reason_type: String,
    reason_label: String,
    title: String,
    authors: Vec<String>,
    blurb: Option<String>,
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    languages: Option<Vec<String>>,
    subjects: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rating: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rating_count: Option<Option<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rating_source: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    book_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cover_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    year: Option<Option<i32>>,
    cta: &'static str,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct SpotlightResponse {
    items: Vec<SpotlightItem>,
    recommendations: Vec<SpotlightItem>,
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
const SPOTLIGHT_SIZE: usize = 8;

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

const LOCAL_LANGUAGE_FILTER: &str = "AND (
    NOT EXISTS (SELECT 1 FROM preferred)
    OR EXISTS (
        SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id
        JOIN preferred p ON p.language = lower(e.language)
        WHERE e.book_id = b.id
    )
    OR (
        NOT EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id)
        AND EXISTS (
            SELECT 1 FROM book_available_languages l JOIN preferred p ON p.language = lower(l.language)
            WHERE l.book_id = b.id
        )
    )
    OR (
        NOT EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id)
        AND NOT EXISTS (SELECT 1 FROM book_available_languages WHERE book_id = b.id)
        AND (b.language IS NULL OR EXISTS (SELECT 1 FROM preferred p WHERE p.language = lower(b.language)))
    )
    OR (
        EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id)
        AND NOT EXISTS (
            SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id
            WHERE e.book_id = b.id AND e.language IS NOT NULL
        )
    )
)";

struct Seed {
    id: i64,
    name: String,
    weight: i64,
    followed: bool,
}

struct Taste {
    subjects: Vec<Seed>,
    authors: Vec<Seed>,
}

fn fallback_reason(source: &str) -> &'static str {
    match source {
        "shelf" => "From your shelf",
        _ => "From your household library · matches your interests",
    }
}

fn reason_for(source: &str, taste: &Taste, matches_subject: i64, matches_author: i64) -> String {
    if matches_subject == 1
        && let Some(subject) = taste.subjects.first()
        && subject.weight >= 3
    {
        return format!("Because you like {}", subject.name);
    }
    if matches_author == 1
        && let Some(author) = taste.authors.first()
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
        subjects: subjects
            .split(", ")
            .filter(|part| !part.is_empty())
            .take(2)
            .map(str::to_string)
            .collect(),
        rating: Some(rating),
        rating_count: Some(rating_count),
        rating_source: Some(rating_source),
        book_id: Some(id),
        provider: None,
        provider_key: None,
        cover_id: None,
        cover_provider: None,
        year: None,
        cta: "explore",
    }
}

/// Deterministic taste profile from existing signals: likes, requests,
/// reader sends, manual shelf adds and followed authors. Household
/// ownership alone contributes nothing, and not-for-me books neither boost
/// nor appear as candidates.
async fn taste_profile(state: &AppState, user_id: i64) -> Result<Taste, AppError> {
    let subject_candidates: Vec<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT s.id, s.name, s.normalized_name, MAX(CASE
                    WHEN ub.preference = 'liked' THEN 5
                    WHEN ub.source IN ('requested', 'sent') THEN 3
                    WHEN ub.source = 'manual' THEN 1
                    ELSE 0 END) AS weight
         FROM user_books ub
         JOIN book_subjects bs ON bs.book_id = ub.book_id
         JOIN subjects s ON s.id = bs.subject_id
         WHERE ub.user_id = ? AND (ub.on_shelf = 1 OR ub.preference = 'liked')
           AND (ub.preference IS NULL OR ub.preference = 'liked')
           AND NOT EXISTS (
               SELECT 1 FROM user_subject_prefs usp
               WHERE usp.user_id = ub.user_id
                 AND usp.normalized_name = s.normalized_name
                 AND usp.hidden = 1
           )
         GROUP BY s.id
         HAVING weight > 0
         ORDER BY weight DESC, s.name
         LIMIT 5",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    // Dewey codes and other catalog noise must never become a reason.
    let derived_subjects: Vec<Seed> = subject_candidates
        .into_iter()
        .filter(|(_, _, normalized, _)| {
            crate::library::subjects::is_displayable(normalized)
                // Classification codes (005.1) have almost no letters.
                && normalized.chars().filter(|character| character.is_alphabetic()).count() >= 3
        })
        .take(3)
        .map(|(id, name, _, weight)| Seed {
            id,
            name,
            weight,
            followed: false,
        })
        .collect();

    // Wizard interests are useful even before the first book is acquired.
    let interests: Vec<(i64, String)> = sqlx::query_as(
        "SELECT COALESCE(s.id, 0), i.normalized_name
         FROM user_subject_interests i
         LEFT JOIN subjects s ON s.normalized_name = i.normalized_name
         WHERE i.user_id = ? ORDER BY i.created_at, i.rowid LIMIT 3",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    let interest_limit = if derived_subjects.is_empty() { 2 } else { 1 };
    let mut subjects = Vec::new();
    for (id, name) in interests {
        if subjects.len() >= interest_limit {
            break;
        }
        if subjects.iter().any(|seed: &Seed| seed.name == name) {
            continue;
        }
        subjects.push(Seed {
            id,
            name,
            weight: 3,
            followed: false,
        });
    }
    for seed in derived_subjects {
        if subjects.len() >= 2 {
            break;
        }
        if !subjects.iter().any(|existing: &Seed| {
            bokhylle_core::identity::normalize_text(&existing.name)
                == bokhylle_core::identity::normalize_text(&seed.name)
        }) {
            subjects.push(seed);
        }
    }

    let followed = crate::follows::followed_authors(&state.db, user_id).await?;
    let authors: Vec<Seed> = if !followed.is_empty() {
        followed
            .into_iter()
            .take(3)
            .map(|(id, name)| Seed {
                id,
                name,
                weight: 5,
                followed: true,
            })
            .collect()
    } else {
        let rows: Vec<(i64, String, i64)> = sqlx::query_as(
            "SELECT a.id, a.name, MAX(CASE
                        WHEN ub.preference = 'liked' THEN 5
                        WHEN ub.source IN ('requested', 'sent') THEN 3
                        WHEN ub.source = 'manual' THEN 1
                        ELSE 0 END) AS weight
             FROM user_books ub
             JOIN book_authors ba ON ba.book_id = ub.book_id
             JOIN authors a ON a.id = ba.author_id
             WHERE ub.user_id = ? AND (ub.on_shelf = 1 OR ub.preference = 'liked')
               AND (ub.preference IS NULL OR ub.preference = 'liked')
             GROUP BY a.id
             HAVING weight > 0
             ORDER BY weight DESC, a.name
             LIMIT 3",
        )
        .bind(user_id)
        .fetch_all(&state.db)
        .await?;
        rows.into_iter()
            .map(|(id, name, weight)| Seed {
                id,
                name,
                weight,
                followed: false,
            })
            .collect()
    };

    Ok(Taste { subjects, authors })
}

#[derive(Default)]
struct SeedMatches {
    heroes: Vec<SpotlightItem>,
    recommendations: Vec<SpotlightItem>,
}

#[derive(Clone, Copy)]
enum SeedAudience<'a> {
    Adult(i64),
    Child(&'a HashSet<(String, String)>),
}

async fn seed_search(
    state: &AppState,
    seed: &Seed,
    kind: SearchKind,
    preferred_languages: &[String],
    reason: String,
    audience: SeedAudience<'_>,
) -> SeedMatches {
    let results = match audience {
        SeedAudience::Child(_) => discovery::external_results(state, kind, &seed.name, 12).await,
        SeedAudience::Adult(user_id) => {
            discovery::search(state, kind, &seed.name, 12, user_id).await
        }
    }
    .unwrap_or_default();
    let mut candidates: Vec<_> = results
        .into_iter()
        .filter(|result| {
            result.owned_book_id.is_none()
                && !matches!(audience, SeedAudience::Child(keys) if keys.contains(&(result.provider.clone(), result.provider_key.clone())))
                && (preferred_languages.is_empty()
                    || (result.languages.is_empty() && result.language.is_none())
                    || result
                        .languages
                        .iter()
                        .chain(result.language.iter())
                        .any(|language| {
                            preferred_languages
                                .iter()
                                .any(|preferred| preferred.eq_ignore_ascii_case(language))
                        }))
        })
        .collect();
    // A slide without artwork looks broken; covered candidates first.
    candidates.sort_by_key(|result| result.cover_id.is_none());
    let recommendations = candidates
        .iter()
        .take(6)
        .map(|result| SpotlightItem {
            source: "discover".to_string(),
            ownership: "discover".to_string(),
            reason_type: if seed.followed { "follow" } else { "taste" }.to_string(),
            reason_label: reason.clone(),
            title: result.title.clone(),
            authors: result.authors.clone(),
            blurb: None,
            language: result.language.clone(),
            languages: Some(result.languages.clone()),
            subjects: Vec::new(),
            rating: None,
            rating_count: None,
            rating_source: None,
            book_id: None,
            provider: Some(result.provider.clone()),
            provider_key: Some(result.provider_key.clone()),
            cover_id: Some(result.cover_id.clone()),
            cover_provider: None,
            year: Some(result.year),
            cta: "discover",
        })
        .collect();
    let mut heroes = Vec::new();
    for result in candidates.into_iter().take(4) {
        if heroes.len() >= 2 {
            break;
        }
        let detail =
            match discovery::detail(state, Some(&result.provider), &result.provider_key).await {
                Ok(Some(detail)) => detail,
                _ => continue,
            };
        let Some(blurb) = detail
            .description
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
        else {
            continue;
        };
        heroes.push(SpotlightItem {
            source: "discover".to_string(),
            ownership: "discover".to_string(),
            reason_type: if seed.followed { "follow" } else { "taste" }.to_string(),
            reason_label: reason.clone(),
            title: detail.title,
            authors: detail.authors,
            blurb: Some(blurb),
            language: detail.language,
            languages: Some(result.languages.clone()),
            subjects: Vec::new(),
            rating: None,
            rating_count: None,
            rating_source: None,
            book_id: None,
            provider: Some(detail.provider.clone()),
            provider_key: Some(detail.provider_key),
            cover_id: Some(detail.cover_id),
            cover_provider: Some(detail.provider),
            year: Some(detail.year.or(result.year)),
            cta: "discover",
        });
    }
    SeedMatches {
        heroes,
        recommendations,
    }
}

/// Deliberately mixed recommendations: the user's shelf, the household
/// library, and discoverable not-owned books seeded by their strongest
/// taste signals. Sources are admin-controlled and may all be disabled.
pub async fn spotlight(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<SpotlightResponse>, AppError> {
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
        return Ok(Json(SpotlightResponse {
            items: Vec::new(),
            recommendations: Vec::new(),
        }));
    }

    let shelf_enabled = sources.iter().any(|source| source == "shelf");
    let household_enabled = sources.iter().any(|source| source == "household");
    let discover_enabled = sources.iter().any(|source| source == "discover");
    let (preferred_languages, _) = crate::updates::user_preferences(&state, user.id).await?;
    let preferred_json = serde_json::to_string(&preferred_languages)
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    let taste = taste_profile(&state, user.id).await?;
    let subject_id = taste.subjects.first().map(|seed| seed.id).unwrap_or(0);
    let author_id = taste.authors.first().map(|seed| seed.id).unwrap_or(0);

    let exclusions = relevance::EXCLUSIONS;
    let personal = relevance::personal();
    let shelf_rows: Vec<SpotlightRow> = if shelf_enabled {
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "WITH viewer(id) AS (SELECT ?), preferred(language) AS (SELECT lower(value) FROM json_each(?))
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
                    COALESCE((SELECT group_concat(s.name, ', ')
                              FROM book_subjects bs JOIN subjects s ON s.id = bs.subject_id
                              WHERE bs.book_id = b.id), ''),
                    ub.added_at,
                    b.rating,
                    b.rating_count,
                    b.rating_source,
                    EXISTS(SELECT 1 FROM book_subjects bs2
                           WHERE bs2.book_id = b.id AND bs2.subject_id = ?),
                    EXISTS(SELECT 1 FROM book_authors ba2
                           WHERE ba2.book_id = b.id AND ba2.author_id = ?),
                    {LOCAL_LANGUAGES_SQL}
             FROM user_books ub
             JOIN books b ON b.id = ub.book_id
             WHERE ub.user_id = ? AND ub.on_shelf = 1
               AND (ub.preference IS NULL OR ub.preference = 'liked')
               AND b.description IS NOT NULL AND length(trim(b.description)) > 120
               {exclusions}
               {LOCAL_LANGUAGE_FILTER}
             ORDER BY 11 DESC, 12 DESC, ub.added_at DESC
             LIMIT 5"
        )))
        .bind(user.id)
        .bind(&preferred_json)
        .bind(subject_id)
        .bind(author_id)
        .bind(user.id)
        .fetch_all(&state.db)
        .await?
    } else {
        Vec::new()
    };

    let household_rows: Vec<SpotlightRow> = if household_enabled {
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "WITH viewer(id) AS (SELECT ?), preferred(language) AS (SELECT lower(value) FROM json_each(?))
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
                    COALESCE((SELECT group_concat(s.name, ', ')
                              FROM book_subjects bs JOIN subjects s ON s.id = bs.subject_id
                              WHERE bs.book_id = b.id), ''),
                    b.created_at,
                    b.rating,
                    b.rating_count,
                    b.rating_source,
                    EXISTS(SELECT 1 FROM book_subjects bs2
                           WHERE bs2.book_id = b.id AND bs2.subject_id = ?),
                    EXISTS(SELECT 1 FROM book_authors ba2
                           WHERE ba2.book_id = b.id AND ba2.author_id = ?),
                    {LOCAL_LANGUAGES_SQL}
             FROM books b
             WHERE b.description IS NOT NULL AND length(trim(b.description)) > 120
               AND EXISTS (SELECT 1 FROM book_files f
                           JOIN editions e ON e.id = f.edition_id
                           WHERE e.book_id = b.id)
               AND NOT EXISTS (SELECT 1 FROM user_books ub
                               WHERE ub.user_id = ? AND ub.book_id = b.id AND ub.on_shelf = 1)
               {personal}
               {exclusions}
               {LOCAL_LANGUAGE_FILTER}
             ORDER BY 11 DESC, 12 DESC, b.created_at DESC
             LIMIT 5"
        )))
        .bind(user.id)
        .bind(&preferred_json)
        .bind(subject_id)
        .bind(author_id)
        .bind(user.id)
        .fetch_all(&state.db)
        .await?
    } else {
        Vec::new()
    };

    let mut items: Vec<SpotlightItem> = Vec::new();
    let mut shelf_queue = shelf_rows.into_iter();
    let mut household_queue = household_rows.into_iter();

    if let Some(row) = shelf_queue.next() {
        let reason = reason_for("shelf", &taste, row.10, row.11);
        items.push(slide(row, "shelf", "shelf", reason));
    }
    if let Some(row) = household_queue.next() {
        let reason = reason_for("household", &taste, row.10, row.11);
        items.push(slide(row, "household", "household", reason));
    }

    let mut recommendations = Vec::new();
    if discover_enabled {
        let excluded_keys: HashSet<(String, String)> = if child {
            sqlx::query_as(
                "SELECT DISTINCT ids.provider, ids.provider_key
                 FROM book_external_ids ids
                 WHERE EXISTS (
                     SELECT 1 FROM user_books ub
                     WHERE ub.book_id = ids.book_id AND ub.user_id = ? AND ub.on_shelf = 1
                 ) OR EXISTS (
                     SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                     WHERE e.book_id = ids.book_id
                 )",
            )
            .bind(user.id)
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .collect()
        } else {
            HashSet::new()
        };
        let mut seen_heroes = HashSet::new();
        let mut seen_recommendations = HashSet::new();
        let mut seeds = Vec::new();
        for index in 0..2 {
            if let Some(subject) = taste.subjects.get(index) {
                seeds.push((
                    subject,
                    SearchKind::Subject,
                    format!("Because you like {}", subject.name),
                ));
            }
            if let Some(author) = taste.authors.get(index) {
                seeds.push((
                    author,
                    SearchKind::Author,
                    format!("More from {}", author.name),
                ));
            }
        }
        for (seed, kind, reason) in seeds {
            let audience = if child {
                SeedAudience::Child(&excluded_keys)
            } else {
                SeedAudience::Adult(user.id)
            };
            let matches =
                seed_search(&state, seed, kind, &preferred_languages, reason, audience).await;
            for item in matches.heroes {
                let key = item.provider_key.as_deref().unwrap_or_default().to_string();
                if seen_heroes.insert(key) {
                    items.push(item);
                }
            }
            for item in matches.recommendations {
                let key = item.provider_key.as_deref().unwrap_or_default().to_string();
                if seen_recommendations.insert(key) {
                    recommendations.push(item);
                }
            }
        }
        recommendations
            .retain(|item| !seen_heroes.contains(item.provider_key.as_deref().unwrap_or_default()));
        recommendations.truncate(18);
    }

    // Backfill from whichever enabled sources still have candidates.
    while items.len() < SPOTLIGHT_SIZE {
        let mut filled = false;
        if let Some(row) = shelf_queue.next() {
            let reason = reason_for("shelf", &taste, row.10, row.11);
            items.push(slide(row, "shelf", "shelf", reason));
            filled = true;
        }
        if items.len() >= SPOTLIGHT_SIZE {
            break;
        }
        if let Some(row) = household_queue.next() {
            let reason = reason_for("household", &taste, row.10, row.11);
            items.push(slide(row, "household", "household", reason));
            filled = true;
        }
        if !filled {
            break;
        }
    }

    items.truncate(SPOTLIGHT_SIZE);
    Ok(Json(SpotlightResponse {
        items,
        recommendations,
    }))
}
