//! Updates for followed authors: one shared provider refresh per author per
//! day, plus the in-app feed of library, discovery and ready facts.

use serde::Serialize;

use crate::AppState;
use crate::discovery::{self, SearchKind};
use crate::error::AppError;

const REFRESH_INTERVAL_SECONDS: i64 = 24 * 3600;

type DiscoveryRow = (
    i64,
    String,
    String,
    String,
    Option<i64>,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
);

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookUpdate {
    book_id: i64,
    title: String,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryUpdate {
    author_id: i64,
    authors: Vec<String>,
    title: String,
    provider_key: String,
    year: Option<i64>,
    cover_id: Option<String>,
    provider: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct UpdatesResponse {
    library: Vec<BookUpdate>,
    discoveries: Vec<DiscoveryUpdate>,
    ready: Vec<BookUpdate>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DirectActivityItem {
    author_id: i64,
    author_name: String,
    title: String,
    authors: Vec<String>,
    outcome: String,
    detail: &'static str,
    delivery_id: Option<i64>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DirectActivityResponse {
    items: Vec<DirectActivityItem>,
}

/// Arms automation for a follow only after a successful refresh, recording
/// the currently known provider identities as the baseline. A failed refresh
/// leaves automation disarmed.
pub async fn arm_automation(
    state: &AppState,
    user_id: i64,
    author_id: i64,
    delivery_target_id: Option<i64>,
) -> Result<bool, AppError> {
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM authors WHERE id = ?")
        .bind(author_id)
        .fetch_optional(&state.db)
        .await?;
    let Some(name) = name else {
        return Err(AppError::NotFound("author not found".to_string()));
    };

    let results = match discovery::search(state, SearchKind::Author, &name, 40, 0).await {
        Ok(results) => results,
        Err(_) => return Ok(false),
    };
    for result in results.iter().filter(|result| {
        result
            .authors
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&name))
    }) {
        let _ = sqlx::query(
            "INSERT INTO author_discoveries
                 (author_id, provider, provider_key, title, authors, year, cover_id, language, languages, subjects)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(author_id, provider, provider_key) DO UPDATE SET
                 cover_id = COALESCE(author_discoveries.cover_id, excluded.cover_id),
                 language = excluded.language, languages = excluded.languages, subjects = excluded.subjects",
        )
        .bind(author_id)
        .bind(&result.provider)
        .bind(&result.provider_key)
        .bind(&result.title)
        .bind(result.authors.join(", "))
        .bind(result.year)
        .bind(&result.cover_id)
        .bind(&result.language)
        .bind(serde_json::to_string(&result.languages).unwrap_or_else(|_| "[]".into()))
        .bind(serde_json::to_string(&result.subjects).unwrap_or_else(|_| "[]".into()))
        .execute(&state.db)
        .await;
    }
    let _ = sqlx::query(
        "INSERT INTO author_follow_refreshes (author_id, checked_at) VALUES (?, unixepoch())
         ON CONFLICT(author_id) DO UPDATE SET checked_at = excluded.checked_at",
    )
    .bind(author_id)
    .execute(&state.db)
    .await;

    sqlx::query(
        "UPDATE author_follows
         SET auto_acquire = 1,
             delivery_target_id = ?,
             baseline_at = unixepoch()
         WHERE user_id = ? AND author_id = ?",
    )
    .bind(delivery_target_id)
    .bind(user_id)
    .bind(author_id)
    .execute(&state.db)
    .await?;
    Ok(true)
}

/// A user's accepted languages (ordered) and preferred format. A lookup
/// failure is surfaced so automation skips the follower instead of widening
/// the language policy to "any".
pub async fn user_preferences(
    state: &AppState,
    user_id: i64,
) -> Result<(Vec<String>, Option<String>), AppError> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT preferred_language, preferred_format, preferred_languages
         FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    let (preferred_language, preferred_format, preferred_languages) = row.unwrap_or_default();
    let mut languages: Vec<String> = preferred_languages
        .as_deref()
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default();
    if languages.is_empty()
        && let Some(language) = preferred_language
    {
        languages.push(language);
    }
    Ok((languages, preferred_format))
}

/// Unknown languages never qualify; an empty preference list accepts any
/// known language.
fn language_accepted(language: Option<&str>, languages: &[String]) -> bool {
    match language {
        Some(language) => {
            languages.is_empty()
                || languages
                    .iter()
                    .any(|preferred| preferred.eq_ignore_ascii_case(language))
        }
        None => false,
    }
}

/// Box sets and multi-book bundles are not useful author discoveries: they
/// match many authors at once and render as one enormous entry.
fn plausible_discovery(result: &crate::discovery::DiscoveryResult) -> bool {
    if result.authors.len() > 3 {
        return false;
    }
    let title = result.title.to_lowercase();
    ![
        "collection set",
        "box set",
        "boxed set",
        "bundle",
        "complete series",
        "books collection",
    ]
    .iter()
    .any(|marker| title.contains(marker))
}

/// A plausible publication recency check: unknown dates never acquire, and
/// anything clearly old is treated as catalogue noise, not a new book.
fn recently_published(year: Option<i64>) -> bool {
    let Some(year) = year else {
        return false;
    };
    let current = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| (value.as_secs() / 31_536_000) as i64 + 1970)
        .unwrap_or(2026);
    year >= current - 1
}

