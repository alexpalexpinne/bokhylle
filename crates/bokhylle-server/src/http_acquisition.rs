//! Direct HTTP retrieval. A durable `acquisition_inputs` row owns the URL;
//! public events and diagnostics contain only its source identity.

use std::path::{Path, PathBuf};

use bokhylle_acquisition::state::AcquisitionStatus;
use bokhylle_importer::Limits;
use sqlx::FromRow;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{AppState, acquisition, error::AppError, remote_http};

#[derive(FromRow)]
pub struct HttpInput {
    pub url: String,
    pub expected_format: String,
    pub source_kind: String,
    pub source_name: String,
    pub source_key: String,
    pub trusted_origin: Option<String>,
}

pub async fn input(state: &AppState, id: &str) -> Result<Option<HttpInput>, AppError> {
    Ok(sqlx::query_as(
        "SELECT url, expected_format, source_kind, source_name, source_key, trusted_origin
         FROM acquisition_inputs WHERE acquisition_id = ? AND method = 'http'",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?)
}

pub async fn run(state: &AppState, id: &str, input: HttpInput) -> Result<(), AppError> {
    let result = retrieve(state, id, &input).await;
    if let Err(error) = result {
        if is_cancelled(state, id).await? {
            return Ok(());
        }
        // Remote URLs may contain signed query parameters. Never put the
        // lower-level request error (which can include the URL) in the journal.
        tracing::warn!(acquisition_id = %id, "acquisition.http.failed");
        acquisition::fail(
            &state.db,
            id,
            "download_failed",
            &format!("direct download failed: {error}"),
        )
        .await?;
    }
    Ok(())
}

async fn retrieve(state: &AppState, id: &str, input: &HttpInput) -> Result<(), AppError> {
    let Some(acquisition) = acquisition::get(&state.db, id).await? else {
        return Ok(());
    };
    let mut status = acquisition.status()?;
    if !matches!(
        status,
        AcquisitionStatus::Requested
            | AcquisitionStatus::Searching
            | AcquisitionStatus::Evaluating
            | AcquisitionStatus::Queued
            | AcquisitionStatus::Downloading
    ) {
        return Ok(());
    }
    acquisition::set_provider(&state.db, id, "http", None).await?;
    for next in [
        AcquisitionStatus::Searching,
        AcquisitionStatus::Evaluating,
        AcquisitionStatus::Queued,
        AcquisitionStatus::Downloading,
    ] {
        if status == next {
            continue;
        }
        if status.can_transition_to(next) {
            acquisition::transition(&state.db, id, next, None).await?;
            status = next;
        }
    }
    sqlx::query(
        "UPDATE acquisitions SET selected_release_name = ?, selected_release_indexer = ?, 
         selected_release_format = ?, updated_at = unixepoch() WHERE id = ?",
    )
    .bind(&input.source_name)
    .bind(&input.source_name)
    .bind(&input.expected_format)
    .bind(id)
    .execute(&state.db)
    .await?;

    let directory = state.paths.downloads_dir.join("http").join(id);
    tokio::fs::create_dir_all(&directory).await?;
    if !crate::paths::is_within(&state.paths.downloads_dir, &directory) {
        return Err(AppError::Unprocessable(
            "HTTP staging path is outside the downloads directory".to_string(),
        ));
    }
    let title: String = sqlx::query_scalar("SELECT title FROM books WHERE id = ?")
        .bind(acquisition.book_id)
        .fetch_one(&state.db)
        .await?;
    let filename = format!("{}.{}", safe_filename(&title), input.expected_format);
    let target = directory.join(filename);
    let partial = directory.join(".download.partial");
    let _ = tokio::fs::remove_file(&partial).await;
    let result = download_to(state, id, input, &partial).await;
    if result.is_err() || result.as_ref().is_ok_and(|finished| !finished) {
        let _ = tokio::fs::remove_file(&partial).await;
        return result.map(|_| ());
    }
    if let Err(error) = verify_file(&partial, &input.expected_format).await {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(error);
    }
    if is_cancelled(state, id).await? {
        let _ = tokio::fs::remove_file(&partial).await;
        return Ok(());
    }
    tokio::fs::rename(&partial, &target).await?;
    if !crate::paths::is_within(&state.paths.downloads_dir, &target) {
        return Err(AppError::Unprocessable(
            "HTTP artifact is outside the downloads directory".to_string(),
        ));
    }
    sqlx::query("UPDATE acquisitions SET content_path = ? WHERE id = ?")
        .bind(target.to_string_lossy().as_ref())
        .bind(id)
        .execute(&state.db)
        .await?;
    acquisition::set_progress(&state.db, id, 100.0).await?;
    if let Err(error) =
        acquisition::transition(&state.db, id, AcquisitionStatus::Downloaded, None).await
    {
        let _ = tokio::fs::remove_file(&target).await;
        return Err(error);
    }
    crate::import_pipeline::spawn(state, id.to_string());
    Ok(())
}

async fn download_to(
    state: &AppState,
    id: &str,
    input: &HttpInput,
    partial: &Path,
) -> Result<bool, AppError> {
    let max_bytes = Limits::default().max_file_bytes;
    let mut response =
        remote_http::response(&input.url, input.trusted_origin.as_deref(), None, max_bytes).await?;
    if response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"))
    {
        return Err(AppError::Unprocessable(
            "the download returned a web page, not a book file".to_string(),
        ));
    }
    let expected_size = response.content_length();
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(partial)
        .await?;
    let mut written = 0u64;
    let mut next_check = 4 * 1024 * 1024;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AppError::Unavailable("remote content could not be read".to_string()))?
    {
        written += chunk.len() as u64;
        if written > max_bytes {
            return Err(AppError::Unprocessable(
                "book file exceeds the size limit".to_string(),
            ));
        }
        file.write_all(&chunk).await?;
        if written >= next_check {
            if is_cancelled(state, id).await? {
                return Ok(false);
            }
            if let Some(total) = expected_size.filter(|size| *size > 0) {
                acquisition::set_progress(
                    &state.db,
                    id,
                    (written as f64 * 100.0 / total as f64).min(99.0),
                )
                .await?;
            }
            next_check = written + 4 * 1024 * 1024;
        }
    }
    if written == 0 {
        return Err(AppError::Unprocessable(
            "the download was empty".to_string(),
        ));
    }
    file.sync_all().await?;
    Ok(true)
}

async fn verify_file(path: &Path, format: &str) -> Result<(), AppError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut header = [0u8; 8];
    let count = file.read(&mut header).await?;
    let valid = match format {
        "pdf" => count >= 5 && &header[..5] == b"%PDF-",
        "epub" | "cbz" => count >= 4 && &header[..4] == b"PK\x03\x04",
        _ => false,
    };
    if !valid {
        return Err(AppError::Unprocessable(
            "downloaded bytes do not match the expected book format".to_string(),
        ));
    }
    Ok(())
}

async fn is_cancelled(state: &AppState, id: &str) -> Result<bool, AppError> {
    Ok(acquisition::get(&state.db, id)
        .await?
        .is_none_or(|record| record.status().ok() == Some(AcquisitionStatus::Cancelled)))
}

fn safe_filename(title: &str) -> String {
    let clean: String = title
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(character, ' ' | '-' | '_'))
        .take(80)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "book".to_string()
    } else {
        clean.to_string()
    }
}

pub fn owned_file(state: &AppState, id: &str, path: &Path) -> bool {
    let directory: PathBuf = state.paths.downloads_dir.join("http").join(id);
    path.parent() == Some(directory.as_path())
        && crate::paths::is_within(&state.paths.downloads_dir, path)
}
