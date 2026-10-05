//! Profile-scoped ranking and feedback. No acquisition or shelf side effects.
use std::collections::{HashMap, HashSet};

use bokhylle_core::identity::normalize_text;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::library::{relevance::Taste, subjects};
use crate::routes::spotlight::SpotlightItem;
use crate::{AppState, auth::User, error::AppError};

pub fn identity(item: &SpotlightItem) -> String {
    let mut authors: Vec<_> = item.authors.iter().map(|a| normalize_text(a)).collect();
    authors.sort();
    hex::encode(Sha256::digest(
        serde_json::to_vec(&(normalize_text(&item.title), authors)).expect("identity serializes"),
    ))
}

#[derive(Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScoreExplanation {
    pub key: String,
    pub title: String,
    pub affinity: i64,
    #[serde(default)]
    pub subject_affinity: i64,
    #[serde(default)]
    pub author_affinity: i64,
    pub score: i64,
    pub matching_subjects: Vec<String>,
    pub matching_authors: Vec<String>,
    pub recently_seen: bool,
}

pub(crate) fn affinity(item: &SpotlightItem, taste: &Taste) -> ScoreExplanation {
    let topics: HashSet<_> = item
        .subjects
        .iter()
        .map(|s| {
            let normalized = normalize_text(s);
            subjects::concept(&normalized).to_string()
        })
        .collect();
    let authors: HashSet<_> = item.authors.iter().map(|a| normalize_text(a)).collect();
    let matched_topics: Vec<_> = taste
        .subjects
        .iter()
        .filter(|s| topics.contains(&s.concept))
        .collect();
    let matched_authors: Vec<_> = taste
        .authors
        .iter()
        .filter(|s| authors.contains(&s.concept))
        .collect();
    let subject_affinity = matched_topics.iter().map(|s| s.weight).sum::<i64>();
    let author_affinity = matched_authors.iter().map(|s| s.weight).sum::<i64>();
    let score = subject_affinity + author_affinity;
    ScoreExplanation {
        key: identity(item),
        title: item.title.clone(),
        affinity: score,
        subject_affinity,
        author_affinity,
        score,
        matching_subjects: matched_topics.into_iter().map(|s| s.name.clone()).collect(),
        matching_authors: matched_authors
            .into_iter()
            .map(|s| s.name.clone())
            .collect(),
        recently_seen: false,
    }
}

pub(crate) async fn rank(
    state: &AppState,
    user_id: i64,
    taste: &Taste,
    items: &mut Vec<SpotlightItem>,
) -> Result<Vec<ScoreExplanation>, AppError> {
    let now = epoch();
    let history: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT identity_key,last_seen_at,dismissed_until FROM recommendation_candidates WHERE user_id = ? AND (last_seen_at > ? OR dismissed_until > ?)",
    ).bind(user_id).bind(now - 7 * 86400).bind(now).fetch_all(&state.db).await?;
    let history: HashMap<_, _> = history
        .into_iter()
        .map(|(key, seen, dismissed)| (key, (seen, dismissed)))
        .collect();
    let mut scores = HashMap::new();
    items.retain_mut(|item| {
        let mut score = affinity(item, taste);
        if let Some((seen, dismissed)) = history.get(&score.key) {
            if dismissed.is_some_and(|until| until > now) {
                return false;
            }
            if seen.is_some_and(|at| at > now - 7 * 86400) {
                score.recently_seen = true;
                score.score = (score.score
                    * if seen.is_some_and(|at| at > now - 3 * 86400) {
                        80
                    } else {
                        90
                    })
                    / 100;
            }
        }
        if item.source == "discover"
            && (!score.matching_subjects.is_empty() || !score.matching_authors.is_empty())
        {
            item.reason_label = if !score.matching_subjects.is_empty() {
                format!(
                    "Matches your interests: {}",
                    score
                        .matching_subjects
                        .iter()
                        .take(2)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" and ")
                )
            } else {
                format!("More from {}", score.matching_authors[0])
            };
        }
        item.recommendation_key = Some(score.key.clone());
        scores.insert(score.key.clone(), score);
        true
    });
    // Stable sort preserves provider relevance and source mixing for ties.
    items.sort_by_key(|item| std::cmp::Reverse(scores[&identity(item)].score));
    Ok(items
        .iter()
        .map(|item| scores[&identity(item)].clone())
        .collect())
}

