use std::collections::{HashMap, HashSet};

use serde::Serialize;
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};

use super::metadata_fields::{self, MetadataSource, Scope};
use crate::error::AppError;
use crate::library::{relevance, subjects};
use crate::services::sharing;

const OWNED_FILTER: &str = "WHERE EXISTS (
    SELECT 1 FROM book_files f
    JOIN editions e ON e.id = f.edition_id
    WHERE e.book_id = b.id
)";

const BOOK_SELECT: &str = "SELECT b.id, b.title, b.language,
       CASE WHEN b.series_id IS NOT NULL
            THEN (SELECT s.name FROM series s WHERE s.id = b.series_id)
            WHEN b.series_link_locked = 1 THEN NULL ELSE b.series END AS series,
       b.series_number,
       b.series_id, b.series_sort_order, b.publication_kind,
       (b.cover_path IS NOT NULL) AS has_cover,
       (b.description IS NOT NULL AND length(trim(b.description)) > 120) AS has_description,
       b.rating,
       b.rating_count,
       b.rating_source,
       b.created_at,
       COALESCE((
           SELECT group_concat(a.name, ', ')
           FROM book_authors ba
           JOIN authors a ON a.id = ba.author_id
           WHERE ba.book_id = b.id
       ), '') AS authors
FROM books b";

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookSummary {
    pub id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
    pub series_id: Option<i64>,
    pub series_sort_order: Option<f64>,
    pub publication_kind: String,
    pub has_cover: bool,
    pub has_description: bool,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
    pub rating_source: Option<String>,
    pub added_at: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditionDetail {
    pub metadata_sources: Vec<MetadataSource>,
    pub id: i64,
    pub title: String,
    pub language: Option<String>,
    pub publication_year: Option<i64>,
    pub isbn10: Option<String>,
    pub isbn13: Option<String>,
    pub publisher: Option<String>,
    pub unknown: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileDetail {
    pub id: i64,
    pub edition_id: i64,
    pub format: String,
    pub size: i64,
    pub filename: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubjectName {
    pub name: String,
    pub normalized: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthorRef {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookDetail {
    pub metadata_sources: Vec<MetadataSource>,
    pub id: i64,
    pub title: String,
    pub authors: Vec<String>,
    /// Durable author identities for linking; `authors` stays the display
    /// names for every existing consumer.
    pub author_refs: Vec<AuthorRef>,
    pub language: Option<String>,
    /// Catalogue availability; this is separate from the language of a file.
    pub available_languages: Vec<String>,
    pub series: Option<String>,
    pub legacy_series_text: Option<String>,
    pub series_number: Option<String>,
    pub series_id: Option<i64>,
    pub series_sort_order: Option<f64>,
    pub publication_kind: String,
    pub reading_direction: Option<String>,
    pub has_cover: bool,
    pub added_at: i64,
    pub rating: Option<f64>,
    pub rating_count: Option<i64>,
    pub rating_source: Option<String>,
    pub description: Option<String>,
    pub publication_year: Option<i64>,
    pub sharing: Option<sharing::BookSharing>,
    pub sharing_managed: bool,
    pub shared_in_household: bool,
    pub on_shelf: bool,
    pub preference: Option<String>,
    /// The viewer's last browser EPUB, populated by the book service.
    pub browser_file_id: Option<i64>,
    pub subjects: Vec<SubjectName>,
    pub editions: Vec<EditionDetail>,
    pub files: Vec<FileDetail>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthorSummary {
    pub id: i64,
    pub name: String,
    pub book_count: i64,
    pub following: bool,
    pub auto_acquire: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthorDetail {
    pub id: i64,
    pub name: String,
    pub books: Vec<BookSummary>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookPage {
    pub items: Vec<BookSummary>,
    pub total: i64,
    pub letters: Vec<String>,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeriesSummary {
    pub id: i64,
    pub name: String,
    pub sort_name: Option<String>,
    pub default_reading_direction: Option<String>,
    pub volume_count: i64,
    pub cover_book_id: i64,
    pub added_at: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum ComicTile {
    Series(SeriesSummary),
    Book(BookSummary),
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComicShelfPage {
    pub items: Vec<ComicTile>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeriesDetail {
    pub id: i64,
    pub name: String,
    pub sort_name: Option<String>,
    pub default_reading_direction: Option<String>,
    pub volumes: Vec<BookSummary>,
    pub reading: SeriesReading,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeriesReading {
    pub finished_book_ids: Vec<i64>,
    pub current: Option<SeriesCurrent>,
    pub next_book_id: Option<i64>,
    pub missing_next_volume: Option<i64>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeriesCurrent {
    pub book_id: i64,
    pub browser_file_id: Option<i64>,
    pub percentage: f64,
}

#[derive(FromRow)]
struct ComicTileRow {
    tile_type: String,
    item_id: i64,
    added_at: i64,
    volume_count: i64,
    cover_book_id: i64,
}

pub async fn comic_shelf_visible(
    pool: &SqlitePool,
    mine: Option<i64>,
    viewer_id: i64,
    sort: &str,
    page: i64,
    page_size: i64,
) -> Result<ComicShelfPage, AppError> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let scope = if mine.is_some() {
        "AND EXISTS (SELECT 1 FROM user_books ub WHERE ub.book_id = b.id AND ub.user_id = ? AND ub.on_shelf = 1)"
    } else {
        ""
    };
    let visibility = sharing::predicate("b.id", viewer_id);
    let tiles = format!(
        "WITH eligible AS (
            SELECT b.id, b.series_id, b.created_at, b.normalized_title FROM books b
            WHERE b.publication_kind IN ('comic', 'manga')
              AND {} {scope} AND {visibility}
         ), tiles AS (
            SELECT 'series' AS tile_type, series_id AS item_id,
                   MAX(created_at) AS added_at, COUNT(*) AS volume_count,
                   MAX(id) AS cover_book_id,
                   (SELECT lower(coalesce(s.sort_name, s.name)) FROM series s WHERE s.id = series_id) AS sort_title
            FROM eligible WHERE series_id IS NOT NULL GROUP BY series_id
            UNION ALL
            SELECT 'book', id, created_at, 1, id, normalized_title
            FROM eligible WHERE series_id IS NULL
         )",
        OWNED_FILTER.trim_start_matches("WHERE ")
    );
    let count_sql = format!("{tiles} SELECT count(*) FROM tiles");
    let mut count_query = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(count_sql));
    if let Some(user_id) = mine {
        count_query = count_query.bind(user_id);
    }
    let total = count_query.fetch_one(pool).await?;
    let rows_sql = format!(
        "{tiles} SELECT tile_type, item_id, added_at, volume_count, cover_book_id
         FROM tiles ORDER BY {} LIMIT ? OFFSET ?",
        if sort == "title" {
            "sort_title COLLATE NOCASE, item_id"
        } else {
            "added_at DESC, tile_type, item_id"
        }
    );
    let mut rows_query = sqlx::query_as::<_, ComicTileRow>(sqlx::AssertSqlSafe(rows_sql));
    if let Some(user_id) = mine {
        rows_query = rows_query.bind(user_id);
    }
    let rows = rows_query
        .bind(page_size)
        .bind((page - 1) * page_size)
        .fetch_all(pool)
        .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        if row.tile_type == "series" {
            let series: (String, Option<String>, Option<String>) = sqlx::query_as(
                "SELECT name, sort_name, default_reading_direction FROM series WHERE id = ?",
            )
            .bind(row.item_id)
            .fetch_one(pool)
            .await?;
            items.push(ComicTile::Series(SeriesSummary {
                id: row.item_id,
                name: series.0,
                sort_name: series.1,
                default_reading_direction: series.2,
                volume_count: row.volume_count,
                cover_book_id: row.cover_book_id,
                added_at: row.added_at,
            }));
        } else {
            let sql = format!("{BOOK_SELECT} WHERE b.id = ?");
            let book: BookRow = sqlx::query_as(sqlx::AssertSqlSafe(sql))
                .bind(row.item_id)
                .fetch_one(pool)
                .await?;
            items.push(ComicTile::Book(book.into_summary()));
        }
    }
    Ok(ComicShelfPage {
        items,
        total,
        page,
        page_size,
    })
}

pub async fn comic_series(
    pool: &SqlitePool,
    id: i64,
    mine: Option<i64>,
    reader_user_id: i64,
) -> Result<Option<SeriesDetail>, AppError> {
    let series: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT name, sort_name, default_reading_direction FROM series WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some((name, sort_name, default_reading_direction)) = series else {
        return Ok(None);
    };
    let visibility = sharing::predicate("b.id", reader_user_id);
    let scope = if mine.is_some() {
        "AND EXISTS (SELECT 1 FROM user_books ub WHERE ub.book_id = b.id AND ub.user_id = ? AND ub.on_shelf = 1)"
    } else {
        ""
    };
    let sql = format!(
        "{BOOK_SELECT} WHERE b.series_id = ? AND b.publication_kind IN ('comic', 'manga')
         AND {} {scope} AND {visibility}
         ORDER BY b.series_sort_order IS NULL, b.series_sort_order,
                  b.normalized_title, b.id",
        OWNED_FILTER.trim_start_matches("WHERE ")
    );
    let mut query = sqlx::query_as::<_, BookRow>(sqlx::AssertSqlSafe(sql)).bind(id);
    if let Some(user_id) = mine {
        query = query.bind(user_id);
    }
    let volumes: Vec<BookSummary> = query
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(BookRow::into_summary)
        .collect();
    if volumes.is_empty() {
        return Ok(None);
    }
    let reading = series_reading(pool, id, reader_user_id, &volumes).await?;
    Ok(Some(SeriesDetail {
        id,
        name,
        sort_name,
        default_reading_direction,
        volumes,
        reading,
    }))
}

async fn series_reading(
    pool: &SqlitePool,
    series_id: i64,
    user_id: i64,
    volumes: &[BookSummary],
) -> Result<SeriesReading, AppError> {
    let visible: HashSet<i64> = volumes.iter().map(|volume| volume.id).collect();
    let completed: Vec<i64> = sqlx::query_scalar(
        "SELECT c.book_id FROM user_book_completions c
         JOIN books b ON b.id = c.book_id
         WHERE c.user_id = ? AND b.series_id = ?",
    )
    .bind(user_id)
    .bind(series_id)
    .fetch_all(pool)
    .await?;
    let finished: HashSet<i64> = completed
        .into_iter()
        .filter(|id| visible.contains(id))
        .collect();
    let mut finished_book_ids: Vec<i64> = finished.iter().copied().collect();
    finished_book_ids.sort_unstable();

    // Read from the complete series, rather than the global Continue reading
    // rail. A busy household can easily push a volume past that rail's limit.
    let activity: Vec<(i64, Option<i64>, f64)> = sqlx::query_as(
        "SELECT activity.book_id, activity.browser_file_id, activity.percentage
         FROM (
             SELECT e.book_id, p.book_file_id AS browser_file_id,
                    p.percentage, p.updated_at, 0 AS source_rank
             FROM browser_reading_positions p
             JOIN book_files f ON f.id = p.book_file_id AND f.sha256 = p.sha256
             JOIN editions e ON e.id = f.edition_id
             WHERE p.user_id = ? AND p.completed = 0 AND p.percentage > 0
             UNION ALL
             SELECT rp.book_id, NULL, rp.percentage, rp.updated_at, 1
             FROM reading_progress rp
             WHERE rp.user_id = ? AND rp.source != 'bokhylle'
               AND rp.book_id IS NOT NULL AND rp.percentage > 0
         ) activity
         JOIN books b ON b.id = activity.book_id
         WHERE b.series_id = ?
         ORDER BY activity.updated_at DESC, activity.source_rank",
    )
    .bind(user_id)
    .bind(user_id)
    .bind(series_id)
    .fetch_all(pool)
    .await?;
    let current = activity
        .into_iter()
        .find(|(book_id, _, _)| visible.contains(book_id) && !finished.contains(book_id))
        .map(|(book_id, browser_file_id, percentage)| SeriesCurrent {
            book_id,
            browser_file_id,
            percentage,
        });

    // Specials and fractional issues do not establish a successor. A regular
    // volume needs an unambiguous positive integer order and matching label.
    let mut numbered: HashMap<i64, Vec<i64>> = HashMap::new();
    for volume in volumes {
        if let (Some(label), Some(order)) = (&volume.series_number, volume.series_sort_order)
            && let Ok(number) = label.trim().parse::<i64>()
            && number > 0
            && order == number as f64
        {
            numbered.entry(number).or_default().push(volume.id);
        }
    }
    let highest_finished = numbered
        .iter()
        .filter(|(_, ids)| ids.len() == 1 && finished.contains(&ids[0]))
        .map(|(number, _)| *number)
        .max();
    let mut next_book_id = None;
    let mut missing_next_volume = None;
    if let Some(next_number) = highest_finished.and_then(|number| number.checked_add(1)) {
        match numbered.get(&next_number) {
            Some(ids) if ids.len() == 1 && !finished.contains(&ids[0]) => {
                next_book_id = Some(ids[0]);
            }
            None if numbered.keys().any(|number| *number > next_number) => {
                missing_next_volume = Some(next_number);
            }
            _ => {}
        }
    }
    Ok(SeriesReading {
        finished_book_ids,
        current,
        next_book_id,
        missing_next_volume,
    })
}

#[derive(FromRow)]
struct EditionRow {
    id: i64,
    title: String,
    language: Option<String>,
    publication_year: Option<i64>,
    isbn10: Option<String>,
    isbn13: Option<String>,
    publisher: Option<String>,
    is_unknown: i64,
    provider: Option<String>,
    provider_key: Option<String>,
}

#[derive(FromRow)]
struct BookRow {
    id: i64,
    title: String,
    language: Option<String>,
    series: Option<String>,
    series_number: Option<String>,
    series_id: Option<i64>,
    series_sort_order: Option<f64>,
    publication_kind: String,
    has_cover: i64,
    has_description: i64,
    rating: Option<f64>,
    rating_count: Option<i64>,
    rating_source: Option<String>,
    created_at: i64,
    authors: String,
}

#[derive(FromRow)]
struct ProgressRow {
    id: i64,
    title: String,
    language: Option<String>,
    series: Option<String>,
    series_number: Option<String>,
    series_id: Option<i64>,
    series_sort_order: Option<f64>,
    publication_kind: String,
    has_cover: i64,
    has_description: i64,
    rating: Option<f64>,
    rating_count: Option<i64>,
    rating_source: Option<String>,
    created_at: i64,
    authors: String,
    percentage: f64,
    updated_at: i64,
    source: String,
    browser_file_id: Option<i64>,
    browser_percentage: Option<f64>,
    epub_file_id: Option<i64>,
}

impl ProgressRow {
    fn into_progress(self) -> ReadingProgress {
        let book = BookRow {
            id: self.id,
            title: self.title,
            language: self.language,
            series: self.series,
            series_number: self.series_number,
            series_id: self.series_id,
            series_sort_order: self.series_sort_order,
            publication_kind: self.publication_kind,
            has_cover: self.has_cover,
            has_description: self.has_description,
            rating: self.rating,
            rating_count: self.rating_count,
            rating_source: self.rating_source,
            created_at: self.created_at,
            authors: self.authors,
        };
        ReadingProgress {
            book: book.into_summary(),
            percentage: self.percentage,
            updated_at: self.updated_at,
            source: self.source,
            browser_file_id: self.browser_file_id,
            browser_percentage: self.browser_percentage,
            epub_file_id: self.epub_file_id,
        }
    }
}

impl BookRow {
    fn into_summary(self) -> BookSummary {
        BookSummary {
            id: self.id,
            title: self.title,
            authors: split_authors(&self.authors),
            language: self.language,
            series: self.series,
            series_number: self.series_number,
            series_id: self.series_id,
            series_sort_order: self.series_sort_order,
            publication_kind: self.publication_kind,
            has_cover: self.has_cover != 0,
            has_description: self.has_description != 0,
            rating: self.rating,
            rating_count: self.rating_count,
            rating_source: self.rating_source,
            added_at: self.created_at,
        }
    }
}

fn split_authors(authors: &str) -> Vec<String> {
    authors
        .split(", ")
        .map(str::trim)
        .filter(|author| !author.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Default, Clone)]
pub struct BookFilters {
    pub viewer_id: Option<i64>,
    pub mine: Option<i64>,
    pub kind: Option<String>,
    pub format: Option<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub subject: Option<String>,
    pub collection: Option<i64>,
    pub letter: Option<String>,
    pub missing: Option<String>,
}

impl BookFilters {
    fn push_predicates(&self, query: &mut QueryBuilder<Sqlite>, author_sort: bool, prefix: &str) {
        query
            .push(prefix)
            .push(OWNED_FILTER.trim_start_matches("WHERE "));

        query.push(" AND ").push(sharing::predicate(
            "b.id",
            self.viewer_id.or(self.mine).unwrap_or(-1),
        ));
        if let Some(mine) = self.mine {
            query.push(
                " AND EXISTS (
                    SELECT 1 FROM user_books ub
                    WHERE ub.book_id = b.id AND ub.user_id = ",
            );
            query.push_bind(mine).push(" AND ub.on_shelf = 1)");
        }
        if let Some(kind) = &self.kind {
            match kind.as_str() {
                "books" => query.push(" AND b.publication_kind IN ('book', 'unknown')"),
                "comics" => query.push(" AND b.publication_kind IN ('comic', 'manga')"),
                _ => query
                    .push(" AND b.publication_kind = ")
                    .push_bind(kind.clone()),
            };
        }
        if let Some(format) = &self.format {
            query.push(
                " AND EXISTS (
                    SELECT 1 FROM book_files f2
                    JOIN editions e2 ON e2.id = f2.edition_id
                    WHERE e2.book_id = b.id AND f2.format = ",
            );
            query.push_bind(format.clone()).push(')');
        }
        if let Some(language) = &self.language {
            query.push(" AND b.language = ").push_bind(language.clone());
        }
        if let Some(series) = &self.series {
            query
                .push(
                    " AND (CASE WHEN b.series_id IS NOT NULL
                    THEN (SELECT s.name FROM series s WHERE s.id = b.series_id)
                    WHEN b.series_link_locked = 1 THEN NULL ELSE b.series END) = ",
                )
                .push_bind(series.clone());
        }
        if let Some(subject) = &self.subject {
            query.push(
                " AND EXISTS (
                    SELECT 1 FROM book_subjects bs2
                    JOIN subjects s2 ON s2.id = bs2.subject_id
                    WHERE bs2.book_id = b.id AND s2.normalized_name = ",
            );
            query.push_bind(subject.clone()).push(')');
        }
        if let Some(missing) = &self.missing {
            query.push(" AND ").push(match missing.as_str() {
                "cover" => "b.cover_path IS NULL",
                "description" => {
                    "NOT (b.description IS NOT NULL AND length(trim(b.description)) > 120)"
                }
                _ => "coalesce(trim(b.language), '') = ''",
            });
        }
        if let Some(collection) = self.collection {
            query.push(
                " AND EXISTS (
                    SELECT 1 FROM collection_books cb
                    WHERE cb.book_id = b.id AND cb.collection_id = ",
            );
            query.push_bind(collection).push(')');
        }
        if let Some(letter) = &self.letter {
            // The letter index follows the active sort: books by title, or
            // authors by name when browsing by author.
            if author_sort {
                query.push(
                    " AND EXISTS (
                        SELECT 1 FROM book_authors ba2
                        JOIN authors a2 ON a2.id = ba2.author_id
                        WHERE ba2.book_id = b.id
                          AND substr(a2.normalized_name, 1, 1) = ",
                );
                query.push_bind(letter.clone()).push(')');
            } else {
                query
                    .push(" AND substr(b.normalized_title, 1, 1) = ")
                    .push_bind(letter.clone());
            }
        }
    }
}

pub async fn list_books(
    pool: &SqlitePool,
    sort: &str,
    page: i64,
    page_size: i64,
    filters: &BookFilters,
) -> Result<BookPage, AppError> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let offset = (page - 1) * page_size;

    let order = match sort {
        "title" => "b.normalized_title ASC, b.id ASC",
        "author" => "authors ASC, b.normalized_title ASC",
        _ => "b.created_at DESC, b.id DESC",
    };

    let mut rows_query = QueryBuilder::<Sqlite>::new(BOOK_SELECT);
    filters.push_predicates(&mut rows_query, sort == "author", " WHERE ");
    rows_query.push(" ORDER BY ").push(order).push(" LIMIT ");
    rows_query
        .push_bind(page_size)
        .push(" OFFSET ")
        .push_bind(offset);
    let rows: Vec<BookRow> = rows_query.build_query_as().fetch_all(pool).await?;

    let mut total_query = QueryBuilder::<Sqlite>::new("SELECT count(*) FROM books b");
    filters.push_predicates(&mut total_query, sort == "author", " WHERE ");
    let total: i64 = total_query.build_query_scalar().fetch_one(pool).await?;

    Ok(BookPage {
        items: rows.into_iter().map(BookRow::into_summary).collect(),
        total,
        letters: Vec::new(),
        page,
        page_size,
    })
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FacetValue {
    pub value: String,
    pub count: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubjectFacet {
    pub name: String,
    pub normalized: String,
    pub count: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookFacets {
    pub formats: Vec<FacetValue>,
    pub publication_kinds: Vec<FacetValue>,
    pub languages: Vec<FacetValue>,
    pub series: Vec<FacetValue>,
    pub subjects: Vec<SubjectFacet>,
}

pub async fn books_in_collection_visible(
    pool: &SqlitePool,
    collection_id: i64,
    viewer_id: i64,
) -> Result<Vec<BookSummary>, AppError> {
    let visibility = sharing::predicate("b.id", viewer_id);
    let sql = format!(
        "{BOOK_SELECT}
         WHERE EXISTS (
             SELECT 1 FROM collection_books cb
             WHERE cb.book_id = b.id AND cb.collection_id = ?
         )
         AND {visibility}
         ORDER BY b.normalized_title ASC, b.id ASC"
    );
    let rows: Vec<BookRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(collection_id)
        .fetch_all(pool)
        .await?;

    Ok(rows.into_iter().map(BookRow::into_summary).collect())
}

pub async fn book_facets_visible(
    pool: &SqlitePool,
    mine: Option<i64>,
    viewer_id: i64,
) -> Result<BookFacets, AppError> {
    // Facet counts follow the shelf scope, or they would advertise household
    // totals next to a personal list.
    let scope = format!(
        "AND (? IS NULL OR EXISTS (
             SELECT 1 FROM user_books ub
             WHERE ub.book_id = {{column}} AND ub.user_id = ? AND ub.on_shelf = 1
         )) AND {}",
        sharing::predicate("{column}", viewer_id)
    );

    let formats_sql = format!(
        "SELECT f.format AS value, count(DISTINCT e.book_id) AS count
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE 1=1 {scope}
         GROUP BY f.format
         ORDER BY count DESC, value ASC",
        scope = scope.replace("{column}", "e.book_id")
    );
    let formats: Vec<FacetValue> = sqlx::query_as(sqlx::AssertSqlSafe(formats_sql))
        .bind(mine)
        .bind(mine)
        .fetch_all(pool)
        .await?;

    let kinds_sql = format!(
        "SELECT b.publication_kind AS value, count(*) AS count
         FROM books b WHERE EXISTS (
             SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id
             WHERE e.book_id = b.id
         ) {scope}
         GROUP BY b.publication_kind ORDER BY value",
        scope = scope.replace("{column}", "b.id")
    );
    let publication_kinds: Vec<FacetValue> = sqlx::query_as(sqlx::AssertSqlSafe(kinds_sql))
        .bind(mine)
        .bind(mine)
        .fetch_all(pool)
        .await?;

    let languages_sql = format!(
        "SELECT b.language AS value, count(*) AS count
         FROM books b
         WHERE b.language IS NOT NULL AND b.language != ''
           AND EXISTS (
               SELECT 1 FROM book_files f
               JOIN editions e ON e.id = f.edition_id
               WHERE e.book_id = b.id
           )
           {scope}
         GROUP BY b.language
         ORDER BY count DESC, value ASC",
        scope = scope.replace("{column}", "b.id")
    );
    let languages: Vec<FacetValue> = sqlx::query_as(sqlx::AssertSqlSafe(languages_sql))
        .bind(mine)
        .bind(mine)
        .fetch_all(pool)
        .await?;

    let series_sql = format!(
        "SELECT CASE WHEN b.series_id IS NOT NULL THEN s.name
                     WHEN b.series_link_locked = 1 THEN NULL ELSE b.series END AS value,
                count(*) AS count
         FROM books b
         LEFT JOIN series s ON s.id = b.series_id
         WHERE EXISTS (
               SELECT 1 FROM book_files f
               JOIN editions e ON e.id = f.edition_id
               WHERE e.book_id = b.id
           )
           {scope}
         GROUP BY value
         HAVING value IS NOT NULL AND value != ''
         ORDER BY count DESC, value ASC
         LIMIT 200",
        scope = scope.replace("{column}", "b.id")
    );
    let series: Vec<FacetValue> = sqlx::query_as(sqlx::AssertSqlSafe(series_sql))
        .bind(mine)
        .bind(mine)
        .fetch_all(pool)
        .await?;

    let subjects_sql = format!(
        "SELECT s.name, s.normalized_name, count(*) AS count
         FROM book_subjects bs
         JOIN subjects s ON s.id = bs.subject_id
         WHERE EXISTS (
             SELECT 1 FROM book_files f
             JOIN editions e ON e.id = f.edition_id
             WHERE e.book_id = bs.book_id
         )
         {scope}
         GROUP BY s.id
         ORDER BY count DESC, s.name ASC
         LIMIT 300",
        scope = scope.replace("{column}", "bs.book_id")
    );
    let subject_rows: Vec<(String, String, i64)> =
        sqlx::query_as(sqlx::AssertSqlSafe(subjects_sql))
            .bind(mine)
            .bind(mine)
            .fetch_all(pool)
            .await?;
    let subjects_facets: Vec<SubjectFacet> = subject_rows
        .into_iter()
        .filter(|(_, normalized, _)| subjects::is_displayable(normalized))
        .take(60)
        .map(|(name, normalized, count)| SubjectFacet {
            name,
            normalized,
            count,
        })
        .collect();

    Ok(BookFacets {
        formats,
        publication_kinds,
        languages,
        series,
        subjects: subjects_facets,
    })
}

pub async fn search_books(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
    filters: &BookFilters,
) -> Result<Vec<BookSummary>, AppError> {
    let fts = fts_query(query);
    if fts.is_empty() {
        return Ok(Vec::new());
    }

    // Text search respects the same filters as browsing, so the active
    // filter chips in the Library always describe the results on screen.
    let mut query_builder = QueryBuilder::<Sqlite>::new(BOOK_SELECT);
    query_builder
        .push(" WHERE (b.id IN (SELECT rowid FROM books_fts WHERE books_fts MATCH ")
        .push_bind(fts)
        .push(") OR EXISTS (SELECT 1 FROM series s WHERE s.id = b.series_id AND instr(lower(s.name), lower(")
        .push_bind(query.trim().to_string())
        .push(")) > 0))");
    filters.push_predicates(&mut query_builder, false, " AND ");
    query_builder
        .push(" ORDER BY b.created_at DESC, b.id DESC LIMIT ")
        .push_bind(limit.clamp(1, 100));
    let rows: Vec<BookRow> = query_builder.build_query_as().fetch_all(pool).await?;
    Ok(rows.into_iter().map(BookRow::into_summary).collect())
}

pub async fn recent_books_visible(
    pool: &SqlitePool,
    limit: i64,
    mine: Option<i64>,
    viewer_id: i64,
) -> Result<Vec<BookSummary>, AppError> {
    // Personal recency means when *you* added it to your shelf, not when the
    // household acquired the file.
    let visibility = sharing::predicate("b.id", viewer_id);
    let sql = match mine {
        Some(_) => format!(
            "{BOOK_SELECT}
             JOIN user_books ub ON ub.book_id = b.id AND ub.user_id = ? AND ub.on_shelf = 1
             {OWNED_FILTER} AND {visibility}
             ORDER BY ub.added_at DESC, b.id DESC LIMIT ?"
        ),
        None => {
            format!(
                "{BOOK_SELECT} {OWNED_FILTER} AND {visibility} ORDER BY b.created_at DESC, b.id DESC LIMIT ?"
            )
        }
    };
    let mut query = sqlx::query_as::<_, BookRow>(sqlx::AssertSqlSafe(sql));
    if let Some(mine) = mine {
        query = query.bind(mine);
    }
    let rows = query.bind(limit.clamp(1, 50)).fetch_all(pool).await?;
    Ok(rows.into_iter().map(BookRow::into_summary).collect())
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadingProgress {
    pub book: BookSummary,
    pub percentage: f64,
    pub updated_at: i64,
    pub source: String,
    pub browser_file_id: Option<i64>,
    pub browser_percentage: Option<f64>,
    pub epub_file_id: Option<i64>,
}

/// One unfinished entry per book, from the most recent reader activity.
/// Browser and KOReader locators remain independent.
pub async fn continue_reading(
    pool: &SqlitePool,
    user_id: i64,
    limit: i64,
    shelf_only: bool,
) -> Result<Vec<ReadingProgress>, AppError> {
    let shelf = if shelf_only {
        "AND EXISTS (
             SELECT 1 FROM user_books ub
             WHERE ub.book_id = b.id AND ub.user_id = ? AND ub.on_shelf = 1
         )"
    } else {
        ""
    };
    let select = BOOK_SELECT.replace(
        "FROM books b",
        ", latest.percentage, latest.updated_at,
       latest.source, browser_last.book_file_id AS browser_file_id,
       browser_last.percentage AS browser_percentage,
       (SELECT f.id FROM book_files f JOIN editions e ON e.id = f.edition_id
        WHERE e.book_id = b.id AND f.format = 'epub' ORDER BY f.id LIMIT 1) AS epub_file_id
       FROM books b",
    );
    let visibility = sharing::predicate("b.id", user_id);
    let sql = format!(
        "WITH browser AS (
             SELECT e.book_id, p.book_file_id, p.percentage, p.updated_at,
                    ROW_NUMBER() OVER (
                        PARTITION BY e.book_id ORDER BY p.updated_at DESC, p.revision DESC
                    ) AS rank
             FROM browser_reading_positions p
             JOIN book_files f ON f.id = p.book_file_id AND f.sha256 = p.sha256
             JOIN editions e ON e.id = f.edition_id
             WHERE p.user_id = ? AND p.completed = 0
               AND p.percentage > 0 AND p.percentage < 0.995
         ), activity AS (
             SELECT rp.book_id, rp.percentage, rp.updated_at,
                    rp.source AS source
             FROM reading_progress rp
             WHERE rp.user_id = ? AND rp.book_id IS NOT NULL
               AND rp.source != 'bokhylle'
               AND rp.percentage > 0 AND rp.percentage < 0.995
             UNION ALL
             SELECT book_id, percentage, updated_at, 'bokhylle' AS source
             FROM browser WHERE rank = 1
         ), latest AS (
             SELECT book_id, percentage, updated_at, source,
                    ROW_NUMBER() OVER (
                        PARTITION BY book_id ORDER BY updated_at DESC,
                        CASE source WHEN 'bokhylle' THEN 0 ELSE 1 END
                    ) AS rank
             FROM activity
         )
         {select}
         JOIN latest ON latest.book_id = b.id AND latest.rank = 1
         LEFT JOIN browser browser_last
           ON browser_last.book_id = b.id AND browser_last.rank = 1
         {OWNED_FILTER} AND {visibility}
         {shelf}
         AND NOT EXISTS (
             SELECT 1 FROM user_book_completions c
             WHERE c.user_id = ? AND c.book_id = b.id
         )
         ORDER BY latest.updated_at DESC, b.id DESC LIMIT ?"
    );
    let mut query = sqlx::query_as::<_, ProgressRow>(sqlx::AssertSqlSafe(sql));
    query = query.bind(user_id).bind(user_id);
    if shelf_only {
        query = query.bind(user_id);
    }
    query = query.bind(user_id);
    let rows = query.bind(limit.clamp(1, 50)).fetch_all(pool).await?;
    Ok(rows.into_iter().map(ProgressRow::into_progress).collect())
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissingFile {
    pub book_id: i64,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryHealth {
    pub books: i64,
    pub files: i64,
    pub missing_covers: i64,
    pub missing_descriptions: i64,
    pub missing_languages: i64,
    pub missing_files: i64,
    pub missing_file_samples: Vec<MissingFile>,
}

#[derive(FromRow)]
struct HealthCounts {
    books: i64,
    files: i64,
    missing_covers: i64,
    missing_descriptions: i64,
    missing_languages: i64,
}

/// Actionable gaps rather than a score: what an admin could fix.
pub async fn library_health(pool: &SqlitePool) -> Result<LibraryHealth, AppError> {
    let counts: HealthCounts = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) AS books,
                (SELECT count(*) FROM book_files) AS files,
                coalesce(sum(CASE WHEN has_cover = 0 THEN 1 ELSE 0 END), 0) AS missing_covers,
                coalesce(sum(CASE WHEN has_description = 0 THEN 1 ELSE 0 END), 0) AS missing_descriptions,
                coalesce(sum(CASE WHEN language IS NULL OR trim(language) = '' THEN 1 ELSE 0 END), 0) AS missing_languages
         FROM ({BOOK_SELECT} {OWNED_FILTER})"
    )))
    .fetch_one(pool)
    .await?;

    let files: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT b.id, b.title, f.path
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         JOIN books b ON b.id = e.book_id
         ORDER BY b.normalized_title COLLATE NOCASE, f.id",
    )
    .fetch_all(pool)
    .await?;
    let missing = tokio::task::spawn_blocking(move || {
        files
            .into_iter()
            .filter(|(_, _, path)| !std::path::Path::new(path).exists())
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))?;
    let samples = missing
        .iter()
        .take(10)
        .map(|(book_id, title, path)| MissingFile {
            book_id: *book_id,
            title: title.clone(),
            path: path.clone(),
        })
        .collect();
    Ok(LibraryHealth {
        books: counts.books,
        files: counts.files,
        missing_covers: counts.missing_covers,
        missing_descriptions: counts.missing_descriptions,
        missing_languages: counts.missing_languages,
        missing_files: missing.len() as i64,
        missing_file_samples: samples,
    })
}

/// Deterministic, seeded sampling: the same user and day produce the same
/// highlights, and a changed day reshuffles modestly. Never `RANDOM()`, so
/// Home does not look arbitrary between refreshes.
pub async fn highlight_books_visible(
    pool: &SqlitePool,
    limit: i64,
    mine: Option<i64>,
    seed: i64,
    viewer_id: i64,
) -> Result<Vec<BookSummary>, AppError> {
    let visibility = sharing::predicate("b.id", viewer_id);
    let sql = match mine {
        Some(_) => format!(
            "{BOOK_SELECT}
             JOIN user_books ub ON ub.book_id = b.id AND ub.user_id = ? AND ub.on_shelf = 1
             {OWNED_FILTER} AND {visibility}
             ORDER BY ((b.id * 1103515245 + ?) % 2147483647) LIMIT ?"
        ),
        None => format!(
            "{BOOK_SELECT} {OWNED_FILTER} AND {visibility}
             ORDER BY ((b.id * 1103515245 + ?) % 2147483647) LIMIT ?"
        ),
    };
    let mut query = sqlx::query_as::<_, BookRow>(sqlx::AssertSqlSafe(sql));
    if let Some(mine) = mine {
        query = query.bind(mine);
    }
    query = query.bind(seed.abs());
    let rows = query.bind(limit.clamp(1, 50)).fetch_all(pool).await?;
    Ok(rows.into_iter().map(BookRow::into_summary).collect())
}

pub async fn get_book(pool: &SqlitePool, id: i64) -> Result<Option<BookDetail>, AppError> {
    let sql = format!("{BOOK_SELECT} WHERE b.id = ?");
    let row: Option<BookRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };

    let (description, reading_direction, legacy_series_text, sharing_managed): (
        Option<String>,
        Option<String>,
        Option<String>,
        bool,
    ) = sqlx::query_as(
        "SELECT description, reading_direction, series, sharing_managed FROM books WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    let edition_rows: Vec<EditionRow> = sqlx::query_as(
        "SELECT id, title, language, publication_year, isbn10, isbn13, publisher, is_unknown,
                provider, provider_key
             FROM editions WHERE book_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let file_rows: Vec<(i64, i64, String, i64, String)> = sqlx::query_as(
        "SELECT f.id, f.edition_id, f.format, f.size, f.path
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ? ORDER BY f.id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let files: Vec<FileDetail> = file_rows
        .into_iter()
        .map(|(id, edition_id, format, size, path)| FileDetail {
            id,
            edition_id,
            format,
            size,
            filename: std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or(path),
        })
        .collect();

    let mut editions = Vec::new();
    for row in edition_rows {
        let metadata_sources = metadata_fields::sources(pool, Scope::Edition(row.id)).await?;
        let manual_year = metadata_sources
            .iter()
            .any(|source| source.field == "publicationYear" && source.manual);
        let manual_language = metadata_sources
            .iter()
            .any(|source| source.field == "language" && source.manual);
        // A catalogue work is not a verified edition; explicit corrections
        // are still meaningful when there is no local file.
        let unverified_work = row.provider.as_deref() == Some("openlibrary")
            && row
                .provider_key
                .as_deref()
                .is_some_and(|key| key.starts_with("/works/"))
            && !files.iter().any(|file| file.edition_id == row.id);
        editions.push(EditionDetail {
            id: row.id,
            title: row.title,
            language: (!unverified_work || manual_language)
                .then_some(row.language)
                .flatten(),
            publication_year: (!unverified_work || manual_year)
                .then_some(row.publication_year)
                .flatten(),
            isbn10: (!unverified_work).then_some(row.isbn10).flatten(),
            isbn13: (!unverified_work).then_some(row.isbn13).flatten(),
            publisher: row.publisher,
            unknown: unverified_work || row.is_unknown != 0,
            metadata_sources,
        });
    }

    let available_languages: Vec<String> = sqlx::query_scalar(
        "SELECT language FROM book_available_languages WHERE book_id = ? ORDER BY language",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let publication_year = editions
        .iter()
        .filter_map(|edition| edition.publication_year)
        .min();

    let subject_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT s.name, s.normalized_name
         FROM book_subjects bs
         JOIN subjects s ON s.id = bs.subject_id
         WHERE bs.book_id = ?
         ORDER BY bs.position, s.name",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    let subjects_list: Vec<SubjectName> = subject_rows
        .into_iter()
        .filter(|(_, normalized)| subjects::is_displayable(normalized))
        .map(|(name, normalized)| SubjectName { name, normalized })
        .collect();

    let author_refs: Vec<AuthorRef> = sqlx::query_as::<_, (i64, String)>(
        "SELECT a.id, a.name
         FROM book_authors ba
         JOIN authors a ON a.id = ba.author_id
         WHERE ba.book_id = ?
         ORDER BY ba.position, a.name",
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(id, name)| AuthorRef { id, name })
    .collect();

    let summary = row.into_summary();

    let metadata_sources = metadata_fields::sources(pool, Scope::Book(id)).await?;
    let manual_language = metadata_sources
        .iter()
        .any(|source| source.field == "language" && source.manual);
    let language = if manual_language {
        summary.language.clone()
    } else if let Some(file) = files.first() {
        editions
            .iter()
            .find(|edition| edition.id == file.edition_id)
            .and_then(|edition| edition.language.clone())
    } else if editions.iter().all(|edition| edition.unknown) && !available_languages.is_empty() {
        None
    } else {
        summary.language
    };

    Ok(Some(BookDetail {
        metadata_sources,
        id: summary.id,
        title: summary.title,
        authors: summary.authors,
        author_refs,
        language,
        available_languages,
        series: summary.series,
        legacy_series_text,
        series_number: summary.series_number,
        series_id: summary.series_id,
        series_sort_order: summary.series_sort_order,
        publication_kind: summary.publication_kind,
        reading_direction,
        has_cover: summary.has_cover,
        added_at: summary.added_at,
        rating: summary.rating,
        rating_count: summary.rating_count,
        rating_source: summary.rating_source,
        description,
        publication_year,
        sharing: None,
        sharing_managed,
        shared_in_household: true,
        on_shelf: false,
        preference: None,
        browser_file_id: None,
        subjects: subjects_list,
        editions,
        files,
    }))
}

/// The canonical existing-file choice, shared by any surface that needs to
/// pick among a book's files. The order matches the release evaluator: format
/// preference first (EPUB before the PDF fallback, unless a format is
/// preferred), then the preferred language order, then file id.
/// Like `preferred_existing_file`, but when languages are configured it only
/// returns a file whose edition language is explicitly allowed. Automation
/// uses this so a wrong-language local copy is never silently sent.
pub async fn existing_file_in_languages(
    pool: &SqlitePool,
    book_id: i64,
    preferred_format: Option<&str>,
    preferred_languages: &[String],
) -> Result<Option<i64>, AppError> {
    if preferred_languages.is_empty() {
        return preferred_existing_file(pool, book_id, preferred_format, preferred_languages).await;
    }
    let language_list = format!(",{},", preferred_languages.join(","));
    let format = preferred_format.unwrap_or("any").to_ascii_lowercase();
    Ok(sqlx::query_scalar(
        "SELECT f.id
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ?
           AND instr(?, ',' || COALESCE(e.language, '') || ',') > 0
         ORDER BY
           CASE
             WHEN ? IN ('epub', 'pdf') THEN CASE WHEN f.format = ? THEN 0 ELSE 1 END
             ELSE CASE f.format WHEN 'epub' THEN 0 ELSE 1 END
           END,
           instr(?, ',' || COALESCE(e.language, '') || ','),
           f.id
         LIMIT 1",
    )
    .bind(book_id)
    .bind(&language_list)
    .bind(&format)
    .bind(&format)
    .bind(&language_list)
    .fetch_optional(pool)
    .await?)
}

pub async fn preferred_existing_file(
    pool: &SqlitePool,
    book_id: i64,
    preferred_format: Option<&str>,
    preferred_languages: &[String],
) -> Result<Option<i64>, AppError> {
    let language_list = if preferred_languages.is_empty() {
        String::new()
    } else {
        format!(",{},", preferred_languages.join(","))
    };
    let format = preferred_format.unwrap_or("any").to_ascii_lowercase();

    Ok(sqlx::query_scalar(
        "SELECT f.id
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ?
         ORDER BY
           CASE
             WHEN ? IN ('epub', 'pdf') THEN CASE WHEN f.format = ? THEN 0 ELSE 1 END
             ELSE CASE f.format WHEN 'epub' THEN 0 ELSE 1 END
           END,
           CASE
             WHEN ? <> '' AND instr(?, ',' || COALESCE(e.language, '') || ',') > 0
             THEN instr(?, ',' || COALESCE(e.language, '') || ',')
             ELSE 999
           END,
           f.id
         LIMIT 1",
    )
    .bind(book_id)
    .bind(&format)
    .bind(&format)
    .bind(&language_list)
    .bind(&language_list)
    .bind(&language_list)
    .fetch_optional(pool)
    .await?)
}

/// Which letters actually have results under the current sort and filters,
/// ignoring the active letter itself so the index never disables the
/// current selection.
pub async fn book_letters(
    pool: &SqlitePool,
    sort: &str,
    filters: &BookFilters,
) -> Result<Vec<String>, AppError> {
    let author_sort = sort == "author";
    let mut filters = filters.clone();
    filters.letter = None;
    let mut query = if author_sort {
        QueryBuilder::<Sqlite>::new(
            "SELECT DISTINCT substr(a2.normalized_name, 1, 1) AS letter
             FROM books b
             JOIN book_authors ba2 ON ba2.book_id = b.id
             JOIN authors a2 ON a2.id = ba2.author_id",
        )
    } else {
        QueryBuilder::<Sqlite>::new(
            "SELECT DISTINCT substr(b.normalized_title, 1, 1) AS letter FROM books b",
        )
    };
    filters.push_predicates(&mut query, author_sort, " WHERE ");
    query.push(" ORDER BY letter");
    let letters: Vec<String> = query.build_query_scalar().fetch_all(pool).await?;
    Ok(letters
        .into_iter()
        .filter(|letter| !letter.is_empty())
        .map(|letter| letter.to_lowercase())
        .collect())
}

pub async fn list_authors(
    pool: &SqlitePool,
    mine: Option<i64>,
    following_only: bool,
    user_id: i64,
) -> Result<Vec<AuthorSummary>, AppError> {
    let visibility = sharing::predicate("ba.book_id", user_id);
    let rows: Vec<(i64, String, i64, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT a.id, a.name,
                (SELECT count(*) FROM author_follows af
                 WHERE af.author_id = a.id AND af.user_id = ?) AS following,
                COALESCE((SELECT af.auto_acquire FROM author_follows af
                 WHERE af.author_id = a.id AND af.user_id = ?), 0) AS auto_acquire,
                count(DISTINCT CASE WHEN EXISTS (
                    SELECT 1 FROM book_files f
                    JOIN editions e ON e.id = f.edition_id
                    WHERE e.book_id = ba.book_id
                ) AND (
                    ? IS NULL OR EXISTS (
                        SELECT 1 FROM user_books ub
                        WHERE ub.book_id = ba.book_id AND ub.user_id = ? AND ub.on_shelf = 1
                    )
                ) AND {visibility} THEN ba.book_id END)
         FROM authors a
         LEFT JOIN book_authors ba ON ba.author_id = a.id
         WHERE ? = 0 OR EXISTS (
             SELECT 1 FROM author_follows af
             WHERE af.author_id = a.id AND af.user_id = ?
         )
         GROUP BY a.id
         ORDER BY a.name COLLATE NOCASE"
    )))
    .bind(user_id)
    .bind(user_id)
    .bind(mine)
    .bind(mine)
    .bind(if following_only { 1 } else { 0 })
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        // Followed authors stay manageable even with zero local books;
        // everything else still needs at least one readable book.
        .filter(|(_, _, following, _, book_count)| *following > 0 || *book_count > 0)
        .map(
            |(id, name, following, auto_acquire, book_count)| AuthorSummary {
                id,
                name,
                book_count,
                following: following > 0,
                auto_acquire: auto_acquire > 0,
            },
        )
        .collect())
}

pub async fn get_author_visible(
    pool: &SqlitePool,
    id: i64,
    mine: Option<i64>,
    viewer_id: i64,
) -> Result<Option<AuthorDetail>, AppError> {
    let author_visibility = sharing::author_predicate("authors.id", viewer_id);
    let author: Option<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT id, name FROM authors WHERE id = ? AND {author_visibility}"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await?;

    let Some((id, name)) = author else {
        return Ok(None);
    };

    let visibility = sharing::predicate("b.id", viewer_id);
    let sql = format!(
        "{BOOK_SELECT}
         JOIN book_authors ba ON ba.book_id = b.id
         WHERE ba.author_id = ? AND {visibility}
           AND EXISTS (
               SELECT 1 FROM book_files f
               JOIN editions e ON e.id = f.edition_id
               WHERE e.book_id = b.id
           )
           AND (
               ? IS NULL OR EXISTS (
                   SELECT 1 FROM user_books ub
                   WHERE ub.book_id = b.id AND ub.user_id = ? AND ub.on_shelf = 1
               )
           )
         ORDER BY b.normalized_title ASC, b.id ASC"
    );
    let rows: Vec<BookRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(mine)
        .bind(mine)
        .fetch_all(pool)
        .await?;

    Ok(Some(AuthorDetail {
        id,
        name,
        books: rows.into_iter().map(BookRow::into_summary).collect(),
    }))
}

pub struct DownloadTarget {
    pub path: String,
    pub title: String,
    pub authors: Vec<String>,
    pub format: String,
}

pub async fn get_download_target(
    pool: &SqlitePool,
    book_id: i64,
    file_id: i64,
) -> Result<Option<DownloadTarget>, AppError> {
    let row: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT f.path, b.title, COALESCE((
             SELECT group_concat(a.name, ', ')
             FROM book_authors ba JOIN authors a ON a.id = ba.author_id WHERE ba.book_id = b.id
         ), ''), f.format
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         JOIN books b ON b.id = e.book_id
         WHERE f.id = ? AND b.id = ?",
    )
    .bind(file_id)
    .bind(book_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(path, title, authors, format)| DownloadTarget {
        path,
        title,
        authors: split_authors(&authors),
        format,
    }))
}

pub struct CoverTarget {
    pub isbn: Option<String>,
    pub cover_path: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
}

pub async fn get_cover_target(
    pool: &SqlitePool,
    book_id: i64,
) -> Result<Option<CoverTarget>, AppError> {
    type CoverRow = (
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
    );

    let row: Option<CoverRow> = sqlx::query_as(
            "SELECT b.cover_path, b.title, COALESCE((
                 SELECT group_concat(a.name, ', ')
                 FROM book_authors ba JOIN authors a ON a.id = ba.author_id WHERE ba.book_id = b.id
             ), ''), (
                 SELECT e.isbn13 FROM editions e WHERE e.book_id = b.id AND e.isbn13 IS NOT NULL LIMIT 1
             ), (
                 SELECT e.isbn10 FROM editions e WHERE e.book_id = b.id AND e.isbn10 IS NOT NULL LIMIT 1
             )
             FROM books b WHERE b.id = ?",
        )
        .bind(book_id)
        .fetch_optional(pool)
        .await?;

    Ok(
        row.map(|(cover_path, title, authors, isbn13, isbn10)| CoverTarget {
            cover_path,
            title,
            authors: split_authors(&authors),
            isbn: isbn13.or(isbn10),
        }),
    )
}