/// Creates a normal acquisition request per auto-acquire follower, with
/// `deliver_on_ready` when they chose a reader; the existing pipeline does
/// the rest.
async fn claim_attempt(
    state: &AppState,
    user_id: i64,
    author_id: i64,
    provider: &str,
    provider_key: &str,
) -> bool {
    sqlx::query(
        "INSERT INTO author_automation_attempts
             (user_id, author_id, provider, provider_key, result)
         VALUES (?, ?, ?, ?, 'attempted')
         ON CONFLICT(user_id, author_id, provider, provider_key) DO NOTHING",
    )
    .bind(user_id)
    .bind(author_id)
    .bind(provider)
    .bind(provider_key)
    .execute(&state.db)
    .await
    .map(|outcome| outcome.rows_affected() > 0)
    .unwrap_or(false)
}

/// Records the direct (no-acquisition) outcome with its linkage so Activity
/// can show it without a fake acquisition.
#[allow(clippy::too_many_arguments)]
async fn finish_owned_attempt(
    state: &AppState,
    user_id: i64,
    author_id: i64,
    provider: &str,
    provider_key: &str,
    book_id: i64,
    delivery_id: Option<i64>,
    outcome: &str,
) {
    let _ = sqlx::query(
        "UPDATE author_automation_attempts
         SET result = ?, book_id = ?, delivery_id = ?
         WHERE user_id = ? AND author_id = ? AND provider = ? AND provider_key = ?",
    )
    .bind(outcome)
    .bind(book_id)
    .bind(delivery_id)
    .bind(user_id)
    .bind(author_id)
    .bind(provider)
    .bind(provider_key)
    .execute(&state.db)
    .await;
}

async fn mark_attempt(
    state: &AppState,
    user_id: i64,
    author_id: i64,
    provider: &str,
    provider_key: &str,
    result: &str,
) {
    let _ = sqlx::query(
        "UPDATE author_automation_attempts SET result = ?
         WHERE user_id = ? AND author_id = ? AND provider = ? AND provider_key = ?",
    )
    .bind(result)
    .bind(user_id)
    .bind(author_id)
    .bind(provider)
    .bind(provider_key)
    .execute(&state.db)
    .await;
}