pub(crate) async fn offer(
    state: &AppState,
    user_id: i64,
    items: &[SpotlightItem],
) -> Result<(), AppError> {
    let mut tx = state.db.begin().await?;
    for item in items {
        sqlx::query("INSERT INTO recommendation_candidates(user_id,identity_key,book_id,provider,provider_key) VALUES(?,?,?,?,?) ON CONFLICT(user_id,identity_key) DO UPDATE SET offered_at = unixepoch(),book_id = excluded.book_id,provider = excluded.provider,provider_key = excluded.provider_key")
            .bind(user_id).bind(identity(item)).bind(item.book_id).bind(&item.provider).bind(&item.provider_key).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM recommendation_candidates WHERE user_id = ? AND offered_at < unixepoch() - 30 * 86400 AND COALESCE(dismissed_until,0) < unixepoch()")
        .bind(user_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

type Candidate = (Option<i64>, Option<String>, Option<String>);
async fn candidate(state: &AppState, user: &User, key: &str) -> Result<Candidate, AppError> {
    let row: Candidate = sqlx::query_as("SELECT book_id,provider,provider_key FROM recommendation_candidates WHERE user_id = ? AND identity_key = ? AND offered_at > unixepoch() - 86400")
        .bind(user.id).bind(key).fetch_optional(&state.db).await?.ok_or_else(|| AppError::NotFound("suggestion expired".into()))?;
    if let Some(id) = row.0 {
        crate::services::sharing::require_access(&state.db, user.id, id).await?;
        if crate::auth::profile_type(&state.db, user.id).await? == "child"
            && !crate::user_books::contains(&state.db, user.id, id).await?
        {
            return Err(AppError::NotFound("book not found".into()));
        }
    } else if !crate::services::requests::may_discover(state, user.id).await? {
        return Err(AppError::Forbidden);
    }
    Ok(row)
}

#[derive(Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackAction {
    Like,
    NotForMe,
    Dismiss,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackReceipt {
    pub undo_token: String,
}

pub async fn feedback(
    state: &AppState,
    user: &User,
    key: &str,
    action: FeedbackAction,
) -> Result<FeedbackReceipt, AppError> {
    if matches!(action, FeedbackAction::NotForMe)
        && !crate::services::requests::is_adult(state, user.id).await?
    {
        return Err(AppError::Forbidden);
    }
    let (book_id, provider, provider_key) = candidate(state, user, key).await?;
    let preference = match action {
        FeedbackAction::Like => Some("liked"),
        FeedbackAction::NotForMe => Some("not_for_me"),
        FeedbackAction::Dismiss => None,
    };
    let preference_book = if preference.is_some() {
        Some(match book_id {
            Some(id) => id,
            None => {
                crate::services::books::catalogue_preference_book(
                    state,
                    user,
                    provider.as_deref(),
                    provider_key
                        .as_deref()
                        .ok_or_else(|| AppError::NotFound("suggestion unavailable".into()))?,
                )
                .await?
            }
        })
    } else {
        None
    };
    let token = uuid::Uuid::new_v4().to_string();
    let mut tx = state.db.begin().await?;
    let previous_dismissed: Option<i64> = sqlx::query_scalar(
        "SELECT dismissed_until FROM recommendation_candidates WHERE user_id=? AND identity_key=?",
    )
    .bind(user.id)
    .bind(key)
    .fetch_one(&mut *tx)
    .await?;
    let mut previous_preference: Option<String> = None;
    let mut applied_dismissed: Option<i64> = None;
    if let Some(id) = preference_book {
        crate::services::sharing::require_access_tx(&mut tx, user.id, id).await?;
        previous_preference =
            sqlx::query_scalar("SELECT preference FROM user_books WHERE user_id=? AND book_id=?")
                .bind(user.id)
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        sqlx::query("INSERT INTO user_books(user_id,book_id,source,on_shelf,preference) VALUES(?,?,'manual',0,?) ON CONFLICT(user_id,book_id) DO UPDATE SET preference=excluded.preference")
            .bind(user.id).bind(id).bind(preference).execute(&mut *tx).await?;
        // Different provider keys can resolve to the same book. A newer
        // preference invalidates older undo receipts for that book as well.
        sqlx::query("DELETE FROM recommendation_feedback_undo WHERE user_id=? AND book_id=?")
            .bind(user.id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    } else {
        applied_dismissed = Some(epoch() + 7 * 86400);
        sqlx::query("UPDATE recommendation_candidates SET dismissed_until=? WHERE user_id=? AND identity_key=?")
            .bind(applied_dismissed).bind(user.id).bind(key).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM recommendation_feedback_undo WHERE expires_at < unixepoch()")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO recommendation_feedback_undo(user_id,identity_key,token,book_id,previous_preference,applied_preference,previous_dismissed_until,applied_dismissed_until,expires_at) VALUES(?,?,?,?,?,?,?,?,unixepoch()+600) ON CONFLICT(user_id,identity_key) DO UPDATE SET token=excluded.token,book_id=excluded.book_id,previous_preference=excluded.previous_preference,applied_preference=excluded.applied_preference,previous_dismissed_until=excluded.previous_dismissed_until,applied_dismissed_until=excluded.applied_dismissed_until,expires_at=excluded.expires_at")
        .bind(user.id).bind(key).bind(&token).bind(preference_book).bind(previous_preference)
        .bind(preference).bind(previous_dismissed).bind(applied_dismissed).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(FeedbackReceipt { undo_token: token })
}

pub async fn undo_feedback(
    state: &AppState,
    user: &User,
    key: &str,
    token: &str,
) -> Result<(), AppError> {
    candidate(state, user, key).await?;
    let mut tx = state.db.begin().await?;
    type Undo = (
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
    );
    let (book, before, applied, dismissed_before, dismissed_applied): Undo = sqlx::query_as("SELECT book_id,previous_preference,applied_preference,previous_dismissed_until,applied_dismissed_until FROM recommendation_feedback_undo WHERE user_id=? AND identity_key=? AND token=? AND expires_at > unixepoch()")
        .bind(user.id).bind(key).bind(token).fetch_optional(&mut *tx).await?
        .ok_or_else(|| AppError::NotFound("Undo is no longer available".into()))?;
    if let Some(id) = book {
        let assigned: bool = sqlx::query_scalar("SELECT profile_type <> 'child' OR EXISTS(SELECT 1 FROM user_books WHERE user_id=? AND book_id=? AND on_shelf=1) FROM users WHERE id=?")
            .bind(user.id).bind(id).bind(user.id).fetch_one(&mut *tx).await?;
        if !assigned {
            return Err(AppError::NotFound("book not found".into()));
        }
    }
    let changed = if let Some(id) = book {
        crate::services::sharing::require_access_tx(&mut tx, user.id, id).await?;
        sqlx::query(
            "UPDATE user_books SET preference=? WHERE user_id=? AND book_id=? AND preference IS ?",
        )
        .bind(before)
        .bind(user.id)
        .bind(id)
        .bind(applied)
        .execute(&mut *tx)
        .await?
        .rows_affected()
    } else {
        sqlx::query("UPDATE recommendation_candidates SET dismissed_until=? WHERE user_id=? AND identity_key=? AND dismissed_until IS ?")
            .bind(dismissed_before).bind(user.id).bind(key).bind(dismissed_applied).execute(&mut *tx).await?.rows_affected()
    };
    if changed != 1 {
        return Err(AppError::Conflict(
            "This suggestion has changed since your feedback".into(),
        ));
    }
    sqlx::query(
        "DELETE FROM recommendation_feedback_undo WHERE user_id=? AND identity_key=? AND token=?",
    )
    .bind(user.id)
    .bind(key)
    .bind(token)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn impressions(state: &AppState, user: &User, keys: &[String]) -> Result<(), AppError> {
    if keys.len() > 80 {
        return Err(AppError::BadRequest("too many impressions".into()));
    }
    // Validate the complete batch before writing; keys from another profile do
    // not establish history or disclose their metadata.
    for key in keys {
        candidate(state, user, key).await?;
    }
    sqlx::query("UPDATE recommendation_candidates SET last_seen_at = unixepoch() WHERE user_id = ? AND identity_key IN (SELECT value FROM json_each(?)) AND (last_seen_at IS NULL OR last_seen_at < unixepoch() - 86400)")
        .bind(user.id).bind(serde_json::to_string(keys).expect("keys serialize")).execute(&state.db).await?;
    Ok(())
}

pub fn epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationDiagnostics {
    #[serde(default)]
    pub filtered: std::collections::BTreeMap<String, usize>,
    #[serde(default)]
    pub timings_ms: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    pub catalogue_cached: bool,
    pub retrieved: usize,
    pub eligible: usize,
    pub selected: usize,
    pub elapsed_ms: u64,
    pub cached_only: bool,
    pub scores: Vec<ScoreExplanation>,
}

pub(crate) async fn save_diagnostics(
    state: &AppState,
    user_id: i64,
    value: &RecommendationDiagnostics,
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO metadata_cache(key,value,fetched_at,expires_at) VALUES(?,?,unixepoch(),unixepoch()+86400) ON CONFLICT(key) DO UPDATE SET value=excluded.value,fetched_at=excluded.fetched_at,expires_at=excluded.expires_at")
        .bind(format!("recommendation-diagnostics:{user_id}"))
        .bind(serde_json::to_string(value).expect("diagnostics serialize")).execute(&state.db).await?;
    Ok(())
}

pub async fn diagnostics(
    state: &AppState,
    user_id: i64,
) -> Result<RecommendationDiagnostics, AppError> {
    let value: Option<String> = sqlx::query_scalar(
        "SELECT value FROM metadata_cache WHERE key = ? AND expires_at > unixepoch()",
    )
    .bind(format!("recommendation-diagnostics:{user_id}"))
    .fetch_optional(&state.db)
    .await?;
    Ok(value
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default())
}

pub async fn restore_rejection(
    state: &AppState,
    user_id: i64,
    book_id: i64,
) -> Result<(), AppError> {
    sqlx::query("UPDATE user_books SET preference=NULL WHERE user_id=? AND book_id=? AND preference='not_for_me'")
        .bind(user_id).bind(book_id).execute(&state.db).await?;
    Ok(())
}
