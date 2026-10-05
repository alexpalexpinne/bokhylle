//! Recommendation eligibility. Every query supplies a single `viewer(id)` CTE
//! and uses `b` for the candidate book. Deliberate household browsing does not
//! apply these filters.

use std::collections::{HashMap, HashSet};

use bokhylle_core::identity::normalize_text;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::AppError;

// A book contributes its strongest deliberate signal once. Repeated deliveries
// and acquisition retries must not manufacture additional preference.
const SIGNALS: &str = ", signals AS (
    SELECT ub.book_id, CASE WHEN ub.preference = 'liked' THEN 5
        WHEN ub.source IN ('requested', 'book_request') THEN 3
        WHEN ub.source IN ('manual', 'agent', 'sent') THEN 1 ELSE 0 END AS weight
    FROM user_books ub WHERE ub.user_id = (SELECT id FROM viewer)
      AND (ub.on_shelf = 1 OR ub.preference = 'liked')
    UNION ALL
    SELECT a.book_id, 3 FROM acquisition_requests r JOIN acquisitions a ON a.id = r.acquisition_id
    WHERE r.user_id = (SELECT id FROM viewer)
    UNION ALL
    SELECT d.book_id, 1 FROM deliveries d
    WHERE d.user_id = (SELECT id FROM viewer) AND d.status = 'SENT'
), taste_books AS (
    SELECT book_id, MAX(weight) AS weight FROM signals
    WHERE weight > 0 AND {signal_visibility}
      AND (NOT EXISTS (SELECT 1 FROM users WHERE id=(SELECT id FROM viewer) AND profile_type='child')
        OR EXISTS (SELECT 1 FROM user_books shelf WHERE shelf.user_id=(SELECT id FROM viewer) AND shelf.book_id=signals.book_id AND shelf.on_shelf=1))
      AND NOT EXISTS (SELECT 1 FROM user_books rejected
        WHERE rejected.user_id = (SELECT id FROM viewer) AND rejected.book_id = signals.book_id
          AND rejected.preference = 'not_for_me')
    GROUP BY book_id
)";

#[derive(Clone, Serialize)]
pub(crate) struct Seed {
    pub id: i64,
    pub name: String,
    pub concept: String,
    pub weight: i64,
    pub followed: bool,
    pub explicit: bool,
}

pub(crate) struct Taste {
    pub subjects: Vec<Seed>,
    pub authors: Vec<Seed>,
}