pub async fn handle_automation(
    state: &AppState,
    author_id: i64,
    author_name: &str,
    result: &crate::discovery::DiscoveryResult,
    first_seen: bool,
) {
    // The seen set is broad; the action is narrow. Only a genuinely new,
    // recent, plausible release may trigger anything.
    if !first_seen
        || !recently_published(result.year.map(i64::from))
        || !plausible_discovery(result)
    {
        return;
    }

    // Only an active adult's own, still-enabled readers may receive anything.
    let followers: Vec<(i64, Option<i64>)> = match sqlx::query_as(
        "SELECT f.user_id,
                (SELECT dt.id FROM delivery_targets dt
                 WHERE dt.id = f.delivery_target_id
                   AND dt.user_id = f.user_id AND dt.enabled = 1)
         FROM author_follows f
         JOIN users u ON u.id = f.user_id
         WHERE f.author_id = ? AND f.auto_acquire = 1 AND f.baseline_at IS NOT NULL
           AND u.disabled = 0 AND u.profile_type = 'adult'
           AND (u.role = 'admin' OR u.can_acquire = 1)",
    )
    .bind(author_id)
    .fetch_all(&state.db)
    .await
    {
        Ok(followers) => followers,
        Err(_) => return,
    };
    if followers.is_empty() {
        return;
    }

    // A book the household already owns satisfies automation by joining the
    // user's shelf and, when configured, sending the existing file through
    // the normal delivery service; no acquisition is created.
    if let Some(book_id) = result.owned_book_id {
        let local_language: Option<String> = if result.language.is_none() {
            sqlx::query_scalar(
                "SELECT language FROM editions
                 WHERE book_id = ? AND language IS NOT NULL
                 ORDER BY id LIMIT 1",
            )
            .bind(book_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        let language = result.language.as_deref().or(local_language.as_deref());
        for (user_id, target) in followers {
            let (languages, preferred_format) = match user_preferences(state, user_id).await {
                Ok(value) => value,
                Err(_) => continue,
            };
            if !language_accepted(language, &languages) {
                continue;
            }
            if !claim_attempt(
                state,
                user_id,
                author_id,
                &result.provider,
                &result.provider_key,
            )
            .await
            {
                continue;
            }
            let _ = crate::user_books::add(&state.db, user_id, book_id, "requested").await;
            let mut outcome = "shelved";
            let mut delivery_id: Option<i64> = None;
            if let Some(target_id) = target {
                let file_id = if languages.is_empty() {
                    crate::library::queries::preferred_existing_file(
                        &state.db,
                        book_id,
                        preferred_format.as_deref(),
                        &languages,
                    )
                    .await
                    .ok()
                    .flatten()
                    .or(result.owned_file_id)
                } else {
                    crate::library::queries::existing_file_in_languages(
                        &state.db,
                        book_id,
                        preferred_format.as_deref(),
                        &languages,
                    )
                    .await
                    .ok()
                    .flatten()
                };
                match file_id {
                    Some(file_id) => {
                        match crate::delivery::deliver(
                            state,
                            user_id,
                            book_id,
                            file_id,
                            Some(target_id),
                        )
                        .await
                        {
                            Ok(delivery) => {
                                let _ = sqlx::query(
                                    "UPDATE deliveries
                                     SET source = 'author_automation', source_author_id = ?
                                     WHERE id = ?",
                                )
                                .bind(author_id)
                                .bind(delivery.id)
                                .execute(&state.db)
                                .await;
                                delivery_id = Some(delivery.id);
                                // A persisted FAILED record is still a failure.
                                outcome = if delivery.status == "FAILED" {
                                    "delivery_failed"
                                } else {
                                    "delivered"
                                };
                            }
                            Err(error) => {
                                tracing::warn!(%error, book_id, user_id, "automation.delivery_failed");
                                outcome = "delivery_failed";
                            }
                        }
                    }
                    None => outcome = "delivery_failed",
                }
            }
            finish_owned_attempt(
                state,
                user_id,
                author_id,
                &result.provider,
                &result.provider_key,
                book_id,
                delivery_id,
                outcome,
            )
            .await;
        }
        return;
    }

    for (user_id, target) in followers {
        let (languages, preferred_format) = match user_preferences(state, user_id).await {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !language_accepted(result.language.as_deref(), &languages) {
            continue;
        }
        if !claim_attempt(
            state,
            user_id,
            author_id,
            &result.provider,
            &result.provider_key,
        )
        .await
        {
            continue;
        }
        let _ = author_name;

        let metadata = match crate::discovery::resolve_metadata(
            state,
            Some(&result.provider),
            &result.provider_key,
        )
        .await
        {
            Ok(Some(metadata)) => metadata,
            _ => {
                release_attempt(
                    state,
                    user_id,
                    author_id,
                    &result.provider,
                    &result.provider_key,
                )
                .await;
                continue;
            }
        };
        if crate::discovery::store_book(state, &metadata)
            .await
            .is_err()
        {
            release_attempt(
                state,
                user_id,
                author_id,
                &result.provider,
                &result.provider_key,
            )
            .await;
            continue;
        }
        let book_id =
            match crate::library::import_metadata::upsert_book_from_metadata(&state.db, &metadata)
                .await
            {
                Ok(book_id) => book_id,
                Err(_) => {
                    release_attempt(
                        state,
                        user_id,
                        author_id,
                        &result.provider,
                        &result.provider_key,
                    )
                    .await;
                    continue;
                }
            };

        let created = crate::acquisition::create_with_languages(
            &state.db,
            book_id,
            Some(user_id),
            preferred_format,
            languages.clone(),
            target.is_some(),
            false,
        )
        .await;
        match created {
            Ok((acquisition, duplicate)) => {
                // Joining a shared acquisition still records this user's
                // provenance and intent; only the spawn differs.
                let _ = sqlx::query(
                    "UPDATE acquisition_requests
                     SET source = 'author_automation', source_author_id = ?
                     WHERE acquisition_id = ? AND user_id = ?",
                )
                .bind(author_id)
                .bind(&acquisition.id)
                .bind(user_id)
                .execute(&state.db)
                .await;
                if !duplicate {
                    crate::acquisition_pipeline::spawn(state, acquisition.id);
                }
                mark_attempt(
                    state,
                    user_id,
                    author_id,
                    &result.provider,
                    &result.provider_key,
                    "requested",
                )
                .await;
            }
            Err(_) => {
                // The acquisition never became durable: allow a later retry.
                release_attempt(
                    state,
                    user_id,
                    author_id,
                    &result.provider,
                    &result.provider_key,
                )
                .await;
            }
        }
    }
}

async fn release_attempt(
    state: &AppState,
    user_id: i64,
    author_id: i64,
    provider: &str,
    provider_key: &str,
) {
    let _ = sqlx::query(
        "DELETE FROM author_automation_attempts
         WHERE user_id = ? AND author_id = ? AND provider = ? AND provider_key = ?",
    )
    .bind(user_id)
    .bind(author_id)
    .bind(provider)
    .bind(provider_key)
    .execute(&state.db)
    .await;
}

pub async fn refresh_followed_authors(state: &AppState) -> Result<(), AppError> {
    let authors: Vec<(i64, String)> = sqlx::query_as(
        "SELECT a.id, a.name
         FROM author_follows f
         JOIN authors a ON a.id = f.author_id
         WHERE NOT EXISTS (
             SELECT 1 FROM author_follow_refreshes r
             WHERE r.author_id = a.id AND r.checked_at > unixepoch() - ?
         )
         GROUP BY a.id
         ORDER BY a.id",
    )
    .bind(REFRESH_INTERVAL_SECONDS)
    .fetch_all(&state.db)
    .await?;

    for (author_id, name) in authors {
        let results = match discovery::search(state, SearchKind::Author, &name, 40, 0).await {
            Ok(results) => results,
            Err(error) => {
                // Leave the author due so the next hourly run retries.
                tracing::warn!(%error, author = %name, "followed author refresh failed");
                continue;
            }
        };
        for result in results.iter().filter(|result| {
            // An author search can surface unrelated works; a discovery
            // must actually be by the followed author. Display and
            // automation eligibility are applied later, so the seen set
            // stays broader than what Home shows.
            result
                .authors
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&name))
        }) {
            // DO NOTHING makes rows_affected an unambiguous first-seen
            // signal; an upsert update would also report an affected row.
            let inserted = sqlx::query(
                "INSERT INTO author_discoveries
                     (author_id, provider, provider_key, title, authors, year, cover_id, language, languages, subjects)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(author_id, provider, provider_key) DO NOTHING",
            )
            .bind(author_id)
            .bind(&result.provider)
            .bind(&result.provider_key)
            .bind(&result.title)
            .bind(result.authors.join(", "))
            .bind(result.year)
            .bind(&result.cover_id)
            .bind(&result.language)
            .bind(serde_json::to_string(&result.languages).unwrap_or_else(|_| "[]".into()))
            .bind(serde_json::to_string(&result.subjects).unwrap_or_else(|_| "[]".into()))
            .execute(&state.db)
            .await
            .map(|outcome| outcome.rows_affected() > 0)
            .unwrap_or(false);

            // Mutable metadata is refreshed separately.
            let _ = sqlx::query(
                "UPDATE author_discoveries
                 SET title = ?, authors = ?, year = ?, language = COALESCE(?, language),
                     cover_id = COALESCE(cover_id, ?), languages = ?, subjects = ?
                 WHERE author_id = ? AND provider = ? AND provider_key = ?",
            )
            .bind(&result.title)
            .bind(result.authors.join(", "))
            .bind(result.year)
            .bind(&result.language)
            .bind(&result.cover_id)
            .bind(serde_json::to_string(&result.languages).unwrap_or_else(|_| "[]".into()))
            .bind(serde_json::to_string(&result.subjects).unwrap_or_else(|_| "[]".into()))
            .bind(author_id)
            .bind(&result.provider)
            .bind(&result.provider_key)
            .execute(&state.db)
            .await;

            // Only a first-seen discovery of the followed author can trigger
            // automation; the baseline guarantees the back catalogue never
            // does.
            if result
                .authors
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&name))
            {
                handle_automation(state, author_id, &name, result, inserted).await;
            }
        }
        let _ = sqlx::query(
            "INSERT INTO author_follow_refreshes (author_id, checked_at) VALUES (?, unixepoch())
             ON CONFLICT(author_id) DO UPDATE SET checked_at = excluded.checked_at",
        )
        .bind(author_id)
        .execute(&state.db)
        .await;
    }

    Ok(())
}

