use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::AppState;
use crate::auth::{AdminUser, Role};
use crate::error::AppError;
use crate::services::reader::ReadingDirection;
use crate::settings;

pub async fn watch_status(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<crate::watch_folder::WatchStatus>, AppError> {
    Ok(Json(crate::watch_folder::status(&state).await?))
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct RecentLogs {
    lines: Vec<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissingFile {
    file_id: i64,
    path: String,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityStatus {
    files_checked: usize,
    missing_count: usize,
    missing: Vec<MissingFile>,
    books_without_files: i64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct UpdatedBook {
    id: i64,
    title: String,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttentionItem {
    id: String,
    book_id: i64,
    title: String,
    authors: Vec<String>,
    kind: &'static str,
    error_code: Option<String>,
    error_message: Option<String>,
    updated_at: i64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct AttentionResponse {
    items: Vec<AttentionItem>,
    count: usize,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct ConnectionResult {
    status: &'static str,
    version: String,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    user_id: i64,
    profile_type: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct UserProfiles {
    users: Vec<UserProfile>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    settings: serde_json::Map<String, Value>,
    env_overrides: std::collections::BTreeMap<String, String>,
    secrets_configured: std::collections::BTreeMap<String, bool>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct SecretConfigured {
    configured: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum SettingUpdateResponse {
    Secret(SecretConfigured),
    String(String),
    Bool(bool),
    Integer(i64),
    Number(f64),
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LogQuery {
    pub limit: Option<usize>,
}

pub async fn image_job_status(
    _admin: AdminUser,
) -> Result<Json<crate::maintenance::ImageJobStatus>, AppError> {
    Ok(Json(crate::maintenance::status()))
}

pub async fn image_job_start(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<crate::maintenance::ImageJobStatus>), AppError> {
    if !crate::maintenance::start(state.clone()) {
        return Err(AppError::Conflict(
            "an image refresh is already running".to_string(),
        ));
    }
    Ok((StatusCode::ACCEPTED, Json(crate::maintenance::status())))
}

pub async fn metadata_job_status(
    _admin: AdminUser,
) -> Result<Json<crate::maintenance::MetadataJobStatus>, AppError> {
    Ok(Json(crate::maintenance::metadata_status()))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ForceFlag {
    #[serde(default)]
    pub force: bool,
}

pub async fn metadata_job_start(
    _admin: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(force): axum::extract::Query<ForceFlag>,
) -> Result<(StatusCode, Json<crate::maintenance::MetadataJobStatus>), AppError> {
    if !crate::maintenance::start_metadata(state.clone(), force.force) {
        return Err(AppError::Conflict(
            "a metadata enrichment is already running".to_string(),
        ));
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(crate::maintenance::metadata_status()),
    ))
}

pub async fn import_job_status(
    _admin: AdminUser,
) -> Result<Json<crate::adopt::ImportJobStatus>, AppError> {
    Ok(Json(crate::adopt::status()))
}

pub async fn import_job_start(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<crate::adopt::ImportJobStatus>), AppError> {
    if !crate::adopt::start(state.clone()) {
        return Err(AppError::Conflict(
            "an import of completed downloads is already running".to_string(),
        ));
    }
    Ok((StatusCode::ACCEPTED, Json(crate::adopt::status())))
}

pub async fn backup_status(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<crate::backup::BackupStatus>, AppError> {
    Ok(Json(
        crate::backup::status_at(&state, crate::server::now()).await?,
    ))
}

pub async fn metadata_job_cancel(_admin: AdminUser) -> Result<StatusCode, AppError> {
    if !crate::maintenance::cancel_metadata() {
        return Err(AppError::Conflict(
            "no metadata enrichment is running".to_string(),
        ));
    }
    Ok(StatusCode::ACCEPTED)
}

pub async fn logs(
    _admin: AdminUser,
    Query(params): Query<LogQuery>,
) -> Result<Json<RecentLogs>, AppError> {
    let lines = crate::observability::recent_logs(params.limit.unwrap_or(300));
    Ok(Json(RecentLogs { lines }))
}

pub async fn integrity(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<IntegrityStatus>, AppError> {
    let files: Vec<(i64, String)> = sqlx::query_as("SELECT id, path FROM book_files ORDER BY id")
        .fetch_all(&state.db)
        .await?;

    let mut missing = Vec::new();
    for (id, path) in &files {
        if !tokio::fs::try_exists(path).await? {
            missing.push(MissingFile {
                file_id: *id,
                path: path.clone(),
            });
        }
    }

    let books_without_files: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM books b
         WHERE NOT EXISTS (
             SELECT 1 FROM book_files f
             JOIN editions e ON e.id = f.edition_id
             WHERE e.book_id = b.id
         )",
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(IntegrityStatus {
        files_checked: files.len(),
        missing_count: missing.len(),
        missing: missing.into_iter().take(50).collect(),
        books_without_files,
    }))
}

pub async fn library_health(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<crate::library::queries::LibraryHealth>, AppError> {
    Ok(Json(
        crate::library::queries::library_health(&state.db).await?,
    ))
}

pub async fn backup(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let cache_dir = state.paths.config_dir.join("cache");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let stamp = now.as_secs() as i64;
    // The cache name is unique even for simultaneous backup requests.
    let cache_stamp = now.as_nanos() as i64;
    let target = crate::backup::create_at(&state.db, &cache_dir, true, cache_stamp).await?;
    let (body, length) = crate::backup::stream_download(target).await?;

    let disposition = format!("attachment; filename=\"bokhylle-backup-{stamp}.db\"");

    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition)
                    .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
            ),
            (
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&length.to_string())
                    .unwrap_or_else(|_| HeaderValue::from_static("0")),
            ),
        ],
        body,
    )
        .into_response())
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookUpdateInput {
    pub title: Option<String>,
    pub authors: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present_nullable")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_nullable")]
    pub language: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_nullable")]
    pub series: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_nullable")]
    pub series_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_nullable")]
    pub series_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "present_nullable")]
    pub series_sort_order: Option<Option<f64>>,
    pub publication_kind: Option<String>,
    #[serde(default, deserialize_with = "present_nullable")]
    pub publication_year: Option<Option<i64>>,
    #[serde(default)]
    pub use_automatic_metadata: Vec<crate::library::metadata_fields::MetadataField>,
    #[serde(default, deserialize_with = "present_nullable")]
    pub reading_direction: Option<Option<ReadingDirection>>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeriesRecord {
    id: i64,
    name: String,
    sort_name: Option<String>,
    default_reading_direction: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSeriesInput {
    name: String,
    sort_name: Option<String>,
    default_reading_direction: Option<ReadingDirection>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSeriesInput {
    name: Option<String>,
    #[serde(default, deserialize_with = "present_nullable")]
    sort_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_nullable")]
    default_reading_direction: Option<Option<ReadingDirection>>,
}

async fn series_record(pool: &sqlx::SqlitePool, id: i64) -> Result<SeriesRecord, AppError> {
    let row: Option<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, name, sort_name, default_reading_direction FROM series WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(
        |(id, name, sort_name, default_reading_direction)| SeriesRecord {
            id,
            name,
            sort_name,
            default_reading_direction,
        },
    )
    .ok_or_else(|| AppError::NotFound("series not found".into()))
}

pub async fn list_series(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<SeriesRecord>>, AppError> {
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, name, sort_name, default_reading_direction FROM series
         ORDER BY coalesce(sort_name, name) COLLATE NOCASE, id",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(id, name, sort_name, default_reading_direction)| SeriesRecord {
                    id,
                    name,
                    sort_name,
                    default_reading_direction,
                },
            )
            .collect(),
    ))
}

pub async fn create_series(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateSeriesInput>,
) -> Result<Json<SeriesRecord>, AppError> {
    let name = body.name.trim();
    if name.is_empty() || name.len() > 200 {
        return Err(AppError::BadRequest(
            "series name must have 1–200 characters".into(),
        ));
    }
    let id = sqlx::query(
        "INSERT INTO series (name, sort_name, default_reading_direction) VALUES (?, ?, ?)",
    )
    .bind(name)
    .bind(optional_text(body.sort_name).flatten())
    .bind(body.default_reading_direction.map(ReadingDirection::as_str))
    .execute(&state.db)
    .await?
    .last_insert_rowid();
    Ok(Json(series_record(&state.db, id).await?))
}

pub async fn update_series(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateSeriesInput>,
) -> Result<Json<SeriesRecord>, AppError> {
    series_record(&state.db, id).await?;
    let name = body.name.as_ref().map(|name| name.trim());
    if name.is_some_and(|name| name.is_empty() || name.len() > 200) {
        return Err(AppError::BadRequest(
            "series name must have 1–200 characters".into(),
        ));
    }
    sqlx::query(
        "UPDATE series SET name = COALESCE(?, name),
         sort_name = CASE WHEN ? THEN ? ELSE sort_name END,
         default_reading_direction = CASE WHEN ? THEN ? ELSE default_reading_direction END,
         updated_at = unixepoch() WHERE id = ?",
    )
    .bind(name)
    .bind(body.sort_name.is_some())
    .bind(
        body.sort_name
            .flatten()
            .and_then(|value| optional_text(Some(value)).flatten()),
    )
    .bind(body.default_reading_direction.is_some())
    .bind(
        body.default_reading_direction
            .flatten()
            .map(ReadingDirection::as_str),
    )
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(Json(series_record(&state.db, id).await?))
}

fn present_nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

fn optional_text(value: Option<String>) -> Option<Option<String>> {
    value.map(|value| {
        let value = value.trim().to_string();
        if value.is_empty() { None } else { Some(value) }
    })
}

pub async fn update_book(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<BookUpdateInput>,
) -> Result<Json<UpdatedBook>, AppError> {
    let mut transaction = state
        .db
        .begin_with(sqlx::AssertSqlSafe("BEGIN IMMEDIATE".to_string()))
        .await?;
    let Some(current_title): Option<String> =
        sqlx::query_scalar("SELECT title FROM books WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?
    else {
        return Err(AppError::NotFound("book not found".to_string()));
    };

    use crate::library::metadata_fields::{self, MetadataField as Field, Scope};
    let supplied = [
        (
            Field::Title,
            body.title
                .as_ref()
                .is_some_and(|title| !title.trim().is_empty()),
        ),
        (Field::Authors, body.authors.is_some()),
        (Field::Description, body.description.is_some()),
        (Field::Language, body.language.is_some()),
        (Field::Series, body.series.is_some()),
        (
            Field::SeriesNumber,
            body.series_number.is_some() || body.series_id.is_some(),
        ),
        (Field::PublicationYear, body.publication_year.is_some()),
    ];
    for field in &body.use_automatic_metadata {
        if supplied
            .iter()
            .any(|(candidate, present)| candidate == field && *present)
        {
            return Err(AppError::BadRequest(
                "a metadata field cannot be corrected and reset together".into(),
            ));
        }
    }
    let first_edition: Option<i64> =
        sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? ORDER BY id LIMIT 1")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await?;
    for (field, present) in supplied {
        if present {
            let scope = if field == Field::PublicationYear {
                first_edition.map(Scope::Edition)
            } else {
                Some(Scope::Book(id))
            };
            if let Some(scope) = scope {
                metadata_fields::mark_manual(&mut transaction, scope, field).await?;
            }
        }
    }
    for field in &body.use_automatic_metadata {
        let scope = if matches!(field, Field::PublicationYear | Field::Publisher) {
            first_edition.map(Scope::Edition)
        } else {
            Some(Scope::Book(id))
        };
        if let Some(scope) = scope {
            metadata_fields::reset(&mut transaction, scope, *field).await?;
        }
    }
    // A reset may have restored the title before the ordinary patch is applied.
    let current_title: String = if body.use_automatic_metadata.contains(&Field::Title) {
        sqlx::query_scalar("SELECT title FROM books WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await?
    } else {
        current_title
    };

    let title = match body.title {
        Some(title) if !title.trim().is_empty() => title.trim().to_string(),
        Some(_) => current_title,
        None => current_title,
    };
    let normalized_title = bokhylle_core::identity::normalize_text(&title);
    let series_text = body.series.map(|value| optional_text(value).flatten());
    let marks_classification_reviewed =
        body.publication_kind.is_some() || body.series_id.is_some() || body.series_number.is_some();
    let direction_present = body.reading_direction.is_some();
    let direction_value = body
        .reading_direction
        .flatten()
        .map(ReadingDirection::as_str);
    let sort_present = body.series_sort_order.is_some();
    let sort_value = body.series_sort_order.flatten();
    if sort_value.is_some_and(|value| !value.is_finite()) {
        return Err(AppError::BadRequest(
            "series sort order must be finite".into(),
        ));
    }
    let kind = match body.publication_kind.as_deref() {
        Some("unknown" | "book" | "comic" | "manga" | "magazine" | "catalogue") => {
            body.publication_kind.as_deref()
        }
        Some(_) => return Err(AppError::BadRequest("unknown publication kind".into())),
        None => None,
    };

    sqlx::query(
        "UPDATE books SET title = ?, normalized_title = ?,
                description = CASE WHEN ? THEN ? ELSE description END,
                language = CASE WHEN ? THEN ? ELSE language END,
                series = CASE WHEN ? THEN ? ELSE series END,
                series_number = CASE WHEN ? THEN ? ELSE series_number END,
                series_sort_order = CASE WHEN ? THEN ? ELSE series_sort_order END,
                publication_kind = COALESCE(?, publication_kind),
                reading_direction = CASE WHEN ? THEN ? ELSE reading_direction END,
                classification_reviewed_at = CASE WHEN ? THEN unixepoch()
                    ELSE classification_reviewed_at END,
                updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(&title)
    .bind(&normalized_title)
    .bind(body.description.is_some())
    .bind(
        body.description
            .and_then(|value| optional_text(value).flatten()),
    )
    .bind(body.language.is_some())
    .bind(
        body.language
            .and_then(|value| optional_text(value).flatten()),
    )
    .bind(series_text.is_some())
    .bind(series_text.as_ref().and_then(|value| value.as_deref()))
    .bind(body.series_number.is_some())
    .bind(
        body.series_number
            .and_then(|value| optional_text(value).flatten()),
    )
    .bind(sort_present)
    .bind(sort_value)
    .bind(kind)
    .bind(direction_present)
    .bind(direction_value)
    .bind(marks_classification_reviewed)
    .bind(id)
    .execute(&mut *transaction)
    .await?;

    if let Some(series_id) = body.series_id {
        if let Some(series_id) = series_id {
            let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM series WHERE id = ?")
                .bind(series_id)
                .fetch_optional(&mut *transaction)
                .await?;
            if exists.is_none() {
                return Err(AppError::BadRequest("series does not exist".into()));
            }
            sqlx::query("UPDATE books SET series_id = ?, series_link_locked = 1 WHERE id = ?")
                .bind(series_id)
                .bind(id)
                .execute(&mut *transaction)
                .await?;
        } else {
            sqlx::query("UPDATE books SET series_id = NULL, series_link_locked = 1 WHERE id = ?")
                .bind(id)
                .execute(&mut *transaction)
                .await?;
        }
    } else if let Some(series_text) = series_text {
        if let Some(name) = series_text {
            sqlx::query(
                "INSERT INTO series (name, auto_key) SELECT ?, ?
                 WHERE NOT EXISTS (SELECT 1 FROM series WHERE name = ? OR auto_key = ?)",
            )
            .bind(&name)
            .bind(&name)
            .bind(&name)
            .bind(&name)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "UPDATE books SET series_id = (SELECT id FROM series
                 WHERE name = ? OR auto_key = ?
                 ORDER BY CASE WHEN name = ? THEN 0 ELSE 1 END, id LIMIT 1)
                 WHERE id = ?",
            )
            .bind(&name)
            .bind(&name)
            .bind(&name)
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        } else {
            sqlx::query("UPDATE books SET series_id = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut *transaction)
                .await?;
        }
    }

    if let Some(authors) = body.authors {
        sqlx::query("DELETE FROM book_authors WHERE book_id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;

        for (position, author) in authors
            .iter()
            .map(|author| author.trim())
            .filter(|author| !author.is_empty())
            .enumerate()
        {
            let normalized = bokhylle_core::identity::normalize_text(author);
            let existing: Option<i64> =
                sqlx::query_scalar("SELECT id FROM authors WHERE normalized_name = ? LIMIT 1")
                    .bind(&normalized)
                    .fetch_optional(&mut *transaction)
                    .await?;
            let author_id = match existing {
                Some(id) => id,
                None => sqlx::query("INSERT INTO authors (name, normalized_name) VALUES (?, ?)")
                    .bind(author)
                    .bind(&normalized)
                    .execute(&mut *transaction)
                    .await?
                    .last_insert_rowid(),
            };

            sqlx::query(
                "INSERT OR IGNORE INTO book_authors (book_id, author_id, position) VALUES (?, ?, ?)",
            )
            .bind(id)
            .bind(author_id)
            .bind(position as i64)
            .execute(&mut *transaction)
            .await?;
        }
    }

    if let Some(year) = body.publication_year {
        sqlx::query(
            "UPDATE editions SET publication_year = ?
             WHERE id = (SELECT id FROM editions WHERE book_id = ? ORDER BY id LIMIT 1)",
        )
        .bind(year)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    }

    if !body.use_automatic_metadata.is_empty() {
        sqlx::query("UPDATE books SET metadata_checked_at = NULL WHERE id = ?")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
    }
    crate::library::refresh_fts(&mut transaction, id).await?;
    transaction.commit().await?;

    Ok(Json(UpdatedBook { id, title }))
}

pub async fn delete_book(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let paths: Vec<String> = sqlx::query_scalar(
        "SELECT f.path FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ?",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let cover_path: Option<String> =
        sqlx::query_scalar("SELECT cover_path FROM books WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .flatten();

    let mut transaction = state.db.begin().await?;
    sqlx::query("DELETE FROM books_fts WHERE rowid = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    let result = sqlx::query("DELETE FROM books WHERE id = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("book not found".to_string()));
    }

    for path in paths {
        if let Err(error) = tokio::fs::remove_file(&path).await {
            tracing::warn!(%error, %path, "admin.book.file_remove_failed");
        }
    }

    if let Some(cover_path) = cover_path {
        let referenced: i64 = sqlx::query_scalar("SELECT count(*) FROM books WHERE cover_path = ?")
            .bind(&cover_path)
            .fetch_one(&state.db)
            .await?;
        if referenced == 0
            && let Err(error) = tokio::fs::remove_file(&cover_path).await
        {
            tracing::warn!(%error, %cover_path, "admin.book.cover_remove_failed");
        }
    }

    tracing::info!(book_id = id, "admin.book.deleted");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_book_file(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path((book_id, file_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    let path: Option<String> = sqlx::query_scalar(
        "SELECT f.path FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE f.id = ? AND e.book_id = ?",
    )
    .bind(file_id)
    .bind(book_id)
    .fetch_optional(&state.db)
    .await?;

    let Some(path) = path else {
        return Err(AppError::NotFound("file not found".to_string()));
    };

    sqlx::query("DELETE FROM book_files WHERE id = ?")
        .bind(file_id)
        .execute(&state.db)
        .await?;

    if let Err(error) = tokio::fs::remove_file(&path).await {
        tracing::warn!(%error, %path, "admin.file.remove_failed");
    }

    tracing::info!(book_id, file_id, "admin.book_file.deleted");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserView {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub role: Role,
    pub credential_type: String,
    pub disabled: bool,
    pub reader_count: i64,
    pub preferred_languages: Vec<String>,
    /// Whether a child may submit a book for adult approval.
    pub can_request: bool,
    /// Children only: access to public catalogue browsing and suggestions.
    pub can_discover: bool,
    /// Adults only: permission to add new files to the shared collection.
    pub can_acquire: bool,
    pub avatar_preset: Option<String>,
    pub avatar_url: Option<String>,
}

fn parse_languages(stored: Option<String>) -> Vec<String> {
    stored
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
}

async fn preferred_languages_of(
    pool: &sqlx::SqlitePool,
    user_id: i64,
) -> Result<Vec<String>, AppError> {
    let stored: Option<String> =
        sqlx::query_scalar("SELECT preferred_languages FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?
            .flatten();
    Ok(parse_languages(stored))
}

struct AdminAccess {
    can_request: bool,
    can_discover: bool,
    can_acquire: bool,
}

async fn user_view(
    state: &AppState,
    user: crate::auth::User,
    credential_type: &str,
    readers: i64,
    disabled: bool,
    preferred_languages: Vec<String>,
    access: AdminAccess,
) -> Result<AdminUserView, AppError> {
    let (avatar_preset, avatar_version): (Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT avatar_preset,
         CASE WHEN EXISTS (SELECT 1 FROM user_avatars WHERE user_id = users.id)
              THEN avatar_version ELSE NULL END FROM users WHERE id = ?",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    let avatar_url = avatar_version
        .filter(|_| !disabled)
        .map(|version| format!("/api/auth/users/{}/avatar?v={version}", user.id));
    Ok(AdminUserView {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        role: user.role,
        credential_type: credential_type.to_string(),
        disabled,
        reader_count: readers,
        preferred_languages,
        can_request: access.can_request,
        can_discover: access.can_discover,
        can_acquire: access.can_acquire,
        avatar_preset,
        avatar_url,
    })
}

pub async fn list_users(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminUserView>>, AppError> {
    let users = state.auth.list_users().await?;
    let mut views = Vec::with_capacity(users.len());
    for (user, readers, disabled) in users {
        let (credential_type, can_request, can_discover): (Option<String>, i64, i64) =
            sqlx::query_as(
                "SELECT credential_type, can_request, can_discover FROM users WHERE id = ?",
            )
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?
            .unwrap_or((None, 1, 0));
        let preferred_languages = preferred_languages_of(&state.db, user.id).await?;
        let can_acquire = crate::auth::can_acquire(&state.db, &user).await?;
        views.push(
            user_view(
                &state,
                user,
                credential_type.as_deref().unwrap_or("legacy"),
                readers,
                disabled,
                preferred_languages,
                AdminAccess {
                    can_request: can_request != 0,
                    can_discover: can_discover != 0,
                    can_acquire,
                },
            )
            .await?,
        );
    }
    Ok(Json(views))
}

pub use crate::services::users::CreateUserInput;

/// The ordered list is the reader's preference; the legacy single column
/// keeps the first entry so older queries stay meaningful.
async fn set_preferred_languages(
    pool: &sqlx::SqlitePool,
    user_id: i64,
    languages: &[String],
) -> Result<(), AppError> {
    let cleaned: Vec<String> = languages
        .iter()
        .map(|language| language.trim().to_ascii_lowercase())
        .filter(|language| !language.is_empty())
        .collect();
    let serialized = serde_json::to_string(&cleaned)
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    sqlx::query("UPDATE users SET preferred_languages = ?, preferred_language = ? WHERE id = ?")
        .bind(&serialized)
        .bind(cleaned.first().map(String::as_str))
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

fn parse_role(value: Option<&str>) -> Result<Option<Role>, AppError> {
    match value {
        None => Ok(None),
        Some("admin") => Ok(Some(Role::Admin)),
        Some("user") => Ok(Some(Role::User)),
        Some(_) => Err(AppError::BadRequest(
            "role must be 'admin' or 'user'".to_string(),
        )),
    }
}

pub async fn create_user(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateUserInput>,
) -> Result<(StatusCode, Json<AdminUserView>), AppError> {
    let created = crate::services::users::create(&state, &admin, body).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            user_view(
                &state,
                created.user,
                &created.credential_type,
                0,
                false,
                created.preferred_languages,
                AdminAccess {
                    can_request: created.can_request,
                    can_discover: created.can_discover,
                    can_acquire: created.can_acquire,
                },
            )
            .await?,
        ),
    ))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserInput {
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub password: Option<String>,
    pub credential: Option<String>,
    pub credential_type: Option<String>,
    pub disabled: Option<bool>,
    pub preferred_languages: Option<Vec<String>>,
    /// Children only: permits request submission and basic catalogue search.
    pub can_request: Option<bool>,
    /// Children only: allow public catalogue browsing and suggestions.
    pub can_discover: Option<bool>,
    /// Adults only: allow adding new books to the shared library.
    pub can_acquire: Option<bool>,
    /// Omitted preserves the mark; null restores initials. Does not remove a photo.
    #[serde(default, deserialize_with = "present_nullable")]
    pub avatar_preset: Option<Option<crate::services::users::ProfileMark>>,
}

pub async fn update_user(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUserInput>,
) -> Result<Json<AdminUserView>, AppError> {
    let role = parse_role(body.role.as_deref())?;

    if id == admin.id && (body.disabled == Some(true) || matches!(role, Some(Role::User))) {
        return Err(AppError::Unprocessable(
            "you cannot disable or demote your own account".to_string(),
        ));
    }

    let users = state.auth.list_users().await?;
    let Some((target, _, _)) = users.iter().find(|(user, _, _)| user.id == id) else {
        return Err(AppError::NotFound("user not found".to_string()));
    };

    // A child profile can never be promoted: admin implies adult.
    if matches!(role, Some(Role::Admin)) {
        let profile: Option<String> =
            sqlx::query_scalar("SELECT profile_type FROM users WHERE id = ?")
                .bind(id)
                .fetch_optional(&state.db)
                .await?;
        if profile.as_deref() == Some("child") {
            return Err(AppError::Unprocessable(
                "a child profile cannot be an administrator".to_string(),
            ));
        }
    }

    let removing_admin = matches!(target.role, Role::Admin)
        && (body.disabled == Some(true) || matches!(role, Some(Role::User)));
    if removing_admin
        && users
            .iter()
            .filter(|(user, _, disabled)| matches!(user.role, Role::Admin) && !disabled)
            .count()
            <= 1
    {
        return Err(AppError::Unprocessable(
            "at least one active administrator is required".to_string(),
        ));
    }

    let display_name = body
        .display_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let credential = body.credential.or(body.password).map(|secret| {
        (
            body.credential_type
                .unwrap_or_else(|| "password".to_string()),
            secret,
        )
    });
    let updated = state
        .auth
        .update_user_admin(id, display_name, role, credential, body.disabled)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    if let Some(preset) = body.avatar_preset {
        crate::services::users::set_profile_mark(&state.db, id, preset).await?;
    }

    // A disabled account must not receive queued background sends.
    if body.disabled == Some(true) {
        sqlx::query("UPDATE acquisition_requests SET deliver_on_ready = 0 WHERE user_id = ?")
            .bind(id)
            .execute(&state.db)
            .await?;
    }

    if let Some(languages) = body.preferred_languages.as_deref() {
        set_preferred_languages(&state.db, id, languages).await?;
    }
    if let Some(can_request) = body.can_request {
        sqlx::query("UPDATE users SET can_request = ? WHERE id = ?")
            .bind(can_request)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(can_discover) = body.can_discover {
        sqlx::query("UPDATE users SET can_discover = ? WHERE id = ?")
            .bind(can_discover)
            .bind(id)
            .execute(&state.db)
            .await?;
    }
    if let Some(can_acquire) = body.can_acquire {
        sqlx::query("UPDATE users SET can_acquire = ? WHERE id = ?")
            .bind(can_acquire)
            .bind(id)
            .execute(&state.db)
            .await?;
        if !can_acquire {
            sqlx::query("UPDATE author_follows SET auto_acquire = 0 WHERE user_id = ?")
                .bind(id)
                .execute(&state.db)
                .await?;
        }
    }

    let readers = users
        .iter()
        .find(|(user, _, _)| user.id == id)
        .map(|(_, readers, _)| *readers)
        .unwrap_or(0);

    let credential_type: Option<String> =
        sqlx::query_scalar("SELECT credential_type FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .flatten();
    let disabled: i64 = sqlx::query_scalar("SELECT disabled FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let can_request: i64 = sqlx::query_scalar("SELECT can_request FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let can_discover: i64 = sqlx::query_scalar("SELECT can_discover FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let preferred_languages = preferred_languages_of(&state.db, id).await?;
    let can_acquire = crate::auth::can_acquire(&state.db, &updated).await?;
    Ok(Json(
        user_view(
            &state,
            updated,
            credential_type.as_deref().unwrap_or("legacy"),
            readers,
            disabled != 0,
            preferred_languages,
            AdminAccess {
                can_request: can_request != 0,
                can_discover: can_discover != 0,
                can_acquire,
            },
        )
        .await?,
    ))
}

pub async fn list_settings(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<SettingsResponse>, AppError> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&state.db)
            .await?;

    let mut stored = serde_json::Map::new();
    for (key, value) in rows {
        if settings::is_secret(&key) {
            continue;
        }
        stored.insert(key, serde_json::from_str(&value).unwrap_or(Value::Null));
    }

    let mut env_overrides = std::collections::BTreeMap::new();
    let mut secrets_configured = std::collections::BTreeMap::new();

    for key in settings::KNOWN_KEYS {
        let env_value = settings::env_var_for(key).and_then(|name| std::env::var(name).ok());

        if settings::is_secret(key) {
            let configured = state.settings.raw(key).await?.is_some() || env_value.is_some();
            secrets_configured.insert(key.to_string(), configured);
        } else if let Some(value) = env_value {
            env_overrides.insert(key.to_string(), value);
        }
    }

    Ok(Json(SettingsResponse {
        settings: stored,
        env_overrides,
        secrets_configured,
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateSetting {
    pub value: Value,
}

pub async fn update_setting(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(key): Path<String>,
    Json(body): Json<UpdateSetting>,
) -> Result<Json<SettingUpdateResponse>, AppError> {
    if !settings::KNOWN_KEYS.contains(&key.as_str()) {
        return Err(AppError::NotFound(format!("unknown setting '{key}'")));
    }

    settings::validate(&key, &body.value)?;

    state.settings.set(&key, &body.value).await?;
    if matches!(
        key.as_str(),
        settings::BACKUP_INTERVAL_HOURS | settings::BACKUP_KEEP
    ) {
        state.server.backup_wakeup.notify_one();
    }
    if key == settings::UPDATE_CHECKS {
        state.server.release_wakeup.notify_one();
    }
    tracing::info!(key = %key, secret = settings::is_secret(&key), "settings.updated");

    if settings::is_secret(&key) {
        Ok(Json(SettingUpdateResponse::Secret(SecretConfigured {
            configured: true,
        })))
    } else {
        let value = match body.value {
            Value::String(value) => SettingUpdateResponse::String(value),
            Value::Bool(value) => SettingUpdateResponse::Bool(value),
            Value::Number(value) if value.is_i64() => {
                SettingUpdateResponse::Integer(value.as_i64().expect("checked integer"))
            }
            Value::Number(value) => {
                SettingUpdateResponse::Number(value.as_f64().expect("checked number"))
            }
            _ => unreachable!("validated setting has a scalar value"),
        };
        Ok(Json(value))
    }
}

pub async fn integration_status(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<crate::health::IntegrationStatus>, AppError> {
    Ok(Json(crate::health::integrations(&state).await?))
}

/// The Needs Attention inbox: acquisitions that require a human decision.
/// Download progress never belongs here; this is review states only.
pub async fn attention(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<AttentionResponse>, AppError> {
    type AttentionRow = (
        String,
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
    );
    let rows: Vec<AttentionRow> = sqlx::query_as(
        "SELECT a.id, a.book_id, b.title,
                COALESCE((SELECT group_concat(au.name, ', ')
                          FROM book_authors ba JOIN authors au ON au.id = ba.author_id
                          WHERE ba.book_id = b.id), ''),
                a.status, a.error_code, a.error_message, a.updated_at
         FROM acquisitions a
         JOIN books b ON b.id = a.book_id
         WHERE a.status = 'NEEDS_REVIEW'
            OR (a.status = 'IMPORT_FAILED'
                AND NOT (a.retry_stopped = 0 AND a.next_retry_at IS NOT NULL))
            OR (a.status = 'CANCELLED' AND a.cancel_pending = 1)
         ORDER BY a.updated_at DESC
         LIMIT 50",
    )
    .fetch_all(&state.db)
    .await?;

    let items: Vec<AttentionItem> = rows
        .into_iter()
        .map(
            |(id, book_id, title, authors, status, error_code, error_message, updated_at)| {
                // Known human actions, not statuses: review the found files, or
                // repair a path/mount problem before choosing a file manually.
                let kind = if status == "NEEDS_REVIEW" {
                    "review"
                } else if status == "CANCELLED" {
                    "cancel_failed"
                } else if error_code.as_deref() == Some("content_missing") {
                    "path"
                } else {
                    "failed_import"
                };
                AttentionItem {
                    id,
                    book_id,
                    title,
                    authors: authors
                        .split(", ")
                        .filter(|part| !part.is_empty())
                        .map(str::to_string)
                        .collect(),
                    kind,
                    error_code,
                    error_message,
                    updated_at,
                }
            },
        )
        .collect();

    Ok(Json(AttentionResponse {
        count: items.len(),
        items,
    }))
}

pub async fn test_prowlarr(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ConnectionResult>, AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Err(AppError::Unprocessable(
            "integrations are not available".to_string(),
        ));
    };

    let Some(indexer) = factory.indexer_named(&state, "prowlarr").await? else {
        return Err(AppError::Unprocessable(
            "Prowlarr is not configured".to_string(),
        ));
    };

    let version = indexer
        .test_connection()
        .await
        .map_err(|error| AppError::Unprocessable(format!("Prowlarr connection failed: {error}")))?;

    Ok(Json(ConnectionResult {
        status: "ok",
        version,
    }))
}

pub async fn test_torznab(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ConnectionResult>, AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Err(AppError::Unprocessable(
            "integrations are not available".to_string(),
        ));
    };
    let Some(indexer) = factory.indexer_named(&state, "torznab").await? else {
        return Err(AppError::Unprocessable(
            "Torznab is not configured".to_string(),
        ));
    };
    let version = indexer
        .test_connection()
        .await
        .map_err(|error| AppError::Unprocessable(format!("Torznab connection failed: {error}")))?;
    Ok(Json(ConnectionResult {
        status: "ok",
        version,
    }))
}

pub async fn test_newznab(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ConnectionResult>, AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Err(AppError::Unprocessable(
            "integrations are not available".into(),
        ));
    };
    let Some(indexer) = factory.indexer_named(&state, "newznab").await? else {
        return Err(AppError::Unprocessable("Newznab is not configured".into()));
    };
    let version = indexer
        .test_connection()
        .await
        .map_err(|error| AppError::Unprocessable(format!("Newznab connection failed: {error}")))?;
    Ok(Json(ConnectionResult {
        status: "ok",
        version,
    }))
}

pub async fn test_sabnzbd(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ConnectionResult>, AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Err(AppError::Unprocessable(
            "integrations are not available".into(),
        ));
    };
    let Some(client) = factory.nzb_downloader(&state).await? else {
        return Err(AppError::Unprocessable("SABnzbd is not configured".into()));
    };
    let version = client
        .test_connection()
        .await
        .map_err(|error| AppError::Unprocessable(format!("SABnzbd connection failed: {error}")))?;
    Ok(Json(ConnectionResult {
        status: "ok",
        version,
    }))
}

pub async fn test_qbittorrent(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ConnectionResult>, AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Err(AppError::Unprocessable(
            "integrations are not available".to_string(),
        ));
    };

    let Some(downloader) = factory.downloader(&state).await? else {
        return Err(AppError::Unprocessable(
            "qBittorrent is not configured".to_string(),
        ));
    };

    let version = downloader.test_connection().await.map_err(|error| {
        AppError::Unprocessable(format!("qBittorrent connection failed: {error}"))
    })?;

    Ok(Json(ConnectionResult {
        status: "ok",
        version,
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTypeInput {
    pub profile_type: String,
}

pub async fn set_profile_type(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ProfileTypeInput>,
) -> Result<StatusCode, AppError> {
    if !matches!(body.profile_type.as_str(), "adult" | "child") {
        return Err(AppError::Unprocessable(
            "profile type must be 'adult' or 'child'".to_string(),
        ));
    }
    let current: Option<(String, String)> =
        sqlx::query_as("SELECT profile_type, role FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let Some((current, role)) = current else {
        return Err(AppError::NotFound("user not found".to_string()));
    };
    // An administrator is always an adult; keep the invariant on mutation too.
    if body.profile_type == "child" && role == "admin" {
        return Err(AppError::Unprocessable(
            "an administrator cannot be a child profile".to_string(),
        ));
    }
    if current != body.profile_type {
        let mut tx = state.db.begin().await?;
        if body.profile_type == "child" {
            sqlx::query("UPDATE book_access SET sharing = 'private' WHERE user_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            // Only a parent assigns a child's shelf. A former adult's own
            // shelf must not read as parent approval; preferences and
            // interests survive as taste.
            sqlx::query("UPDATE user_books SET on_shelf = 0 WHERE user_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            // Background adult capabilities must not survive the conversion:
            // author automation could otherwise acquire and deliver books the
            // parent never assigned.
            sqlx::query(
                "UPDATE author_follows SET auto_acquire = 0, delivery_target_id = NULL
                 WHERE user_id = ?",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE acquisition_requests SET deliver_on_ready = 0 WHERE user_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        // The wizard differs by profile type, so a converted account must
        // run the flow that matches its new role.
        sqlx::query(
            "UPDATE users SET profile_type = ?,
                    can_acquire = CASE WHEN ? = 'child' THEN 0 ELSE can_acquire END,
                    onboarded_at = NULL WHERE id = ?",
        )
        .bind(&body.profile_type)
        .bind(&body.profile_type)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn restart_onboarding(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let updated = sqlx::query("UPDATE users SET onboarded_at = NULL WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::NotFound("user not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn user_profiles(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<UserProfiles>, AppError> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, profile_type FROM users ORDER BY username")
            .fetch_all(&state.db)
            .await?;
    let entries: Vec<UserProfile> = rows
        .into_iter()
        .map(|(user_id, profile_type)| UserProfile {
            user_id,
            profile_type,
        })
        .collect();
    Ok(Json(UserProfiles { users: entries }))
}