pub fn fts_query(input: &str) -> String {
    fts_query_in(input, None)
}

/// Column-scoped FTS query: `Title` matches the title column only, `Author`
/// the author column only, `None` both. This keeps local result semantics in
/// step with the provider's type selection.
pub fn fts_query_in(input: &str, column: Option<&str>) -> String {
    let tokens: Vec<String> = input
        .split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|character| character.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|token| !token.is_empty())
        .map(|token| match column {
            Some(column) => format!("{column}:\"{token}\"*"),
            None => format!("\"{token}\"*"),
        })
        .collect();

    tokens.join(" ")
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SimilarBook {
    pub book: BookSummary,
    pub shared_subjects: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelatedBooks {
    pub series: Vec<BookSummary>,
    pub author: Vec<BookSummary>,
    pub similar: Vec<SimilarBook>,
}

struct SimilarCandidate {
    score: i64,
    shared: Vec<(u8, String)>,
    language: Option<String>,
    year: Option<i64>,
}

#[derive(FromRow)]
struct RelatedSourceRow {
    language: Option<String>,
    series: Option<String>,
    series_id: Option<i64>,
    series_link_locked: i64,
}

pub async fn related_books_visible(
    pool: &SqlitePool,
    id: i64,
    viewer_id: i64,
) -> Result<RelatedBooks, AppError> {
    let visibility = sharing::predicate("b.id", viewer_id);
    let source: Option<RelatedSourceRow> = sqlx::query_as(
        "SELECT language, series, series_id, series_link_locked FROM books WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some(source) = source else {
        return Ok(RelatedBooks {
            series: Vec::new(),
            author: Vec::new(),
            similar: Vec::new(),
        });
    };

    let language = source.language;
    let series_id = source.series_id;
    let series = if source.series_link_locked == 1 && series_id.is_none() {
        None
    } else {
        source.series
    };
    let series_rail: Vec<BookSummary> = if series_id.is_some() || series.is_some() {
        let match_column = if series_id.is_some() {
            "b.series_id"
        } else {
            "b.series"
        };
        let legacy_filter = if series_id.is_some() {
            ""
        } else {
            "AND b.series_link_locked = 0"
        };
        let sql = format!(
            "{BOOK_SELECT}
             WHERE {match_column} = ? AND b.id != ?
               {legacy_filter}
               AND {visibility}
               AND EXISTS (
                   SELECT 1 FROM book_files f
                   JOIN editions e ON e.id = f.edition_id
                   WHERE e.book_id = b.id
               )
             ORDER BY b.series_sort_order IS NULL, b.series_sort_order,
                      b.normalized_title ASC
             LIMIT 12"
        );
        let mut query = sqlx::query_as::<_, BookRow>(sqlx::AssertSqlSafe(sql));
        query = if let Some(series_id) = series_id {
            query.bind(series_id)
        } else {
            query.bind(series.as_deref().unwrap_or_default().to_string())
        };
        let rows = query.bind(id).fetch_all(pool).await?;
        rows.into_iter().map(BookRow::into_summary).collect()
    } else {
        Vec::new()
    };
    let series_ids: Vec<i64> = series_rail.iter().map(|book| book.id).collect();

    let author_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT author_id FROM book_authors WHERE book_id = ? ORDER BY position",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let mut author_rail: Vec<BookSummary> = Vec::new();
    if !author_ids.is_empty() {
        let author_placeholders = placeholders(author_ids.len());
        let sql = format!(
            "{BOOK_SELECT}
             WHERE b.id != ?
               AND EXISTS (
                   SELECT 1 FROM book_authors ba
                   WHERE ba.book_id = b.id AND ba.author_id IN ({author_placeholders})
               )
               AND {visibility}
               AND EXISTS (
                   SELECT 1 FROM book_files f
                   JOIN editions e ON e.id = f.edition_id
                   WHERE e.book_id = b.id
               )
             ORDER BY b.created_at DESC, b.id DESC
             LIMIT 24"
        );
        let mut query = sqlx::query_as::<_, BookRow>(sqlx::AssertSqlSafe(sql)).bind(id);
        for author_id in &author_ids {
            query = query.bind(author_id);
        }
        let rows = query.fetch_all(pool).await?;
        author_rail = rows
            .into_iter()
            .map(BookRow::into_summary)
            .filter(|book| !series_ids.contains(&book.id))
            .take(12)
            .collect();
    }

    let source_subjects: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT s.id, s.name, s.normalized_name
         FROM book_subjects bs
         JOIN subjects s ON s.id = bs.subject_id
         WHERE bs.book_id = ?
         ORDER BY bs.position",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let mut similar: Vec<SimilarBook> = Vec::new();
    if !source_subjects.is_empty() {
        let source_year: Option<i64> =
            sqlx::query_scalar("SELECT MIN(publication_year) FROM editions WHERE book_id = ?")
                .bind(id)
                .fetch_one(pool)
                .await?;

        let subject_ids: Vec<i64> = source_subjects
            .iter()
            .map(|(subject_id, _, _)| *subject_id)
            .collect();
        let subject_placeholders = placeholders(subject_ids.len());
        let author_clause = if author_ids.is_empty() {
            String::new()
        } else {
            format!(
                "AND NOT EXISTS (
                     SELECT 1 FROM book_authors ba2
                     WHERE ba2.book_id = b.id
                       AND ba2.author_id IN ({})
                 )",
                placeholders(author_ids.len())
            )
        };

        let sql = format!(
            "SELECT bs.book_id, s.name, s.normalized_name, b.language, b.series,
                    (SELECT MIN(publication_year) FROM editions e WHERE e.book_id = bs.book_id) AS year
             FROM book_subjects bs
             JOIN subjects s ON s.id = bs.subject_id
             JOIN books b ON b.id = bs.book_id
             WHERE bs.subject_id IN ({subject_placeholders})
               AND bs.book_id != ?
               AND {visibility}
               AND EXISTS (
                   SELECT 1 FROM book_files f
                   JOIN editions e2 ON e2.id = f.edition_id
                   WHERE e2.book_id = b.id
               )
               {author_clause}"
        );
        let mut query = sqlx::query_as::<
            _,
            (
                i64,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<i64>,
            ),
        >(sqlx::AssertSqlSafe(sql));
        for subject_id in &subject_ids {
            query = query.bind(subject_id);
        }
        query = query.bind(id);
        for author_id in &author_ids {
            query = query.bind(author_id);
        }
        let rows = query.fetch_all(pool).await?;

        let mut candidates: HashMap<i64, SimilarCandidate> = HashMap::new();
        for (book_id, name, normalized, book_language, book_series, year) in rows {
            if book_series.is_some() && book_series == series {
                continue;
            }
            let Some(weight) = subjects::similarity_weight(&normalized) else {
                continue;
            };
            let candidate = candidates
                .entry(book_id)
                .or_insert_with(|| SimilarCandidate {
                    score: 0,
                    shared: Vec::new(),
                    language: book_language,
                    year,
                });
            if !candidate
                .shared
                .iter()
                .any(|(_, existing)| existing == &name)
            {
                candidate.score += weight as i64 * 2;
                candidate.shared.push((weight, name));
            }
        }

        for candidate in candidates.values_mut() {
            if candidate.language.is_some() && candidate.language == language {
                candidate.score += 1;
            }
            if let (Some(source_year), Some(year)) = (source_year, candidate.year)
                && (source_year - year).abs() <= 10
            {
                candidate.score += 1;
            }
        }

        let mut ranked: Vec<(i64, i64, Vec<String>)> = candidates
            .into_iter()
            .map(|(book_id, mut candidate)| {
                candidate
                    .shared
                    .sort_by_key(|left| std::cmp::Reverse(left.0));
                let shared = candidate
                    .shared
                    .into_iter()
                    .map(|(_, name)| name)
                    .take(3)
                    .collect();
                (book_id, candidate.score, shared)
            })
            .collect();
        ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        ranked.truncate(8);

        let ids: Vec<i64> = ranked.iter().map(|(book_id, _, _)| *book_id).collect();
        let by_id: HashMap<i64, BookSummary> = books_by_ids(pool, &ids, viewer_id)
            .await?
            .into_iter()
            .map(|book| (book.id, book))
            .collect();
        similar = ranked
            .into_iter()
            .filter_map(|(book_id, _, shared_subjects)| {
                by_id.get(&book_id).cloned().map(|book| SimilarBook {
                    book,
                    shared_subjects,
                })
            })
            .collect();
    }

    Ok(RelatedBooks {
        series: series_rail,
        author: author_rail,
        similar,
    })
}