/// Three honest categories: what entered the library, what Bokhylle found
/// for followed authors, and what the user requested that is now ready.
pub async fn list(state: &AppState, user_id: i64) -> Result<UpdatesResponse, AppError> {
    let visibility = crate::services::sharing::predicate("b.id", user_id);
    let library: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT b.id, b.title
         FROM books b
         WHERE {visibility} AND b.created_at > unixepoch() - 14 * 24 * 3600
           AND EXISTS (SELECT 1 FROM book_files f
                       JOIN editions e ON e.id = f.edition_id
                       WHERE e.book_id = b.id)
         ORDER BY b.created_at DESC
         LIMIT 5"
    )))
    .fetch_all(&state.db)
    .await?;

    // Display eligibility is stricter than the stored seen set: bundles and
    // many-author compilations stay out of Home, and known non-preferred
    // languages are hidden while unknown languages remain as a fallback.
    let (languages, _) = user_preferences(state, user_id).await?;
    let exclusions = crate::library::relevance::catalogue_exclusions(
        &state.db,
        user_id,
        false,
        state.demo.is_none(),
    )
    .await?;
    let sql = "SELECT author_id, authors, title, provider_key, year, cover_id, provider, languages, subjects, language
         FROM (
             SELECT d.author_id, d.authors, d.title, d.provider_key, d.year, d.cover_id,
                    d.provider, d.discovered_at, d.languages, d.subjects, d.language,
                    ROW_NUMBER() OVER (
                        PARTITION BY d.author_id
                        ORDER BY d.discovered_at DESC, d.id DESC
                    ) AS rn
             FROM author_discoveries d
             JOIN author_follows f ON f.author_id = d.author_id AND f.user_id = ?
             WHERE (length(d.authors) - length(replace(d.authors, ',', ''))) < 3
               AND lower(d.title) NOT LIKE '%collection set%'
               AND lower(d.title) NOT LIKE '%box set%'
               AND lower(d.title) NOT LIKE '%boxed set%'
               AND lower(d.title) NOT LIKE '%bundle%'
               AND lower(d.title) NOT LIKE '%complete series%'
               AND lower(d.title) NOT LIKE '%books collection%'
         )
         WHERE rn <= 40
         ORDER BY rn ASC, discovered_at DESC, author_id ASC
         LIMIT 1000";
    let candidates: Vec<DiscoveryRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(&state.db)
        .await?;

    let mut discoveries = Vec::new();
    let mut counts = std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for (author_id, authors, title, key, year, cover_id, provider, available, subjects, primary) in
        candidates
    {
        let author_names = authors.split(", ").map(str::to_string).collect::<Vec<_>>();
        let available: Vec<String> = serde_json::from_str(&available).unwrap_or_default();
        let subjects: Vec<String> = serde_json::from_str(&subjects).unwrap_or_default();
        if !crate::library::relevance::language_matches(&languages, &available, primary.as_deref())
            || !exclusions.permits(&provider, &key, &title, &author_names, &subjects)
            || counts.get(&author_id).copied().unwrap_or(0) >= 2
            || !seen.insert((provider.clone(), key.clone()))
        {
            continue;
        }
        *counts.entry(author_id).or_insert(0) += 1;
        discoveries.push(DiscoveryUpdate {
            author_id,
            authors: author_names,
            title,
            provider_key: key,
            year,
            cover_id,
            provider,
        });
        if discoveries.len() == 12 {
            break;
        }
    }

    let ready: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT b.id, b.title
         FROM acquisition_requests ar
         JOIN acquisitions ac ON ac.id = ar.acquisition_id
         JOIN books b ON b.id = ac.book_id
         WHERE ar.user_id = ? AND ac.status = 'READY' AND {visibility}
         ORDER BY ac.updated_at DESC
         LIMIT 5"
    )))
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(UpdatesResponse {
        library: library
            .into_iter()
            .map(|(book_id, title)| BookUpdate { book_id, title })
            .collect(),
        discoveries,
        ready: ready
            .into_iter()
            .map(|(book_id, title)| BookUpdate { book_id, title })
            .collect(),
    })
}

