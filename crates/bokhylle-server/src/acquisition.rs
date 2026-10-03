use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use bokhylle_acquisition::model::EvaluatedRelease;
use bokhylle_acquisition::state::{AcquisitionStatus, validate_transition};

use crate::error::AppError;

const SELECT_COLUMNS: &str = "id, book_id, user_id, preferred_format, preferred_language,
       acquisition_languages, status,
       selected_release_name, selected_release_indexer, selected_release_score,
       selected_release_confidence, selected_release_size, selected_release_format,
       selected_release_seeders, ask_before_download, download_speed,
       download_provider, provider_download_id,
       error_code, error_message, progress, content_path, created_at, updated_at";

fn protected_states_sql() -> String {
    AcquisitionStatus::DUPLICATE_PROTECTED_STATES
        .iter()
        .map(|status| format!("'{}'", status.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

const GET_SQL: &str = "SELECT id, book_id, user_id, preferred_format, preferred_language,
       acquisition_languages, status,
       selected_release_name, selected_release_indexer, selected_release_score,
       selected_release_confidence, selected_release_size, selected_release_format,
       selected_release_seeders, ask_before_download, download_speed,
       download_provider, provider_download_id,
       error_code, error_message, progress, content_path, created_at, updated_at
FROM acquisitions
WHERE id = ?";

#[derive(Debug, Clone, FromRow)]
pub struct Acquisition {
    pub id: String,
    pub book_id: i64,
    pub user_id: Option<i64>,
    pub preferred_format: Option<String>,
    pub preferred_language: Option<String>,
    /// Frozen ordered accepted languages as JSON; NULL on legacy rows.
    pub acquisition_languages: Option<String>,
    pub status: String,
    pub selected_release_name: Option<String>,
    pub selected_release_indexer: Option<String>,
    pub selected_release_score: Option<i64>,
    pub selected_release_confidence: Option<f64>,
    pub selected_release_size: Option<i64>,
    pub selected_release_format: Option<String>,
    pub selected_release_seeders: Option<i64>,
    pub ask_before_download: bool,
    pub download_speed: Option<i64>,
    pub download_provider: Option<String>,
    pub provider_download_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub progress: f64,
    pub content_path: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Acquisition {
    pub fn status(&self) -> Result<AcquisitionStatus, AppError> {
        AcquisitionStatus::from_db(&self.status).ok_or_else(|| {
            AppError::Unprocessable(format!("unknown acquisition status '{}'", self.status))
        })
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcquisitionView {
    pub id: String,
    pub book_id: i64,
    pub book_title: String,
    pub book_authors: Vec<String>,
    pub status: AcquisitionStatus,
    pub preferred_format: Option<String>,
    pub preferred_language: Option<String>,
    pub source: Option<String>,
    pub source_author: Option<String>,
    pub selected_release_name: Option<String>,
    pub selected_release_indexer: Option<String>,
    pub selected_release_score: Option<i64>,
    pub selected_release_confidence: Option<f64>,
    pub selected_release_size: Option<i64>,
    pub selected_release_format: Option<String>,
    pub selected_release_seeders: Option<i64>,
    pub ask_before_download: bool,
    pub download_speed: Option<i64>,
    pub deliver_on_ready: bool,
    /// The viewing profile participates in this acquisition.
    pub requested_by_me: bool,
    /// Frozen destination, visible only to the requester who chose it.
    pub scheduled_delivery_address: Option<String>,
    pub requested_by_user_id: Option<i64>,
    pub requested_by: Option<String>,
    pub download_provider: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub progress: f64,
    /// True while an automatic retry is scheduled for this failure.
    pub keep_looking: bool,
    pub retry_attempts: i64,
    pub next_retry_at: Option<i64>,
    /// NONE | PENDING | SENT | FAILED for the viewing requester, derived from
    /// the delivery record rather than the consumed delivery intent.
    pub delivery_status: String,
    /// True when this viewer approved the request this acquisition fulfils,
    /// so an approving adult can manage a child's acquisition.
    pub managed_by_me: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(FromRow)]
struct AcquisitionRow {
    id: String,
    book_id: i64,
    book_title: String,
    book_authors: String,
    status: String,
    preferred_format: Option<String>,
    preferred_language: Option<String>,
    selected_release_name: Option<String>,
    selected_release_indexer: Option<String>,
    selected_release_score: Option<i64>,
    selected_release_confidence: Option<f64>,
    selected_release_size: Option<i64>,
    selected_release_format: Option<String>,
    selected_release_seeders: Option<i64>,
    ask_before_download: i64,
    download_speed: Option<i64>,
    requested_by_user_id: Option<i64>,
    my_deliver_on_ready: i64,
    requested_by_me: i64,
    scheduled_delivery_address: Option<String>,
    requested_by: Option<String>,
    download_provider: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    progress: f64,
    retry_attempts: i64,
    next_retry_at: Option<i64>,
    retry_stopped: i64,
    delivery_status: Option<String>,
    managed_by_me: i64,
    created_at: i64,
    updated_at: i64,
}

impl AcquisitionRow {
    fn into_view(self) -> Result<AcquisitionView, AppError> {
        let status = AcquisitionStatus::from_db(&self.status).ok_or_else(|| {
            AppError::Unprocessable(format!("unknown acquisition status '{}'", self.status))
        })?;

        Ok(AcquisitionView {
            id: self.id,
            book_id: self.book_id,
            book_title: self.book_title,
            book_authors: self
                .book_authors
                .split(", ")
                .map(str::trim)
                .filter(|author| !author.is_empty())
                .map(str::to_string)
                .collect(),
            status,
            preferred_format: self.preferred_format,
            preferred_language: self.preferred_language,
            source: None,
            source_author: None,
            selected_release_name: self.selected_release_name,
            selected_release_indexer: self.selected_release_indexer,
            selected_release_score: self.selected_release_score,
            selected_release_confidence: self.selected_release_confidence,
            selected_release_size: self.selected_release_size,
            selected_release_format: self.selected_release_format,
            selected_release_seeders: self.selected_release_seeders,
            deliver_on_ready: self.my_deliver_on_ready != 0,
            requested_by_me: self.requested_by_me != 0,
            scheduled_delivery_address: self.scheduled_delivery_address,
            requested_by_user_id: self.requested_by_user_id,
            ask_before_download: self.ask_before_download != 0,
            download_speed: self.download_speed,
            requested_by: self.requested_by,
            download_provider: self.download_provider,
            error_code: self.error_code,
            error_message: self.error_message,
            progress: self.progress,
            keep_looking: self.retry_stopped == 0 && self.next_retry_at.is_some(),
            retry_attempts: self.retry_attempts,
            next_retry_at: self.next_retry_at,
            delivery_status: self.delivery_status.unwrap_or_else(|| "NONE".to_string()),
            managed_by_me: self.managed_by_me != 0,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const VIEW_SQL: &str = "SELECT a.id, a.book_id, b.title AS book_title,
       COALESCE((SELECT group_concat(au.name, ', ')
                 FROM book_authors ba JOIN authors au ON au.id = ba.author_id
                 WHERE ba.book_id = b.id), '') AS book_authors,
       a.status, a.preferred_format, a.preferred_language,
       a.selected_release_name, a.selected_release_indexer, a.selected_release_score,
       a.selected_release_confidence, a.selected_release_size, a.selected_release_format,
       a.selected_release_seeders, a.ask_before_download,
       a.download_speed, a.user_id AS requested_by_user_id,
       COALESCE(r.deliver_on_ready, 0) AS my_deliver_on_ready,
       (r.user_id IS NOT NULL) AS requested_by_me,
       CASE WHEN r.deliver_on_ready = 1 THEN r.delivery_address END AS scheduled_delivery_address,
       COALESCE(u.display_name, u.username) AS requested_by, a.download_provider,
       a.error_code, a.error_message, a.progress,
       a.retry_attempts, a.next_retry_at, a.retry_stopped,
       (SELECT d.status FROM deliveries d
         WHERE d.book_id = a.book_id
           AND d.user_id = COALESCE(r.user_id, a.user_id)
          ORDER BY d.id DESC LIMIT 1) AS delivery_status,
       EXISTS (SELECT 1 FROM book_requests br
               WHERE br.acquisition_id = a.id AND br.decided_by = ?) AS managed_by_me,
       a.created_at, a.updated_at
FROM acquisitions a
JOIN books b ON b.id = a.book_id
LEFT JOIN users u ON u.id = a.user_id
LEFT JOIN acquisition_requests r ON r.acquisition_id = a.id AND r.user_id = ?";

#[allow(clippy::too_many_arguments)]
pub async fn create(
    pool: &SqlitePool,
    book_id: i64,
    user_id: Option<i64>,
    preferred_format: Option<String>,
    preferred_language: Option<String>,
    deliver_on_ready: bool,
    ask_before_download: bool,
) -> Result<(Acquisition, bool), AppError> {
    let languages: Vec<String> = preferred_language.iter().cloned().collect();
    create_with_languages(
        pool,
        book_id,
        user_id,
        preferred_format,
        languages,
        deliver_on_ready,
        ask_before_download,
    )
    .await
}

/// Creates or joins an acquisition for a language policy. The ordered
/// languages are frozen onto the acquisition so retries execute the original
/// intent, and duplicate matching only joins an in-flight acquisition whose
/// policy intersects this one.
#[allow(clippy::too_many_arguments)]
pub async fn create_with_languages(
    pool: &SqlitePool,
    book_id: i64,
    user_id: Option<i64>,
    preferred_format: Option<String>,
    languages: Vec<String>,
    deliver_on_ready: bool,
    ask_before_download: bool,
) -> Result<(Acquisition, bool), AppError> {
    // The pre-check runs outside a transaction so an active pipeline writer
    // cannot turn this into a read-then-write snapshot conflict.
    if let Some(existing) = find_duplicate(pool, book_id, &languages).await? {
        if let Some(user_id) = user_id {
            crate::acquisition_requests::register(pool, &existing.id, user_id, deliver_on_ready)
                .await?;
        }
        tracing::info!(acquisition_id = %existing.id, book_id, "acquisition.duplicate");
        return Ok((existing, true));
    }

    let id = Uuid::now_v7().to_string();
    let mut tx = pool.begin().await?;
    let result = insert_acquisition_tx(
        &mut tx,
        &id,
        book_id,
        user_id,
        &preferred_format,
        &languages,
        deliver_on_ready,
        ask_before_download,
    )
    .await;
    if let Err(error) = result {
        let unique = is_unique_violation(&error);
        drop(tx);
        if unique && let Some(existing) = find_duplicate(pool, book_id, &languages).await? {
            // Lost a create race: still register this requester's intent on
            // the shared acquisition, exactly like the normal duplicate path.
            if let Some(user_id) = user_id {
                crate::acquisition_requests::register(
                    pool,
                    &existing.id,
                    user_id,
                    deliver_on_ready,
                )
                .await?;
            }
            tracing::info!(acquisition_id = %existing.id, book_id, "acquisition.duplicate");
            return Ok((existing, true));
        }
        return Err(error);
    }
    tx.commit().await?;
    tracing::info!(acquisition_id = %id, book_id, "acquisition.created");

    let acquisition = get(pool, &id)
        .await?
        .ok_or_else(|| AppError::Unprocessable("acquisition disappeared after insert".into()))?;

    Ok((acquisition, false))
}

/// A direct HTTP request and its retrieval instruction commit together. A
/// restart can therefore never mistake an HTTP acquisition for an indexer job.
#[allow(clippy::too_many_arguments)]
pub async fn create_http_with_languages(
    pool: &SqlitePool,
    book_id: i64,
    user_id: i64,
    preferred_format: Option<String>,
    languages: Vec<String>,
    deliver_on_ready: bool,
    url: &str,
    expected_format: &str,
    source_kind: &str,
    source_name: &str,
    source_key: &str,
    trusted_origin: Option<&str>,
) -> Result<(Acquisition, bool), AppError> {
    let mut tx = pool
        .begin_with(sqlx::AssertSqlSafe("BEGIN IMMEDIATE".to_string()))
        .await?;
    if let Some(existing) = find_duplicate(&mut *tx, book_id, &languages).await? {
        crate::acquisition_requests::register_tx(&mut tx, &existing.id, user_id, deliver_on_ready)
            .await?;
        tx.commit().await?;
        return Ok((existing, true));
    }
    let id = Uuid::now_v7().to_string();
    insert_acquisition_tx(
        &mut tx,
        &id,
        book_id,
        Some(user_id),
        &preferred_format,
        &languages,
        deliver_on_ready,
        false,
    )
    .await?;
    sqlx::query(
        "INSERT INTO acquisition_inputs
         (acquisition_id, method, url, expected_format, source_kind, source_name, source_key, trusted_origin)
         VALUES (?, 'http', ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(url)
    .bind(expected_format)
    .bind(source_kind)
    .bind(source_name)
    .bind(source_key)
    .bind(trusted_origin)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let acquisition = get(pool, &id).await?.ok_or_else(|| {
        AppError::Unprocessable("acquisition disappeared after insert".to_string())
    })?;
    Ok((acquisition, false))
}

/// Creates or joins an acquisition inside the caller's transaction, so a
/// request decision and its acquisition can commit together. The caller has
/// already written in this transaction, so the duplicate pre-check cannot
/// start a read-then-write snapshot conflict. Returns the acquisition id and
/// whether it already existed.
#[allow(clippy::too_many_arguments)]
pub async fn create_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: i64,
    user_id: Option<i64>,
    preferred_format: Option<String>,
    preferred_language: Option<String>,
    deliver_on_ready: bool,
    ask_before_download: bool,
) -> Result<(String, bool), AppError> {
    let languages: Vec<String> = preferred_language.iter().cloned().collect();
    create_tx_with_languages(
        tx,
        book_id,
        user_id,
        preferred_format,
        languages,
        deliver_on_ready,
        ask_before_download,
    )
    .await
}

/// Transactional variant of [`create_with_languages`].
#[allow(clippy::too_many_arguments)]
pub async fn create_tx_with_languages(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    book_id: i64,
    user_id: Option<i64>,
    preferred_format: Option<String>,
    languages: Vec<String>,
    deliver_on_ready: bool,
    ask_before_download: bool,
) -> Result<(String, bool), AppError> {
    if let Some(existing) = find_duplicate(&mut **tx, book_id, &languages).await? {
        if let Some(user_id) = user_id {
            crate::acquisition_requests::register_tx(tx, &existing.id, user_id, deliver_on_ready)
                .await?;
        }
        tracing::info!(acquisition_id = %existing.id, book_id, "acquisition.duplicate");
        return Ok((existing.id, true));
    }

    let id = Uuid::now_v7().to_string();
    match insert_acquisition_tx(
        tx,
        &id,
        book_id,
        user_id,
        &preferred_format,
        &languages,
        deliver_on_ready,
        ask_before_download,
    )
    .await
    {
        Ok(()) => Ok((id, false)),
        Err(error) if is_unique_violation(&error) => {
            if let Some(existing) = find_duplicate(&mut **tx, book_id, &languages).await? {
                if let Some(user_id) = user_id {
                    crate::acquisition_requests::register_tx(
                        tx,
                        &existing.id,
                        user_id,
                        deliver_on_ready,
                    )
                    .await?;
                }
                tracing::info!(acquisition_id = %existing.id, book_id, "acquisition.duplicate");
                return Ok((existing.id, true));
            }
            Err(error)
        }
        Err(error) => Err(error),
    }
}

#[allow(clippy::too_many_arguments)]
async fn insert_acquisition_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    book_id: i64,
    user_id: Option<i64>,
    preferred_format: &Option<String>,
    languages: &[String],
    deliver_on_ready: bool,
    ask_before_download: bool,
) -> Result<(), AppError> {
    let languages = normalize_languages(languages);
    let preferred_language = languages.first();
    let serialized = serde_json::to_string(&languages)
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    sqlx::query(
        "INSERT INTO acquisitions
            (id, book_id, user_id, preferred_format, preferred_language,
             acquisition_languages, language_key, ask_before_download, status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'REQUESTED')",
    )
    .bind(id)
    .bind(book_id)
    .bind(user_id)
    .bind(preferred_format)
    .bind(preferred_language)
    .bind(&serialized)
    .bind(language_key(&languages))
    .bind(ask_before_download)
    .execute(&mut **tx)
    .await?;

    if let Some(user_id) = user_id {
        crate::acquisition_requests::register_tx(tx, id, user_id, deliver_on_ready).await?;
    }
    insert_event_tx(tx, id, "acquisition.created", None).await?;
    Ok(())
}

fn is_unique_violation(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Internal(sqlx::Error::Database(database_error))
            if database_error.is_unique_violation()
    )
}

/// The canonical identity of a language policy: lowercased, trimmed, sorted
/// and deduped, so equivalent sets collide in the partial unique index.
pub fn language_key(languages: &[String]) -> String {
    normalize_languages(languages).join(",")
}

fn normalize_languages(languages: &[String]) -> Vec<String> {
    let mut values: Vec<String> = languages
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect();
    values.sort();
    values.dedup();
    values
}

/// Parses the frozen language list stored on an acquisition. `None` means a
/// legacy row that predates the column.
pub fn stored_languages(acquisition: &Acquisition) -> Option<Vec<String>> {
    acquisition.acquisition_languages.as_deref().map(|raw| {
        normalize_languages(&serde_json::from_str::<Vec<String>>(raw).unwrap_or_default())
    })
}

/// Empty means "any language", so it intersects everything.
fn languages_intersect(left: &[String], right: &[String]) -> bool {
    if left.is_empty() || right.is_empty() {
        return true;
    }
    left.iter()
        .any(|value| right.iter().any(|other| other.eq_ignore_ascii_case(value)))
}

async fn find_duplicate<'e, E>(
    executor: E,
    book_id: i64,
    languages: &[String],
) -> Result<Option<Acquisition>, AppError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let query = format!(
        "SELECT {SELECT_COLUMNS} FROM acquisitions
         WHERE book_id = ? AND status IN ({})
         ORDER BY created_at DESC",
        protected_states_sql()
    );

    let candidates: Vec<Acquisition> = sqlx::query_as(sqlx::AssertSqlSafe(query))
        .bind(book_id)
        .fetch_all(executor)
        .await?;
    let wanted = normalize_languages(languages);
    Ok(candidates.into_iter().find(|acquisition| {
        languages_intersect(&wanted, &stored_languages(acquisition).unwrap_or_default())
    }))
}

pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<Acquisition>, AppError> {
    let acquisition: Option<Acquisition> = sqlx::query_as(GET_SQL)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(acquisition)
}

pub async fn view(
    pool: &SqlitePool,
    viewer_id: i64,
    id: &str,
) -> Result<Option<AcquisitionView>, AppError> {
    let row: Option<AcquisitionRow> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{VIEW_SQL} WHERE a.id = ?")))
            .bind(viewer_id)
            .bind(viewer_id)
            .bind(id)
            .fetch_optional(pool)
            .await?;
    row.map(AcquisitionRow::into_view).transpose()
}

pub async fn list(
    pool: &SqlitePool,
    viewer_id: i64,
    limit: i64,
    household: bool,
    include_approved: bool,
) -> Result<Vec<AcquisitionView>, AppError> {
    // Personal by default: acquisitions the viewer asked for. Administrators
    // also see work they approved and can request the household view.
    let filter = if household {
        ""
    } else if !include_approved {
        "WHERE EXISTS (
             SELECT 1 FROM acquisition_requests ar2
             WHERE ar2.acquisition_id = a.id AND ar2.user_id = ?
         )"
    } else {
        "WHERE EXISTS (
             SELECT 1 FROM acquisition_requests ar2
             WHERE ar2.acquisition_id = a.id AND ar2.user_id = ?
         ) OR EXISTS (
             SELECT 1 FROM book_requests br2
             WHERE br2.acquisition_id = a.id AND br2.decided_by = ?
         )"
    };
    let mut query = sqlx::query_as::<_, AcquisitionRow>(sqlx::AssertSqlSafe(format!(
        "{VIEW_SQL} {filter} ORDER BY a.created_at DESC, a.id DESC LIMIT ?"
    )))
    .bind(viewer_id);
    query = query.bind(viewer_id);
    if !household {
        query = query.bind(viewer_id);
        if include_approved {
            query = query.bind(viewer_id);
        }
    }
    let rows = query.bind(limit.clamp(1, 200)).fetch_all(pool).await?;
    let mut views: Vec<AcquisitionView> = rows
        .into_iter()
        .map(AcquisitionRow::into_view)
        .collect::<Result<_, _>>()?;

    // Why Bokhylle acted, per user: extra requests are silent, authored
    // automation says so.
    let provenance: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT ar.acquisition_id, ar.source, a.name
         FROM acquisition_requests ar
         LEFT JOIN authors a ON a.id = ar.source_author_id
         WHERE ar.user_id = ?",
    )
    .bind(viewer_id)
    .fetch_all(pool)
    .await?;
    for (acquisition_id, source, author) in provenance {
        if source != "manual"
            && let Some(view) = views.iter_mut().find(|view| view.id == acquisition_id)
        {
            view.source = Some(source);
            view.source_author = author;
        }
    }

    Ok(views)
}

/// Book pages need all of their relevant work, independent of Activity's
/// pagination. Other profiles' requests are visible only to administrators.
pub async fn list_for_book(
    pool: &SqlitePool,
    viewer_id: i64,
    book_id: i64,
    admin: bool,
) -> Result<Vec<AcquisitionView>, AppError> {
    let rows: Vec<AcquisitionRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{VIEW_SQL} WHERE a.book_id = ?
         AND (? OR a.user_id = ? OR r.user_id IS NOT NULL)
         ORDER BY a.created_at DESC, a.id DESC"
    )))
    .bind(viewer_id)
    .bind(viewer_id)
    .bind(book_id)
    .bind(admin)
    .bind(viewer_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(AcquisitionRow::into_view).collect()
}

pub async fn transition(
    pool: &SqlitePool,
    id: &str,
    next: AcquisitionStatus,
    detail: Option<Value>,
) -> Result<Acquisition, AppError> {
    let current = get(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;

    let from = current.status()?;
    if from == next {
        return Ok(current);
    }

    validate_transition(from, next).map_err(|error| AppError::Conflict(error.to_string()))?;

    let mut tx = pool.begin().await?;
    transition_tx(&mut tx, id, from, next, detail).await?;
    tx.commit().await?;

    tracing::info!(
        acquisition_id = %id,
        from = from.as_str(),
        to = next.as_str(),
        "acquisition.status.changed"
    );

    get(pool, id)
        .await?
        .ok_or_else(|| AppError::Unprocessable("acquisition disappeared after transition".into()))
}

/// The compare-and-swap transition plus its event, usable inside a larger
/// transaction so a state change cannot commit without its bookkeeping.
pub(crate) async fn transition_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    from: AcquisitionStatus,
    next: AcquisitionStatus,
    detail: Option<Value>,
) -> Result<(), AppError> {
    validate_transition(from, next).map_err(|error| AppError::Conflict(error.to_string()))?;
    let updated = sqlx::query(
        "UPDATE acquisitions SET status = ?, updated_at = unixepoch() WHERE id = ? AND status = ?",
    )
    .bind(next.as_str())
    .bind(id)
    .bind(from.as_str())
    .execute(&mut **tx)
    .await?;

    if updated.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "the acquisition state changed concurrently".to_string(),
        ));
    }

    insert_event_tx(tx, id, "acquisition.status.changed", detail).await
}