async fn books_by_ids(
    pool: &SqlitePool,
    ids: &[i64],
    viewer_id: i64,
) -> Result<Vec<BookSummary>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let visibility = sharing::predicate("b.id", viewer_id);
    let sql = format!(
        "{BOOK_SELECT} WHERE b.id IN ({}) AND {visibility}",
        placeholders(ids.len())
    );
    let mut query = sqlx::query_as::<_, BookRow>(sqlx::AssertSqlSafe(sql));
    for id in ids {
        query = query.bind(id);
    }
    let rows = query.fetch_all(pool).await?;
    let by_id: HashMap<i64, BookSummary> = rows
        .into_iter()
        .map(BookRow::into_summary)
        .map(|book| (book.id, book))
        .collect();
    Ok(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect())
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HomeRail {
    pub key: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub subject: Option<String>,
    pub books: Vec<BookSummary>,
}

/// Home rails: subjects ranked for the signed-in user. The household shelf
/// provides the candidate pool, but each user's own requests and deliveries
/// lift their interests, and hidden subjects are dropped for that user only.
pub async fn home_rails(pool: &SqlitePool, user_id: i64) -> Result<Vec<HomeRail>, AppError> {
    let mut rails: Vec<HomeRail> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let hidden = hidden_subjects(pool, user_id).await?;

    let visibility = sharing::predicate("b.id", user_id);
    let personal = relevance::personal();
    let exclusions = relevance::EXCLUSIONS;
    let candidates: Vec<(String, String, i64, i64, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "WITH viewer(id) AS (SELECT ?)
         SELECT s.name, s.normalized_name, count(DISTINCT bs.book_id) AS owned,
                (SELECT count(DISTINCT a.id) FROM acquisition_requests ar
                 JOIN acquisitions a ON a.id = ar.acquisition_id
                 JOIN book_subjects other ON other.book_id = a.book_id
                 WHERE ar.user_id = (SELECT id FROM viewer) AND other.subject_id = s.id) AS requested,
                (SELECT count(DISTINCT d.book_id) FROM deliveries d
                 JOIN book_subjects other ON other.book_id = d.book_id
                 WHERE d.user_id = (SELECT id FROM viewer) AND d.status = 'SENT'
                   AND other.subject_id = s.id) AS delivered,
                (SELECT count(DISTINCT ub.book_id) FROM user_books ub
                 JOIN book_subjects other ON other.book_id = ub.book_id
                 WHERE ub.user_id = (SELECT id FROM viewer) AND ub.preference = 'liked'
                   AND other.subject_id = s.id) AS liked
         FROM book_subjects bs JOIN subjects s ON s.id = bs.subject_id
         JOIN books b ON b.id = bs.book_id
         WHERE EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id
                       WHERE e.book_id = b.id)
           AND {visibility} {personal} {exclusions}
         GROUP BY s.id"
    )))
    .bind(user_id).fetch_all(pool).await?;

    let mut ranked: Vec<(String, String, i64, i64, i64, i64)> = candidates
        .into_iter()
        // A rail needs enough books to be browsable; household abundance is
        // viability, never affinity.
        .filter(|(_, normalized, owned, _, _, _)| {
            *owned >= 3
                && !hidden.contains(normalized)
                && subjects::similarity_weight(normalized).is_some()
        })
        .collect();
    ranked.sort_by(|left, right| {
        let left_score = 5 * left.5 + 3 * left.3 + left.4;
        let right_score = 5 * right.5 + 3 * right.3 + right.4;
        right_score
            .cmp(&left_score)
            .then_with(|| right.5.cmp(&left.5))
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.cmp(&right.0))
    });

    for (name, normalized, _owned, _requested, _delivered, _liked) in ranked {
        if rails.len() >= 3 {
            break;
        }
        let books = books_for_subject(pool, user_id, &normalized, 12).await?;
        if books.len() < 3 {
            continue;
        }
        used.push(normalized.clone());
        rails.push(HomeRail {
            key: format!("shelf-{normalized}"),
            title: name,
            subtitle: Some("Matches your interests".to_string()),
            subject: Some(normalized.clone()),
            books,
        });
    }

    let requested_subjects: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT s.name, s.normalized_name, count(DISTINCT a.id) AS count
         FROM acquisition_requests ar
         JOIN acquisitions a ON a.id = ar.acquisition_id
         JOIN book_subjects bs ON bs.book_id = a.book_id
         JOIN subjects s ON s.id = bs.subject_id
         WHERE ar.user_id = ?
           AND EXISTS (
               SELECT 1 FROM book_files f
               JOIN editions e ON e.id = f.edition_id
               WHERE e.book_id = a.book_id
           )
         GROUP BY s.id
         HAVING count >= 2
         ORDER BY count DESC, s.name ASC
         LIMIT 10",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    for (name, normalized, _) in requested_subjects {
        if rails.len() >= 5 {
            break;
        }
        if used.contains(&normalized)
            || hidden.contains(&normalized)
            || subjects::similarity_weight(&normalized).is_none()
        {
            continue;
        }
        let books = books_for_subject(pool, user_id, &normalized, 12).await?;
        if books.len() < 3 {
            continue;
        }
        used.push(normalized.clone());
        rails.push(HomeRail {
            key: format!("for-you-{normalized}"),
            title: format!("Because you requested {name}"),
            subtitle: None,
            subject: Some(normalized.clone()),
            books,
        });
    }

    // "Because you liked X": aggregate over every liked book, not just the
    // most recent one, while still requiring two shared subjects so the rail
    // stays personal. Liked and not-for-me books are excluded.
    // The aggregate liked rail is the strongest personal signal; it may add
    // one rail beyond the subject cap but never replaces one.
    if rails.len() < 4
        && let Some((_subject_name, normalized)) = sqlx::query_as::<_, (String, String)>(
            "SELECT s.name, s.normalized_name
             FROM user_books ub
             JOIN book_subjects bs ON bs.book_id = ub.book_id
             JOIN subjects s ON s.id = bs.subject_id
             WHERE ub.user_id = ? AND ub.preference = 'liked'
               AND NOT EXISTS (
                   SELECT 1 FROM user_subject_prefs usp
                   WHERE usp.user_id = ub.user_id
                     AND usp.normalized_name = s.normalized_name
                     AND usp.hidden = 1
               )
             GROUP BY s.id
             ORDER BY count(*) DESC, s.name
             LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?
    {
        let sql = format!(
            "WITH viewer(id) AS (SELECT ?) {BOOK_SELECT}
             WHERE EXISTS (
                   SELECT 1 FROM book_files f
                   JOIN editions e ON e.id = f.edition_id
                   WHERE e.book_id = b.id
               )
               AND NOT EXISTS (
                   SELECT 1 FROM user_books ub
                   WHERE ub.book_id = b.id AND ub.user_id = ?
                     AND ub.preference IN ('liked', 'not_for_me')
               )
               AND (
                   SELECT count(DISTINCT mine.subject_id)
                   FROM book_subjects mine
                   WHERE mine.book_id = b.id
                     AND mine.subject_id IN (
                         SELECT bs2.subject_id
                         FROM user_books ub2
                         JOIN book_subjects bs2 ON bs2.book_id = ub2.book_id
                         WHERE ub2.user_id = ? AND ub2.preference = 'liked'
                     )
               ) >= 2
             AND {visibility} {personal} {exclusions}
             ORDER BY b.created_at DESC, b.id DESC
             LIMIT 12"
        );
        let rows: Vec<BookRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(user_id)
            .bind(user_id)
            .bind(user_id)
            .fetch_all(pool)
            .await?;
        let books: Vec<BookSummary> = rows.into_iter().map(BookRow::into_summary).collect();
        if books.len() >= 3 {
            rails.push(HomeRail {
                key: format!("liked-{normalized}"),
                title: "Based on books you liked".to_string(),
                subtitle: None,
                subject: None,
                books,
            });
        }
    }

    Ok(rails)
}

