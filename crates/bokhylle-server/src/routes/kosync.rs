use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use md5::Digest;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::AppState;
use crate::auth::User;

/// kosync errors carry the protocol's numeric code so KOReader can tell a
/// permanent credential failure (401) from a retryable one.
#[derive(Debug)]
pub enum KosyncError {
    Unauthorized,
    UsernameTaken,
    InvalidRequest,
    DocumentMissing,
    Malformed,
    NotObject,
    Internal(String),
}

impl KosyncError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::UsernameTaken => StatusCode::PAYMENT_REQUIRED,
            Self::InvalidRequest | Self::DocumentMissing => StatusCode::FORBIDDEN,
            Self::Malformed | Self::NotObject => StatusCode::BAD_REQUEST,
            Self::Internal(_) => StatusCode::BAD_GATEWAY,
        }
    }

    fn code(&self) -> i64 {
        match self {
            Self::Unauthorized => 2001,
            Self::UsernameTaken => 2002,
            Self::InvalidRequest => 2003,
            Self::DocumentMissing => 2004,
            Self::Malformed => 103,
            Self::NotObject => 104,
            Self::Internal(_) => 2000,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::Unauthorized => "Unauthorized".to_string(),
            Self::UsernameTaken => "Username is already registered.".to_string(),
            Self::InvalidRequest => "Invalid request".to_string(),
            Self::DocumentMissing => "Field 'document' not provided.".to_string(),
            Self::Malformed => "Could not parse JSON in body.".to_string(),
            Self::NotObject => "Body should be a JSON hash.".to_string(),
            Self::Internal(error) => {
                tracing::error!(error = %error, "kosync internal error");
                "Unknown server error.".to_string()
            }
        }
    }
}

/// The reference server decodes the raw body and never inspects Content-Type,
/// so kosync routes read bytes and parse JSON themselves.
fn parse_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, KosyncError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| KosyncError::Malformed)?;
    if !value.is_object() {
        return Err(KosyncError::NotObject);
    }
    serde_json::from_value(value).map_err(|_| KosyncError::InvalidRequest)
}