/// Flags an acquisition whose external download could not be removed, so the
/// tracker retries and Needs Attention shows it if it stays stuck.
pub async fn mark_cancel_pending(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE acquisitions SET cancel_pending = 1, updated_at = unixepoch() WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_cancel_pending(pool: &SqlitePool, id: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE acquisitions SET cancel_pending = 0 WHERE id = ? AND cancel_pending != 0")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Cancelled acquisitions whose client-side download still needs removing.
pub async fn pending_cancellations(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<(String, Option<String>)>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, provider_download_id FROM acquisitions
         WHERE status = 'CANCELLED' AND cancel_pending = 1
         ORDER BY updated_at
         LIMIT ?",
    )
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await?)
}

pub async fn cancel(pool: &SqlitePool, id: &str) -> Result<Acquisition, AppError> {
    let current = get(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;

    if current.status()? == AcquisitionStatus::Cancelled {
        return Ok(current);
    }

    let mut tx = pool.begin().await?;
    transition_tx(
        &mut tx,
        id,
        current.status()?,
        AcquisitionStatus::Cancelled,
        None,
    )
    .await?;
    // The add response may not have been saved yet. Persist cancellation
    // intent with the state change so the SAB tracker can recover by name.
    sqlx::query("UPDATE acquisitions SET cancel_pending = 1 WHERE id = ? AND download_provider = 'sabnzbd' AND content_path IS NULL")
        .bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    get(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".into()))
}

/// Reads a persisted candidate list. Entries that no longer match the current
/// schema are skipped so old events keep working; every caller reads through
/// here, so candidate indices stay consistent between listing and selecting.
pub fn evaluated_candidates(detail: &Value) -> Vec<EvaluatedRelease> {
    detail
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|candidate| match serde_json::from_value(candidate) {
            Ok(release) => Some(release),
            Err(error) => {
                tracing::warn!(%error, "candidates.skipped_unreadable");
                None
            }
        })
        .collect()
}

pub async fn events(
    pool: &SqlitePool,
    id: &str,
) -> Result<Vec<(String, Option<String>, i64)>, AppError> {
    let rows: Vec<(String, Option<String>, i64)> = sqlx::query_as(
        "SELECT event, detail, created_at FROM acquisition_events
         WHERE acquisition_id = ? ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub(crate) async fn insert_event_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    acquisition_id: &str,
    event: &str,
    detail: Option<Value>,
) -> Result<(), AppError> {
    let detail = detail
        .map(|value| serde_json::to_string(&value))
        .transpose()
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;

    sqlx::query("INSERT INTO acquisition_events (acquisition_id, event, detail) VALUES (?, ?, ?)")
        .bind(acquisition_id)
        .bind(event)
        .bind(detail)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

async fn insert_event(
    pool: &SqlitePool,
    acquisition_id: &str,
    event: &str,
    detail: Option<Value>,
) -> Result<(), AppError> {
    let detail = detail
        .map(|value| serde_json::to_string(&value))
        .transpose()
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;

    sqlx::query("INSERT INTO acquisition_events (acquisition_id, event, detail) VALUES (?, ?, ?)")
        .bind(acquisition_id)
        .bind(event)
        .bind(detail)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn set_speed(pool: &SqlitePool, id: &str, speed: Option<i64>) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE acquisitions SET download_speed = ?, updated_at = unixepoch() WHERE id = ?",
    )
    .bind(speed)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Error codes that mean the release itself is unusable. Infrastructure or
/// path problems (missing mounts, permissions, temporary IO) are retryable
/// and must not poison a release permanently.
const BLOCKABLE_IMPORT_ERRORS: &[&str] = &[
    "no_supported_files",
    "unsupported_files",
    "corrupt_archive",
    "wrong_content",
    "digest_mismatch",
];

/// Re-opens a failed import for inspection so a person can choose a file
/// from the download (transitions to Downloaded, where the import pipeline
/// re-inspects and falls into review when the pick is ambiguous).
pub async fn inspect_files(pool: &SqlitePool, id: &str) -> Result<Acquisition, AppError> {
    let current = get(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;
    if current.status()? != AcquisitionStatus::ImportFailed {
        return Err(AppError::Unprocessable(
            "only failed imports can be inspected".to_string(),
        ));
    }
    sqlx::query("UPDATE acquisitions SET error_code = NULL, error_message = NULL, updated_at = unixepoch() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    transition(
        pool,
        id,
        AcquisitionStatus::Downloaded,
        Some(serde_json::json!({ "manualInspection": true })),
    )
    .await
}

fn is_failed(status: AcquisitionStatus) -> bool {
    matches!(
        status,
        AcquisitionStatus::NoReleaseFound
            | AcquisitionStatus::DownloadFailed
            | AcquisitionStatus::ImportFailed
    )
}

/// Re-opens a failed acquisition so the pipeline can search again, inside the
/// caller's transaction. The selected release is cleared (its fingerprint is
/// already blocklisted on import failure) and any stale pending placement is
/// dropped, so a failure cannot leave a half-reopened acquisition behind.
async fn reopen_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    from: AcquisitionStatus,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM pending_imports WHERE acquisition_id = ?")
        .bind(id)
        .execute(&mut **tx)
        .await?;

    sqlx::query(
        "UPDATE acquisitions SET
            error_code = NULL,
            error_message = NULL,
            selected_release_name = NULL,
            selected_release_indexer = NULL,
            selected_release_key = NULL,
            selected_release_score = NULL,
            selected_release_confidence = NULL,
            selected_release_size = NULL,
            selected_release_format = NULL,
            selected_release_seeders = NULL,
            provider_download_id = NULL,
            download_provider = NULL,
            content_path = NULL,
            progress = 0,
            download_speed = NULL,
            cancel_pending = 0,
            updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;

    transition_tx(
        tx,
        id,
        from,
        AcquisitionStatus::Requested,
        Some(serde_json::json!({ "retried": true })),
    )
    .await
}

/// Manual "Try again now": re-opens the acquisition and continues the backoff
/// series in one transaction, so the pipeline can always be started on a
/// consistent, reopened state.
pub async fn retry(pool: &SqlitePool, id: &str) -> Result<Acquisition, AppError> {
    let current = get(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".to_string()))?;
    let status = current.status()?;
    if !is_failed(status) {
        return Err(AppError::Unprocessable(
            "only failed requests can be tried again".to_string(),
        ));
    }

    let mut tx = pool.begin().await?;
    // A manual attempt continues the backoff series instead of resetting it.
    sqlx::query(
        "UPDATE acquisitions SET
            retry_stopped = 0,
            next_retry_at = NULL,
            retry_attempts = retry_attempts + 1,
            retry_started_at = COALESCE(retry_started_at, unixepoch()),
            updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    reopen_tx(&mut tx, id, status).await?;
    tx.commit().await?;

    get(pool, id)
        .await?
        .ok_or_else(|| AppError::Unprocessable("acquisition disappeared after retry".into()))
}

/// Automatic claim: consumes one retry attempt and re-opens the acquisition
/// in the same transaction, so a failed reopen cannot strand a cleared
/// `next_retry_at` and lose the retry intent.
pub async fn claim_retry(pool: &SqlitePool, id: &str) -> Result<bool, AppError> {
    let Some(current) = get(pool, id).await? else {
        return Ok(false);
    };
    let status = current.status()?;
    if !is_failed(status) {
        return Ok(false);
    }

    let mut tx = pool.begin().await?;
    let claimed = sqlx::query(
        "UPDATE acquisitions
         SET retry_attempts = retry_attempts + 1, next_retry_at = NULL, updated_at = unixepoch()
         WHERE id = ? AND retry_stopped = 0
           AND next_retry_at IS NOT NULL AND next_retry_at <= unixepoch()",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    if claimed.rows_affected() == 0 {
        tx.rollback().await.ok();
        return Ok(false);
    }
    reopen_tx(&mut tx, id, status).await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn block_release(
    pool: &SqlitePool,
    release_key: &str,
    release_name: &str,
    indexer: Option<&str>,
    reason: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO release_blocklist (release_key, release_name, indexer, reason)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(release_key) DO UPDATE SET
            reason = excluded.reason,
            created_at = unixepoch()",
    )
    .bind(release_key)
    .bind(release_name)
    .bind(indexer)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn blocked_release_keys(
    pool: &SqlitePool,
) -> Result<std::collections::HashSet<String>, AppError> {
    let keys: Vec<String> = sqlx::query_scalar("SELECT release_key FROM release_blocklist")
        .fetch_all(pool)
        .await?;
    Ok(keys.into_iter().collect())
}

pub async fn set_selected(
    pool: &SqlitePool,
    id: &str,
    release: &bokhylle_acquisition::model::EvaluatedRelease,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    set_selected_tx(&mut tx, id, release).await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn set_selected_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
    release: &bokhylle_acquisition::model::EvaluatedRelease,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE acquisitions SET
            selected_release_name = ?,
            selected_release_indexer = ?,
            selected_release_score = ?,
            selected_release_confidence = ?,
            selected_release_size = ?,
            selected_release_format = ?,
            selected_release_seeders = ?,
            selected_release_key = ?,
            updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(&release.candidate.title)
    .bind(&release.candidate.indexer)
    .bind(release.score as i64)
    .bind(f64::from(release.confidence))
    .bind(release.candidate.size_bytes)
    .bind(&release.candidate.detected_format)
    .bind(release.candidate.seeders)
    .bind(bokhylle_acquisition::evaluator::release_key(
        &release.candidate,
    ))
    .bind(id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn set_provider(
    pool: &SqlitePool,
    id: &str,
    provider: &str,
    provider_download_id: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE acquisitions SET
            download_provider = ?,
            provider_download_id = ?,
            updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(provider)
    .bind(provider_download_id)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn fail(
    pool: &SqlitePool,
    id: &str,
    error_code: &str,
    error_message: &str,
) -> Result<Acquisition, AppError> {
    set_error_fields(pool, id, error_code, error_message).await?;
    crate::notifications::for_requesters(
        pool,
        id,
        "failed",
        "Download failed",
        Some(error_message),
    )
    .await
    .ok();
    tracing::warn!(acquisition_id = %id, error_code, error_message, "acquisition.failed");
    let failed = transition(
        pool,
        id,
        AcquisitionStatus::DownloadFailed,
        Some(serde_json::json!({ "errorCode": error_code })),
    )
    .await?;
    crate::keep_looking::schedule_after_failure(pool, id)
        .await
        .ok();
    Ok(failed)
}

pub async fn fail_import(
    pool: &SqlitePool,
    id: &str,
    error_code: &str,
    error_message: &str,
) -> Result<Acquisition, AppError> {
    set_error_fields(pool, id, error_code, error_message).await?;
    // A failed import usually means the release contained the wrong or broken
    // book; remember the fingerprint so the next attempt picks another one.
    if BLOCKABLE_IMPORT_ERRORS.contains(&error_code)
        && let Ok(Some((Some(key), name, indexer))) = sqlx::query_as::<
            _,
            (Option<String>, String, Option<String>),
        >(
            "SELECT selected_release_key, COALESCE(selected_release_name, ''), selected_release_indexer
             FROM acquisitions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        && !key.is_empty()
    {
        block_release(pool, &key, &name, indexer.as_deref(), error_message)
            .await
            .ok();
    }
    crate::notifications::for_requesters(pool, id, "failed", "Import failed", Some(error_message))
        .await
        .ok();
    tracing::warn!(acquisition_id = %id, error_code, error_message, "import.failed");
    let failed = transition(
        pool,
        id,
        AcquisitionStatus::ImportFailed,
        Some(serde_json::json!({ "errorCode": error_code })),
    )
    .await?;
    crate::keep_looking::schedule_after_failure(pool, id)
        .await
        .ok();
    Ok(failed)
}

async fn set_error_fields(
    pool: &SqlitePool,
    id: &str,
    error_code: &str,
    error_message: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE acquisitions SET error_code = ?, error_message = ?, updated_at = unixepoch()
         WHERE id = ?",
    )
    .bind(error_code)
    .bind(error_message)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn log_event(
    pool: &SqlitePool,
    acquisition_id: &str,
    event: &str,
    detail: Option<Value>,
) -> Result<(), AppError> {
    insert_event(pool, acquisition_id, event, detail).await
}

pub async fn latest_event_detail(
    pool: &SqlitePool,
    acquisition_id: &str,
    event: &str,
) -> Result<Option<Value>, AppError> {
    let detail: Option<Option<String>> = sqlx::query_scalar(
        "SELECT detail FROM acquisition_events
         WHERE acquisition_id = ? AND event = ?
         ORDER BY id DESC
         LIMIT 1",
    )
    .bind(acquisition_id)
    .bind(event)
    .fetch_optional(pool)
    .await?;

    let Some(detail) = detail.flatten() else {
        return Ok(None);
    };

    serde_json::from_str(&detail)
        .map(Some)
        .map_err(|error| AppError::Unprocessable(format!("invalid event detail: {error}")))
}

pub async fn set_progress(pool: &SqlitePool, id: &str, progress: f64) -> Result<(), AppError> {
    sqlx::query("UPDATE acquisitions SET progress = ?, updated_at = unixepoch() WHERE id = ?")
        .bind(progress.clamp(0.0, 100.0))
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}
