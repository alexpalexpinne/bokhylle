use std::collections::HashSet;
use std::path::Path;

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::AppState;
use crate::auth::AdminUser;
use crate::error::AppError;
use crate::services::reader::ReadingDirection;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub status: Option<String>,
    pub kind: Option<String>,
    pub attention: Option<String>,
}

#[derive(Debug, FromRow)]
struct ReviewRow {
    id: i64,
    title: String,
    legacy_series_text: Option<String>,
    series_id: Option<i64>,
    series_name: Option<String>,
    series_number: Option<String>,
    series_sort_order: Option<f64>,
    publication_kind: String,
    reading_direction: Option<String>,
    classification_reviewed_at: Option<i64>,
    file_path: Option<String>,
    format: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationSuggestion {
    publication_kind: String,
    series_name: Option<String>,
    series_number: Option<String>,
    reason: String,
    needs_review: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationReviewItem {
    id: i64,
    title: String,
    file_name: Option<String>,
    format: Option<String>,
    legacy_series_text: Option<String>,
    series_id: Option<i64>,
    series_name: Option<String>,
    series_number: Option<String>,
    series_sort_order: Option<f64>,
    publication_kind: String,
    reading_direction: Option<String>,
    reviewed_at: Option<i64>,
    suggestion: ClassificationSuggestion,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationReviewPage {
    items: Vec<ClassificationReviewItem>,
    total: i64,
    page: i64,
    page_size: i64,
    counts: ClassificationReviewCounts,
}

#[derive(Debug, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationReviewCounts {
    total: i64,
    book: i64,
    comic: i64,
    manga: i64,
    simple: i64,
    needs_review: i64,
}

const PAGE_SIZE: i64 = 25;
const MAX_PAGE_SIZE: i64 = 500;

pub async fn list(
    _admin: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<ReviewQuery>,
) -> Result<Json<ClassificationReviewPage>, AppError> {
    let page = query.page.unwrap_or(1);
    if !(1..=100_000).contains(&page) {
        return Err(AppError::BadRequest(
            "page must be between 1 and 100000".into(),
        ));
    }
    let page_size = query.page_size.unwrap_or(PAGE_SIZE);
    if !(1..=MAX_PAGE_SIZE).contains(&page_size) {
        return Err(AppError::BadRequest(
            "pageSize must be between 1 and 500".into(),
        ));
    }
    let pending = match query.status.as_deref().unwrap_or("pending") {
        "pending" => true,
        "all" => false,
        _ => return Err(AppError::BadRequest("status must be pending or all".into())),
    };
    let kind = match query.kind.as_deref().unwrap_or("all") {
        "all" => None,
        "book" | "comic" | "manga" => query.kind.as_deref(),
        _ => return Err(AppError::BadRequest("unknown suggested type".into())),
    };
    let attention = match query.attention.as_deref().unwrap_or("all") {
        "all" => None,
        "simple" => Some(false),
        "review" => Some(true),
        _ => return Err(AppError::BadRequest("unknown review filter".into())),
    };
    let rows: Vec<ReviewRow> = sqlx::query_as(
        "SELECT b.id, b.title, b.series AS legacy_series_text, b.series_id,
                s.name AS series_name, b.series_number, b.series_sort_order,
                b.publication_kind, b.reading_direction, b.classification_reviewed_at,
                (SELECT f.path FROM book_files f JOIN editions e ON e.id = f.edition_id
                 WHERE e.book_id = b.id ORDER BY f.id LIMIT 1) AS file_path,
                (SELECT f.format FROM book_files f JOIN editions e ON e.id = f.edition_id
                 WHERE e.book_id = b.id ORDER BY f.id LIMIT 1) AS format
         FROM books b LEFT JOIN series s ON s.id = b.series_id
         WHERE (? = 0 OR b.classification_reviewed_at IS NULL)
           AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                       WHERE e.book_id = b.id)
         ORDER BY b.created_at DESC, b.id DESC",
    )
    .bind(pending)
    .fetch_all(&state.db)
    .await?;
    let mut counts = ClassificationReviewCounts::default();
    let mut matching = Vec::new();
    for item in rows.into_iter().map(review_item) {
        counts.total += 1;
        match item.suggestion.publication_kind.as_str() {
            "book" => counts.book += 1,
            "comic" => counts.comic += 1,
            "manga" => counts.manga += 1,
            _ => {}
        }
        if item.suggestion.needs_review {
            counts.needs_review += 1;
        } else {
            counts.simple += 1;
        }
        if kind.is_none_or(|kind| item.suggestion.publication_kind == kind)
            && attention.is_none_or(|attention| item.suggestion.needs_review == attention)
        {
            matching.push(item);
        }
    }
    let total = matching.len() as i64;
    let items = matching
        .into_iter()
        .skip(((page - 1) * page_size) as usize)
        .take(page_size as usize)
        .collect();
    Ok(Json(ClassificationReviewPage {
        items,
        total,
        page,
        page_size,
        counts,
    }))
}

fn review_item(row: ReviewRow) -> ClassificationReviewItem {
    let file_name = row.file_path.as_deref().and_then(|path| {
        Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    });
    let suggestion = suggest(
        &row.title,
        file_name.as_deref(),
        row.format.as_deref(),
        row.legacy_series_text.as_deref(),
        row.series_number.as_deref(),
    );
    ClassificationReviewItem {
        id: row.id,
        title: row.title,
        file_name,
        format: row.format,
        legacy_series_text: row.legacy_series_text,
        series_id: row.series_id,
        series_name: row.series_name,
        series_number: row.series_number,
        series_sort_order: row.series_sort_order,
        publication_kind: row.publication_kind,
        reading_direction: row.reading_direction,
        reviewed_at: row.classification_reviewed_at,
        suggestion,
    }
}

fn suggest(
    title: &str,
    file_name: Option<&str>,
    format: Option<&str>,
    legacy_series: Option<&str>,
    series_number: Option<&str>,
) -> ClassificationSuggestion {
    let stem = file_name
        .and_then(|name| Path::new(name).file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let words = format!("{title} {stem}").to_ascii_lowercase();
    let kind = if has_word(&words, "manga") {
        "manga"
    } else if format == Some("cbz") || has_word(&words, "comic") || words.contains("graphic novel")
    {
        "comic"
    } else {
        "book"
    };
    let parsed_title = parse_volume_suffix(title);
    let parsed_file = parse_volume_suffix(stem);
    let raw = legacy_series
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let series_name = raw
        .map(str::to_owned)
        .or_else(|| parsed_title.as_ref().map(|(name, _)| name.clone()))
        .or_else(|| parsed_file.as_ref().map(|(name, _)| name.clone()));
    let number = series_number
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| parsed_title.as_ref().map(|(_, number)| number.clone()))
        .or_else(|| parsed_file.as_ref().map(|(_, number)| number.clone()));
    let reason = if raw.is_some() {
        "Embedded series metadata; review the publication type and volume"
    } else if parsed_title.is_some() {
        "Volume marker in title; verify the series grouping"
    } else if parsed_file.is_some() {
        "Volume marker in filename; verify the series grouping"
    } else if format == Some("cbz") {
        "CBZ file; verify whether this is a comic or manga"
    } else {
        "No series clue found; review the publication type"
    };
    // Comics, manga and anything with a series clue deserve an individual
    // glance. Plain book suggestions can be accepted as a reviewed group.
    let needs_review = kind != "book" || series_name.is_some() || number.is_some();
    ClassificationSuggestion {
        publication_kind: kind.into(),
        series_name,
        series_number: number,
        reason: reason.into(),
        needs_review,
    }
}

fn has_word(text: &str, word: &str) -> bool {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| part == word)
}