/// Accumulated, profile-scoped taste shared by local rails and Spotlight.
pub(crate) async fn taste(pool: &SqlitePool, user_id: i64) -> Result<Taste, AppError> {
    let informative = super::subjects::informative_sql("s.normalized_name");
    let signals = SIGNALS.replace(
        "{signal_visibility}",
        &crate::services::sharing::predicate("signals.book_id", user_id),
    );
    let sql = format!("WITH viewer(id) AS (SELECT ?) {signals}, topic_books AS (
        SELECT DISTINCT t.book_id, t.weight, c.concept
        FROM taste_books t JOIN book_subjects bs ON bs.book_id = t.book_id
        JOIN subjects s ON s.id = bs.subject_id
        JOIN subject_concepts c ON c.normalized_name = s.normalized_name
        WHERE {informative} AND NOT EXISTS (SELECT 1 FROM user_subject_prefs h
            JOIN subject_concepts hc ON hc.normalized_name = h.normalized_name
            WHERE h.user_id = (SELECT id FROM viewer) AND h.hidden = 1 AND hc.concept = c.concept)
    ), topic_weights AS (SELECT concept, SUM(weight) AS weight FROM topic_books GROUP BY concept)
    SELECT MIN(s.id), COALESCE(MIN(CASE WHEN s.normalized_name = w.concept THEN s.name END), MIN(s.name)), w.concept, w.weight
    FROM topic_weights w JOIN subject_concepts c ON c.concept = w.concept
    JOIN subjects s ON s.normalized_name = c.normalized_name GROUP BY w.concept");
    let rows: Vec<(i64, String, String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    let mut subjects: Vec<Seed> = rows
        .into_iter()
        .filter(|(_, _, normalized, _)| {
            super::subjects::is_displayable(normalized)
                && normalized.chars().filter(|c| c.is_alphabetic()).count() >= 3
        })
        .map(|(id, name, concept, weight)| Seed {
            id,
            name,
            concept,
            weight,
            followed: false,
            explicit: false,
        })
        .collect();
    let interests: Vec<(i64, String)> = sqlx::query_as(
        "SELECT COALESCE(MIN(s.id), 0), c.concept FROM user_subject_interests i
         JOIN subject_concepts c ON c.normalized_name = i.normalized_name
         LEFT JOIN subject_concepts aliases ON aliases.concept = c.concept
         LEFT JOIN subjects s ON s.normalized_name = aliases.normalized_name
         WHERE i.user_id = ? AND NOT EXISTS (SELECT 1 FROM user_subject_prefs h
             JOIN subject_concepts hc ON hc.normalized_name = h.normalized_name
             WHERE h.user_id = i.user_id AND hc.concept = c.concept AND h.hidden = 1)
         GROUP BY c.concept ORDER BY c.concept",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    for (id, name) in interests {
        if !super::subjects::is_displayable(&normalize_text(&name)) {
            continue;
        }
        if let Some(seed) = subjects
            .iter_mut()
            .find(|s| s.concept == super::subjects::concept(&normalize_text(&name)))
        {
            seed.weight += 5;
            seed.explicit = true;
        } else {
            subjects.push(Seed {
                id,
                concept: super::subjects::concept(&normalize_text(&name)).to_string(),
                name,
                weight: 5,
                followed: false,
                explicit: true,
            });
        }
    }
    subjects.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| b.explicit.cmp(&a.explicit))
            .then_with(|| a.name.cmp(&b.name))
    });

    let sql = format!(
        "WITH viewer(id) AS (SELECT ?) {signals}
        SELECT a.id, a.name, SUM(t.weight) FROM taste_books t
        JOIN book_authors ba ON ba.book_id = t.book_id JOIN authors a ON a.id = ba.author_id
        GROUP BY a.id"
    );
    let rows: Vec<(i64, String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    let mut authors: Vec<Seed> = rows
        .into_iter()
        .map(|(id, name, weight)| Seed {
            id,
            concept: normalize_text(&name),
            name,
            weight,
            followed: false,
            explicit: false,
        })
        .collect();
    for (id, name) in crate::follows::followed_authors(pool, user_id).await? {
        if let Some(seed) = authors.iter_mut().find(|s| s.id == id) {
            seed.weight += 5;
            seed.followed = true;
        } else {
            authors.push(Seed {
                id,
                concept: normalize_text(&name),
                name,
                weight: 5,
                followed: true,
                explicit: false,
            });
        }
    }
    authors.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| b.followed.cmp(&a.followed))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Taste { subjects, authors })
}

pub(crate) async fn preferred_languages(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<String>, AppError> {
    let row: Option<(Option<String>, Option<String>)> =
        sqlx::query_as("SELECT preferred_languages, preferred_language FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    let (many, one) = row.unwrap_or_default();
    let mut languages = many
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    if languages.is_empty() {
        languages.extend(one);
    }
    Ok(languages)
}

/// Catalogue availability uses all known languages. Unknown remains eligible
/// for display; acquisition automation retains its stricter language policy.
pub(crate) fn language_matches(
    preferred: &[String],
    languages: &[String],
    language: Option<&str>,
) -> bool {
    preferred.is_empty()
        || (languages.is_empty() && language.is_none())
        || languages
            .iter()
            .map(String::as_str)
            .chain(language)
            .any(|language| preferred.iter().any(|p| p.eq_ignore_ascii_case(language)))
}

// Every caller provides preferred(language), including the empty preference set.
pub(crate) const LANGUAGE_FILTER: &str = "AND (
    NOT EXISTS (SELECT 1 FROM preferred)
    OR EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id
        JOIN preferred p ON p.language = lower(e.language) WHERE e.book_id = b.id)
    OR (NOT EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id)
        AND EXISTS (SELECT 1 FROM book_available_languages l JOIN preferred p ON p.language = lower(l.language) WHERE l.book_id = b.id))
    OR (NOT EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id)
        AND NOT EXISTS (SELECT 1 FROM book_available_languages WHERE book_id = b.id)
        AND (b.language IS NULL OR EXISTS (SELECT 1 FROM preferred p WHERE p.language = lower(b.language))))
    OR (EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = b.id)
        AND NOT EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id
            WHERE e.book_id = b.id AND e.language IS NOT NULL))
)";