pub async fn hidden_subjects(pool: &SqlitePool, user_id: i64) -> Result<Vec<String>, AppError> {
    Ok(sqlx::query_scalar(
        "SELECT normalized_name FROM user_subject_prefs
         WHERE user_id = ? AND hidden = 1
         ORDER BY normalized_name",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

pub async fn set_subject_hidden(
    pool: &SqlitePool,
    user_id: i64,
    normalized: &str,
    hidden: bool,
) -> Result<(), AppError> {
    if hidden {
        sqlx::query(
            "INSERT INTO user_subject_prefs (user_id, normalized_name, hidden)
             VALUES (?, ?, 1)
             ON CONFLICT(user_id, normalized_name)
             DO UPDATE SET hidden = 1, updated_at = unixepoch()",
        )
        .bind(user_id)
        .bind(normalized)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("DELETE FROM user_subject_prefs WHERE user_id = ? AND normalized_name = ?")
            .bind(user_id)
            .bind(normalized)
            .execute(pool)
            .await?;
    }
    Ok(())
}

async fn books_for_subject(
    pool: &SqlitePool,
    user_id: i64,
    normalized: &str,
    limit: i64,
) -> Result<Vec<BookSummary>, AppError> {
    let visibility = sharing::predicate("b.id", user_id);
    let personal = relevance::personal();
    let exclusions = relevance::EXCLUSIONS;
    let sql = format!(
        "WITH viewer(id) AS (SELECT ?) {BOOK_SELECT}
         WHERE EXISTS (
             SELECT 1 FROM book_subjects bs
             JOIN subjects s ON s.id = bs.subject_id
             WHERE bs.book_id = b.id AND s.normalized_name = ?
         )
           AND EXISTS (
               SELECT 1 FROM book_files f
               JOIN editions e ON e.id = f.edition_id
               WHERE e.book_id = b.id
           )
           AND {visibility} {personal} {exclusions}
         ORDER BY b.created_at DESC, b.id DESC
         LIMIT ?"
    );
    let rows: Vec<BookRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .bind(normalized)
        .bind(limit.clamp(1, 24))
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(BookRow::into_summary).collect())
}

/// A child's Home: only what is on their own shelf, no household browsing.
pub async fn shelf_rails(pool: &SqlitePool, user_id: i64) -> Result<Vec<HomeRail>, AppError> {
    let sql = format!(
        "{BOOK_SELECT}
         JOIN user_books ub ON ub.book_id = b.id AND ub.user_id = ? AND ub.on_shelf = 1
         WHERE EXISTS (
             SELECT 1 FROM book_files f
             JOIN editions e ON e.id = f.edition_id
             WHERE e.book_id = b.id
         )
         ORDER BY ub.added_at DESC, b.id DESC
         LIMIT 24"
    );
    let rows: Vec<BookRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    let books: Vec<BookSummary> = rows.into_iter().map(BookRow::into_summary).collect();
    if books.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![HomeRail {
        key: "my-shelf".to_string(),
        title: "My shelf".to_string(),
        subtitle: None,
        subject: None,
        books,
    }])
}

// Viewer-free consumers can see only shared books.
pub async fn comic_shelf(
    pool: &SqlitePool,
    mine: Option<i64>,
    sort: &str,
    page: i64,
    page_size: i64,
) -> Result<ComicShelfPage, AppError> {
    comic_shelf_visible(pool, mine, mine.unwrap_or(-1), sort, page, page_size).await
}
pub async fn books_in_collection(pool: &SqlitePool, id: i64) -> Result<Vec<BookSummary>, AppError> {
    books_in_collection_visible(pool, id, -1).await
}
pub async fn book_facets(pool: &SqlitePool, mine: Option<i64>) -> Result<BookFacets, AppError> {
    book_facets_visible(pool, mine, mine.unwrap_or(-1)).await
}
pub async fn recent_books(
    pool: &SqlitePool,
    limit: i64,
    mine: Option<i64>,
) -> Result<Vec<BookSummary>, AppError> {
    recent_books_visible(pool, limit, mine, mine.unwrap_or(-1)).await
}
pub async fn highlight_books(
    pool: &SqlitePool,
    limit: i64,
    mine: Option<i64>,
    seed: i64,
) -> Result<Vec<BookSummary>, AppError> {
    highlight_books_visible(pool, limit, mine, seed, mine.unwrap_or(-1)).await
}
pub async fn get_author(
    pool: &SqlitePool,
    id: i64,
    mine: Option<i64>,
) -> Result<Option<AuthorDetail>, AppError> {
    get_author_visible(pool, id, mine, mine.unwrap_or(-1)).await
}
pub async fn related_books(pool: &SqlitePool, id: i64) -> Result<RelatedBooks, AppError> {
    related_books_visible(pool, id, -1).await
}