// Suggest only explicit volume markers. Bare trailing digits, years and issue
// dates are too ambiguous to group automatically.
fn parse_volume_suffix(value: &str) -> Option<(String, String)> {
    let value = value.trim();
    let (prefix, number) = value.rsplit_once([' ', '#'])?;
    let number = number.trim_end_matches([')', ']']);
    if number.is_empty()
        || number.len() > 8
        || number
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n > 0.0)
            .is_none()
    {
        return None;
    }
    let prefix = prefix.trim_end_matches([' ', '·', ':', '-', '(', '[']);
    let lower = prefix.to_ascii_lowercase();
    for marker in ["volume", "vol.", "vol", "#"] {
        if let Some(base) = lower.strip_suffix(marker) {
            let name = prefix[..base.len()].trim_end_matches([' ', '·', ':', '-', '(', '[']);
            if !name.is_empty() {
                return Some((name.to_string(), number.to_string()));
            }
        }
    }
    // A compact issue marker, such as "Saga #4".
    if value.contains('#') && !prefix.is_empty() {
        return Some((prefix.to_string(), number.to_string()));
    }
    None
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationDecision {
    pub book_id: i64,
    pub action: String,
    #[serde(default)]
    pub only_if_pending: bool,
    pub publication_kind: Option<String>,
    pub series_id: Option<i64>,
    pub new_series_name: Option<String>,
    pub series_number: Option<String>,
    pub series_sort_order: Option<f64>,
    pub reading_direction: Option<ReadingDirection>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationBatch {
    pub decisions: Vec<ClassificationDecision>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationBatchResult {
    updated: usize,
}

pub async fn apply_batch(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<ClassificationBatch>,
) -> Result<Json<ClassificationBatchResult>, AppError> {
    if body.decisions.is_empty() || body.decisions.len() > 1000 {
        return Err(AppError::BadRequest("submit 1–1000 decisions".into()));
    }
    let mut seen = HashSet::new();
    for decision in &body.decisions {
        if !seen.insert(decision.book_id) {
            return Err(AppError::BadRequest("duplicate book in batch".into()));
        }
        if decision.action != "apply" && decision.action != "dismiss" {
            return Err(AppError::BadRequest("unknown review action".into()));
        }
        if decision.action == "apply" {
            if !matches!(
                decision.publication_kind.as_deref(),
                Some("book" | "comic" | "manga" | "magazine" | "catalogue")
            ) {
                return Err(AppError::BadRequest("choose a publication type".into()));
            }
            if decision.series_id.is_some() && decision.new_series_name.is_some() {
                return Err(AppError::BadRequest("choose one series".into()));
            }
            if decision
                .series_number
                .as_deref()
                .is_some_and(|value| value.len() > 40)
            {
                return Err(AppError::BadRequest("volume label is too long".into()));
            }
            if decision
                .series_sort_order
                .is_some_and(|value| !value.is_finite())
            {
                return Err(AppError::BadRequest(
                    "series sort order must be finite".into(),
                ));
            }
            if let Some(name) = &decision.new_series_name
                && (name.trim().is_empty() || name.trim().len() > 200)
            {
                return Err(AppError::BadRequest(
                    "series name must have 1–200 characters".into(),
                ));
            }
        }
    }

    let mut tx = state.db.begin().await?;
    for decision in &body.decisions {
        let current: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT classification_reviewed_at FROM books b WHERE id = ?
             AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                         WHERE e.book_id = b.id)",
        )
        .bind(decision.book_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(reviewed_at) = current else {
            return Err(AppError::NotFound("book not found".into()));
        };
        if decision.only_if_pending && reviewed_at.is_some() {
            return Err(AppError::Conflict(
                "a selected file was already reviewed; refresh the list".into(),
            ));
        }
        if decision.action == "dismiss" {
            sqlx::query("UPDATE books SET classification_reviewed_at = unixepoch() WHERE id = ?")
                .bind(decision.book_id)
                .execute(&mut *tx)
                .await?;
            continue;
        }
        let series_id = if let Some(id) = decision.series_id {
            let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM series WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            if exists.is_none() {
                return Err(AppError::BadRequest("series does not exist".into()));
            }
            Some(id)
        } else if let Some(name) = &decision.new_series_name {
            let name = name.trim();
            let matches: Vec<i64> =
                sqlx::query_scalar("SELECT id FROM series WHERE name = ? COLLATE NOCASE")
                    .bind(name)
                    .fetch_all(&mut *tx)
                    .await?;
            match matches.as_slice() {
                [id] => Some(*id),
                [] => Some(
                    sqlx::query("INSERT INTO series (name) VALUES (?)")
                        .bind(name)
                        .execute(&mut *tx)
                        .await?
                        .last_insert_rowid(),
                ),
                _ => {
                    return Err(AppError::Conflict(
                        "multiple series have that name; choose one explicitly".into(),
                    ));
                }
            }
        } else {
            None
        };
        let number = decision
            .series_number
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let order = decision.series_sort_order.or_else(|| {
            number
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|n| n.is_finite() && *n >= 0.0)
        });
        crate::library::metadata_fields::mark_manual(
            &mut tx,
            crate::library::metadata_fields::Scope::Book(decision.book_id),
            crate::library::metadata_fields::MetadataField::SeriesNumber,
        )
        .await?;
        sqlx::query(
            "UPDATE books SET publication_kind = ?, series_id = ?, series_link_locked = 1,
                    series_number = ?, series_sort_order = ?, reading_direction = ?,
                    classification_reviewed_at = unixepoch(), updated_at = unixepoch()
             WHERE id = ?",
        )
        .bind(&decision.publication_kind)
        .bind(series_id)
        .bind(number)
        .bind(order)
        .bind(decision.reading_direction.map(ReadingDirection::as_str))
        .bind(decision.book_id)
        .execute(&mut *tx)
        .await?;
        crate::library::refresh_fts(&mut tx, decision.book_id).await?;
    }
    tx.commit().await?;
    Ok(Json(ClassificationBatchResult {
        updated: body.decisions.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_use_explicit_clues_only() {
        let result = suggest("Saga Vol. 7", None, Some("cbz"), None, None);
        assert_eq!(result.publication_kind, "comic");
        assert_eq!(result.series_name.as_deref(), Some("Saga"));
        assert_eq!(result.series_number.as_deref(), Some("7"));
        let manga = suggest(
            "Berserk",
            Some("Berserk Manga Volume 1.5.cbz"),
            Some("cbz"),
            None,
            None,
        );
        assert_eq!(manga.publication_kind, "manga");
        assert_eq!(manga.series_name.as_deref(), Some("Berserk Manga"));
        assert_eq!(manga.series_number.as_deref(), Some("1.5"));
        assert!(
            suggest("1984", None, Some("epub"), None, None)
                .series_name
                .is_none()
        );
        assert!(
            suggest("Monocle September 2026", None, Some("pdf"), None, None)
                .series_name
                .is_none()
        );
    }
}