pub(crate) const TASTE_CTES: &str = ", taste_subjects AS (
    SELECT s.id, c.concept, MAX(CAST(json_extract(value, '$.weight') AS INTEGER)) AS weight
    FROM json_each(?) JOIN subject_concepts c ON c.concept = json_extract(value, '$.concept')
    JOIN subjects s ON s.normalized_name = c.normalized_name GROUP BY s.id
), taste_authors AS (
    SELECT CAST(json_extract(value, '$.id') AS INTEGER) AS id,
           CAST(json_extract(value, '$.weight') AS INTEGER) AS weight FROM json_each(?)
)";

pub(crate) const AFFINITY: &str = "CAST((
    COALESCE((SELECT SUM(weight) FROM (SELECT MAX(t.weight) AS weight FROM book_subjects bs
        JOIN taste_subjects t ON t.id = bs.subject_id WHERE bs.book_id = b.id GROUP BY t.concept)), 0)
    + COALESCE((SELECT SUM(t.weight) FROM book_authors ba JOIN taste_authors t ON t.id = ba.author_id WHERE ba.book_id = b.id), 0)
) * COALESCE((SELECT CASE WHEN h.last_seen_at > unixepoch()-3*86400 THEN 0.8 ELSE 0.9 END
    FROM recommendation_candidates h WHERE h.user_id=(SELECT id FROM viewer) AND h.book_id=b.id
    AND h.last_seen_at > unixepoch()-7*86400 ORDER BY h.last_seen_at DESC LIMIT 1),1.0) AS INTEGER)";

/// Current feedback and availability, also applied to saved catalogue snapshots.
pub(crate) struct CatalogueExclusions {
    pub keys: HashSet<(String, String)>,
    hidden: HashSet<String>,
    identities: HashMap<String, HashSet<String>>,
}

impl CatalogueExclusions {
    pub fn permits(
        &self,
        provider: &str,
        key: &str,
        title: &str,
        authors: &[String],
        subjects: &[String],
    ) -> bool {
        self.rejection_reason(provider, key, title, authors, subjects)
            .is_none()
    }

    pub fn rejection_reason(
        &self,
        provider: &str,
        key: &str,
        title: &str,
        authors: &[String],
        subjects: &[String],
    ) -> Option<&'static str> {
        if self.keys.contains(&(provider.to_string(), key.to_string()))
            || self
                .identities
                .get(&normalize_text(title))
                .is_some_and(|names| authors.iter().any(|a| names.contains(&normalize_text(a))))
        {
            Some("owned_or_feedback")
        } else if subjects.iter().any(|s| {
            self.hidden
                .contains(super::subjects::concept(&normalize_text(s)))
        }) {
            Some("hidden_subject")
        } else {
            None
        }
    }
}