/// Direct automation outcomes with no acquisition, for the Activity view.
/// Auto-fetched acquisitions are excluded so nothing is listed twice.
pub async fn direct_outcomes(
    state: &AppState,
    user_id: i64,
) -> Result<DirectActivityResponse, AppError> {
    let rows: Vec<(i64, String, String, String, String, Option<i64>)> = sqlx::query_as(
        "SELECT a.id, a.name, b.title,
                COALESCE((SELECT group_concat(au.name, ', ')
                          FROM book_authors ba JOIN authors au ON au.id = ba.author_id
                          WHERE ba.book_id = b.id), ''),
                t.result, t.delivery_id
         FROM author_automation_attempts t
         JOIN authors a ON a.id = t.author_id
         JOIN books b ON b.id = t.book_id
         LEFT JOIN deliveries d ON d.id = t.delivery_id
         WHERE t.user_id = ?
           AND t.result IN ('shelved', 'delivered', 'delivery_failed')
         ORDER BY COALESCE(d.updated_at, t.attempted_at) DESC
         LIMIT 20",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    let items: Vec<DirectActivityItem> = rows
        .into_iter()
        .map(
            |(author_id, author_name, title, authors, result, delivery_id)| {
                let detail = match result.as_str() {
                    "delivered" => "Already in household library · Sent to your reader",
                    "delivery_failed" => "Already in household library · Delivery failed",
                    _ => "Already in household library · Added to your shelf",
                };
                DirectActivityItem {
                    author_id,
                    author_name,
                    title,
                    authors: authors
                        .split(", ")
                        .filter(|part| !part.is_empty())
                        .map(str::to_string)
                        .collect(),
                    outcome: result,
                    detail,
                    delivery_id,
                }
            },
        )
        .collect();
    Ok(DirectActivityResponse { items })
}

#[cfg(test)]
mod tests {
    use super::{plausible_discovery, recently_published};

    fn discovery(title: &str, authors: Vec<&str>) -> crate::discovery::DiscoveryResult {
        crate::discovery::DiscoveryResult {
            provider: "fake".to_string(),
            provider_key: "k".to_string(),
            title: title.to_string(),
            authors: authors.into_iter().map(str::to_string).collect(),
            year: Some(2026),
            ..Default::default()
        }
    }

    #[test]
    fn bundles_are_not_discoveries() {
        assert!(plausible_discovery(&discovery(
            "Deep Work",
            vec!["Cal Newport"]
        )));
        assert!(!plausible_discovery(&discovery(
            "7 Habits 5 Books Collection Set",
            vec!["Stephen R. Covey"]
        )));
        assert!(!plausible_discovery(&discovery(
            "Boxed set",
            vec!["A", "B", "C", "D"]
        )));
    }

    #[test]
    fn recency_gate_rejects_unknown_and_old_years() {
        assert!(
            !recently_published(None),
            "unknown dates never auto-acquire"
        );
        assert!(
            !recently_published(Some(2008)),
            "old catalogue entries are noise"
        );
        assert!(recently_published(Some(2026)));
    }
}