impl IntoResponse for KosyncError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = json!({ "message": self.message(), "code": self.code() });
        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for KosyncError {
    fn from(error: sqlx::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<crate::error::AppError> for KosyncError {
    fn from(error: crate::error::AppError) -> Self {
        Self::Internal(error.to_string())
    }
}

async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<User, KosyncError> {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    let username = header("x-auth-user");
    let key = header("x-auth-key");
    if username.is_empty() || username.contains(':') || key.is_empty() {
        return Err(KosyncError::Unauthorized);
    }
    crate::reader_tokens::authenticate_sync(&state.db, &key, &username)
        .await?
        .ok_or(KosyncError::Unauthorized)
}

fn valid_document(document: &str) -> bool {
    !document.is_empty() && !document.contains(':')
}

#[derive(Debug, Deserialize)]
pub struct Credentials {
    username: Option<String>,
    password: Option<String>,
}

pub async fn healthcheck() -> Json<Value> {
    Json(json!({ "state": "OK" }))
}

/// kosync self-registration. A reader token is the account's credential and
/// the username is bound to it on first registration, so later logins must use
/// the same name.
pub async fn create_user(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Response, KosyncError> {
    let body: Credentials = parse_body(&body)?;
    let username = body.username.unwrap_or_default();
    let password = body.password.unwrap_or_default();
    if username.is_empty() || username.contains(':') || password.is_empty() {
        return Err(KosyncError::InvalidRequest);
    }
    match crate::reader_tokens::register_sync(&state.db, &password, &username).await {
        Ok(true) => {}
        Ok(false) => return Err(KosyncError::InvalidRequest),
        Err(crate::error::AppError::Conflict(_)) => return Err(KosyncError::UsernameTaken),
        Err(error) => return Err(error.into()),
    }
    Ok((StatusCode::CREATED, Json(json!({ "username": username }))).into_response())
}

pub async fn auth(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, KosyncError> {
    authorize(&state, &headers).await?;
    Ok(Json(json!({ "authorized": "OK" })))
}

#[derive(Debug, Deserialize)]
pub struct ProgressBody {
    document: Option<String>,
    progress: Option<Value>,
    percentage: Option<Value>,
    device: Option<String>,
    device_id: Option<String>,
    #[allow(dead_code)]
    metadata: Option<Value>,
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

fn progress_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

async fn accessible_document(
    state: &AppState,
    user: &User,
    document: &str,
) -> Result<Option<(i64, i64)>, KosyncError> {
    let resolved = resolve_document(state, document).await?;
    if let Some((book_id, _)) = resolved
        && !crate::services::sharing::can_access(&state.db, user.id, book_id).await?
    {
        return Err(KosyncError::InvalidRequest);
    }
    if crate::auth::profile_type(&state.db, user.id).await? == "child" {
        let Some((book_id, _)) = resolved else {
            return Err(KosyncError::InvalidRequest);
        };
        if !crate::user_books::contains(&state.db, user.id, book_id).await? {
            return Err(KosyncError::InvalidRequest);
        }
    }
    Ok(resolved)
}

pub async fn put_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, KosyncError> {
    let user = authorize(&state, &headers).await?;
    let body: ProgressBody = parse_body(&body)?;
    let document = body.document.unwrap_or_default();
    if !valid_document(&document) {
        return Err(KosyncError::DocumentMissing);
    }
    let percentage = number(body.percentage.as_ref()).filter(|value| value.is_finite());
    let Some(percentage) = percentage else {
        return Err(KosyncError::InvalidRequest);
    };
    let Some(locator) = body.progress.as_ref().and_then(progress_text) else {
        return Err(KosyncError::InvalidRequest);
    };
    let Some(device) = body.device else {
        return Err(KosyncError::InvalidRequest);
    };

    let resolved = accessible_document(&state, &user, &document).await?;
    let (book_id, file_id) = resolved.map_or((None, None), |(book_id, file_id)| {
        (Some(book_id), Some(file_id))
    });

    sqlx::query(
        "INSERT INTO reading_progress
             (user_id, document, book_id, book_file_id, percentage, locator, source, source_device, device_id)
         VALUES (?, ?, ?, ?, ?, ?, 'koreader', ?, ?)
         ON CONFLICT(user_id, document) DO UPDATE SET
             book_id = excluded.book_id,
             book_file_id = excluded.book_file_id,
             percentage = excluded.percentage,
             locator = excluded.locator,
             source = excluded.source,
             source_device = excluded.source_device,
             device_id = excluded.device_id,
             revision = reading_progress.revision + 1,
             updated_at = unixepoch()",
    )
    .bind(user.id)
    .bind(&document)
    .bind(book_id)
    .bind(file_id)
    .bind(percentage)
    .bind(&locator)
    .bind(&device)
    .bind(body.device_id.as_deref())
    .execute(&state.db)
    .await?;

    let timestamp: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&state.db)
        .await?;
    Ok(Json(
        json!({ "document": document, "timestamp": timestamp }),
    ))
}

pub async fn get_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document): Path<String>,
) -> Result<Json<Value>, KosyncError> {
    let user = authorize(&state, &headers).await?;
    if !valid_document(&document) {
        return Err(KosyncError::DocumentMissing);
    }
    accessible_document(&state, &user, &document).await?;
    type StoredProgress = (f64, String, Option<String>, Option<String>, i64);
    let row: Option<StoredProgress> = sqlx::query_as(
        "SELECT percentage, locator, source_device, device_id, updated_at
         FROM reading_progress WHERE user_id = ? AND document = ?",
    )
    .bind(user.id)
    .bind(&document)
    .fetch_optional(&state.db)
    .await?;

    let Some((percentage, locator, device, device_id, timestamp)) = row else {
        return Ok(Json(json!({})));
    };
    let mut body = json!({
        "document": document,
        "percentage": percentage,
        "progress": locator,
        "device": device,
        "timestamp": timestamp,
    });
    if let Some(device_id) = device_id {
        body["device_id"] = Value::String(device_id);
    }
    Ok(Json(body))
}

/// KOReader's document id: twelve 1024-byte samples at exponentially spaced
/// offsets, hashed once. The first offset is 0 (LuaJIT masks the shift count,
/// so `lshift(1024, -2)` overflows to 0), and the loop stops at the first
/// offset beyond EOF.
pub fn partial_md5(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut hasher = md5::Md5::new();
    let mut buffer = [0u8; 1024];
    for index in -1..=10i32 {
        let shift = ((index * 2) & 0x1F) as u32;
        let offset = 1024u32.wrapping_shl(shift) as u64;
        if offset >= length {
            break;
        }
        file.seek(SeekFrom::Start(offset)).ok()?;
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hex::encode(hasher.finalize()))
}

fn filename_digest(filename: &str) -> String {
    hex::encode(md5::Md5::digest(filename.as_bytes()))
}

/// Resolves an opaque document id to a library file: a name digest first
/// (cheap), then KOReader's partial MD5 over each file. Unmatched documents
/// are negatively cached so a reader that never syncs a known book does not
/// re-scan the library on every push.
async fn resolve_document(
    state: &AppState,
    document: &str,
) -> Result<Option<(i64, i64)>, KosyncError> {
    let cached: Option<(Option<i64>, Option<i64>, i64)> = sqlx::query_as(
        "SELECT book_id, book_file_id, checked_at FROM kosync_documents WHERE document = ?",
    )
    .bind(document)
    .fetch_optional(&state.db)
    .await?;
    if let Some((book_id, file_id, checked_at)) = cached {
        if let (Some(book_id), Some(file_id)) = (book_id, file_id) {
            return Ok(Some((book_id, file_id)));
        }
        let age: i64 = sqlx::query_scalar("SELECT unixepoch() - ?")
            .bind(checked_at)
            .fetch_one(&state.db)
            .await?;
        if age < 7 * 24 * 3600 {
            return Ok(None);
        }
    }

    let candidates: Vec<(i64, i64, String, String, String)> = sqlx::query_as(
        "SELECT f.id, e.book_id, f.path, b.title, f.format
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         JOIN books b ON b.id = e.book_id
         ORDER BY f.id",
    )
    .fetch_all(&state.db)
    .await?;
    let names: Vec<(i64, i64, String)> = candidates
        .iter()
        .flat_map(|(file_id, book_id, path, title, format)| {
            let basename = path.rsplit('/').next().unwrap_or(path).to_string();
            [
                (*file_id, *book_id, basename),
                (
                    *file_id,
                    *book_id,
                    crate::routes::opds::download_filename(title, format),
                ),
            ]
        })
        .collect();
    if let Some((file_id, book_id)) = names
        .iter()
        .find(|(_, _, name)| filename_digest(name) == document)
        .map(|(file_id, book_id, _)| (*file_id, *book_id))
    {
        cache_document(&state.db, document, Some((book_id, file_id))).await?;
        return Ok(Some((book_id, file_id)));
    }

    let probe: Vec<(i64, i64, String)> = candidates
        .iter()
        .map(|(file_id, book_id, path, _, _)| (*file_id, *book_id, path.clone()))
        .collect();
    let target = document.to_string();
    let matched = tokio::task::spawn_blocking(move || {
        probe.into_iter().find(|(_, _, path)| {
            partial_md5(std::path::Path::new(path)).as_deref() == Some(target.as_str())
        })
    })
    .await
    .map_err(|error| KosyncError::Internal(error.to_string()))?;

    let found = matched.map(|(file_id, book_id, _)| (book_id, file_id));
    cache_document(&state.db, document, found).await?;
    Ok(found)
}

async fn cache_document(
    pool: &sqlx::SqlitePool,
    document: &str,
    found: Option<(i64, i64)>,
) -> Result<(), KosyncError> {
    let (book_id, file_id) = match found {
        Some((book_id, file_id)) => (Some(book_id), Some(file_id)),
        None => (None, None),
    };
    sqlx::query(
        "INSERT INTO kosync_documents (document, book_id, book_file_id)
         VALUES (?, ?, ?)
         ON CONFLICT(document) DO UPDATE SET
             book_id = excluded.book_id,
             book_file_id = excluded.book_file_id,
             checked_at = unixepoch()",
    )
    .bind(document)
    .bind(book_id)
    .bind(file_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_is_at_offset_zero_and_the_loop_stops_at_eof() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("probe.bin");
        let mut bytes = vec![b'A'; 1024];
        bytes.extend(vec![b'B'; 1024]);
        std::fs::write(&path, &bytes).unwrap();

        let mut expected = md5::Md5::new();
        expected.update(&bytes);
        assert_eq!(
            partial_md5(&path).unwrap(),
            hex::encode(expected.finalize()),
            "a 2 KiB file hashes both samples at offsets 0 and 1024"
        );

        let mut short = md5::Md5::new();
        short.update(vec![0u8; 2048]);
        let zeros = dir.path().join("zeros.bin");
        std::fs::write(&zeros, vec![0u8; 4096]).unwrap();
        assert_eq!(
            partial_md5(&zeros).unwrap(),
            hex::encode(short.finalize()),
            "a 4 KiB file stops before offset 4096"
        );
    }
}