pub(crate) async fn catalogue_exclusions(
    pool: &SqlitePool,
    user_id: i64,
    child: bool,
    exclude_downloaded: bool,
) -> Result<CatalogueExclusions, AppError> {
    let visibility = crate::services::sharing::predicate("b.id", user_id);
    // The isolated demo suggests its existing sample copies. Those may be in
    // the household, but personal shelf membership and feedback still apply.
    let owned = if !exclude_downloaded {
        "0 = 1".to_string()
    } else if child {
        "1 = 1".to_string()
    } else {
        visibility
    };
    let sql = format!("WITH viewer(id) AS (SELECT ?)
        SELECT b.id, b.title, COALESCE((SELECT group_concat(a.name, ', ') FROM book_authors ba
            JOIN authors a ON a.id = ba.author_id WHERE ba.book_id = b.id), '')
        FROM books b WHERE ({owned} AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id WHERE e.book_id = b.id))
        OR EXISTS (SELECT 1 FROM user_books ub WHERE ub.user_id = (SELECT id FROM viewer)
            AND ub.book_id = b.id AND (ub.on_shelf = 1 OR ub.preference IN ('liked', 'not_for_me')))");
    let rows: Vec<(i64, String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    let mut keys = HashSet::new();
    let mut identities: HashMap<String, HashSet<String>> = HashMap::new();
    // These are metadata comparisons only. Never expose household ownership to
    // children, even for matching catalogue identities.
    let book_ids: Vec<_> = rows.iter().map(|(id, _, _)| *id).collect();
    for (_, title, authors) in rows {
        identities
            .entry(normalize_text(&title))
            .or_default()
            .extend(authors.split(", ").map(normalize_text));
    }
    let ids: Vec<(String, String)> = sqlx::query_as("SELECT provider, provider_key FROM book_external_ids WHERE book_id IN (SELECT value FROM json_each(?))")
        .bind(serde_json::to_string(&book_ids).map_err(|e| AppError::Unprocessable(e.to_string()))?).fetch_all(pool).await?;
    keys.extend(ids);
    let hidden: Vec<String> = sqlx::query_scalar(
        "SELECT normalized_name FROM user_subject_prefs WHERE user_id = ? AND hidden = 1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(CatalogueExclusions {
        keys,
        hidden: hidden
            .into_iter()
            .map(|s| super::subjects::concept(&s).to_string())
            .collect(),
        identities,
    })
}

pub(crate) const EXCLUSIONS: &str = "
    AND NOT EXISTS (SELECT 1 FROM recommendation_candidates dismissed
        WHERE dismissed.user_id=(SELECT id FROM viewer) AND dismissed.book_id=b.id
        AND dismissed.dismissed_until > unixepoch())
    AND NOT EXISTS (
        SELECT 1 FROM user_books excluded
        WHERE excluded.user_id = (SELECT id FROM viewer) AND excluded.book_id = b.id
          AND excluded.preference = 'not_for_me'
    )
    AND NOT EXISTS (
        SELECT 1 FROM book_subjects excluded_subject
        JOIN subjects s ON s.id = excluded_subject.subject_id
        JOIN subject_concepts topic ON topic.normalized_name = s.normalized_name
        JOIN subject_concepts alias ON alias.concept = topic.concept
        JOIN user_subject_prefs hidden ON hidden.normalized_name = alias.normalized_name
        WHERE excluded_subject.book_id = b.id
          AND hidden.user_id = (SELECT id FROM viewer) AND hidden.hidden = 1
    )";

pub(crate) fn personal() -> String {
    PERSONAL
        .replace(
            "{informative_subject}",
            &super::subjects::informative_sql("seed_topic.normalized_name"),
        )
        .replace(
            "{visible_seed}",
            &crate::services::sharing::predicate_with_viewer(
                "seed_subject.book_id",
                "(SELECT id FROM viewer)",
            ),
        )
}

const PERSONAL: &str = "
    AND (
        EXISTS (
            SELECT 1 FROM user_books own
            WHERE own.book_id = b.id AND own.user_id = (SELECT id FROM viewer)
              AND (own.on_shelf = 1 OR own.preference = 'liked')
        )
        OR EXISTS (
            SELECT 1 FROM book_authors ba JOIN author_follows followed ON followed.author_id = ba.author_id
            WHERE ba.book_id = b.id AND followed.user_id = (SELECT id FROM viewer)
        )
        OR EXISTS (
            SELECT 1 FROM book_subjects candidate_subject JOIN subjects s ON s.id = candidate_subject.subject_id
            JOIN subject_concepts topic ON topic.normalized_name = s.normalized_name
            JOIN subject_concepts alias ON alias.concept = topic.concept
            JOIN user_subject_interests interest ON interest.normalized_name = alias.normalized_name
            WHERE candidate_subject.book_id = b.id AND interest.user_id = (SELECT id FROM viewer)
        )
        OR EXISTS (
            SELECT 1 FROM book_subjects candidate_subject
            JOIN subjects candidate_topic ON candidate_topic.id = candidate_subject.subject_id
            JOIN subject_concepts candidate_concept ON candidate_concept.normalized_name = candidate_topic.normalized_name
            JOIN subject_concepts seed_concept ON seed_concept.concept = candidate_concept.concept
            JOIN subjects matched_topic ON matched_topic.normalized_name = seed_concept.normalized_name
            JOIN book_subjects seed_subject ON seed_subject.subject_id = matched_topic.id
            JOIN subjects seed_topic ON seed_topic.id = seed_subject.subject_id
            JOIN user_books seed ON seed.book_id = seed_subject.book_id
            WHERE candidate_subject.book_id = b.id AND seed.user_id = (SELECT id FROM viewer)
              AND {informative_subject} AND {visible_seed}
              AND (seed.preference IS NULL OR seed.preference = 'liked')
              AND (seed.preference = 'liked' OR (seed.on_shelf = 1
                   AND seed.source IN ('manual', 'requested', 'sent', 'agent', 'book_request')))
        )
        OR EXISTS (
            SELECT 1 FROM book_subjects candidate_subject
            JOIN subjects candidate_topic ON candidate_topic.id = candidate_subject.subject_id
            JOIN subject_concepts candidate_concept ON candidate_concept.normalized_name = candidate_topic.normalized_name
            JOIN subject_concepts seed_concept ON seed_concept.concept = candidate_concept.concept
            JOIN subjects matched_topic ON matched_topic.normalized_name = seed_concept.normalized_name
            JOIN book_subjects seed_subject ON seed_subject.subject_id = matched_topic.id
            JOIN subjects seed_topic ON seed_topic.id = seed_subject.subject_id
            JOIN acquisitions acquisition ON acquisition.book_id = seed_subject.book_id
            JOIN acquisition_requests request ON request.acquisition_id = acquisition.id
            WHERE candidate_subject.book_id = b.id AND request.user_id = (SELECT id FROM viewer)
              AND {informative_subject} AND {visible_seed}
              AND NOT EXISTS (SELECT 1 FROM user_books rejected
                  WHERE rejected.user_id = request.user_id AND rejected.book_id = acquisition.book_id
                    AND rejected.preference = 'not_for_me')
        )
        OR EXISTS (
            SELECT 1 FROM book_subjects candidate_subject
            JOIN subjects candidate_topic ON candidate_topic.id = candidate_subject.subject_id
            JOIN subject_concepts candidate_concept ON candidate_concept.normalized_name = candidate_topic.normalized_name
            JOIN subject_concepts seed_concept ON seed_concept.concept = candidate_concept.concept
            JOIN subjects matched_topic ON matched_topic.normalized_name = seed_concept.normalized_name
            JOIN book_subjects seed_subject ON seed_subject.subject_id = matched_topic.id
            JOIN subjects seed_topic ON seed_topic.id = seed_subject.subject_id
            JOIN deliveries delivery ON delivery.book_id = seed_subject.book_id
            WHERE candidate_subject.book_id = b.id AND delivery.user_id = (SELECT id FROM viewer)
              AND {informative_subject} AND {visible_seed}
              AND delivery.status = 'SENT'
              AND NOT EXISTS (SELECT 1 FROM user_books rejected
                  WHERE rejected.user_id = delivery.user_id AND rejected.book_id = delivery.book_id
                    AND rejected.preference = 'not_for_me')
        )
    )";
