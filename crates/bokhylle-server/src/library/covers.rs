//! Durable library artwork from verified files, with catalogue artwork as a
//! fallback. All file reads and extraction stay off the async runtime.

use std::path::{Path, PathBuf};

use bokhylle_core::BookFormat;
use bokhylle_library::covers::usable_cover;
use bokhylle_library::extract;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::metadata_fields::{self, MetadataField as Field, Scope};
use crate::AppState;
use crate::error::AppError;

pub async fn extract_embedded(
    state: &AppState,
    book_id: i64,
    path: &Path,
    format: BookFormat,
    digest: &str,
) -> Result<Option<PathBuf>, AppError> {
    let root = state.paths.library_root.clone();
    let path = path.to_path_buf();
    let covers_dir = state.paths.config_dir.join("artwork").join("covers");
    let stored = tokio::task::spawn_blocking(move || {
        let path = crate::paths::library_target(&root, &path, false)?;
        let extracted = extract::extract(&path, format)
            .map_err(|error| AppError::Unavailable(error.to_string()))?;
        let Some(cover) = extracted.cover.filter(|cover| usable_cover(&cover.bytes)) else {
            return Ok(None);
        };
        super::write_cover(&covers_dir, &cover)
            .map(Some)
            .map_err(AppError::from)
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))??;
    if let Some(path) = &stored {
        let mut tx = state.db.begin().await?;
        metadata_fields::automatic(
            &mut tx,
            Scope::Book(book_id),
            Field::Cover,
            json!(path.to_string_lossy()),
            format.as_str(),
            Some(digest),
            true,
        )
        .await?;
        tx.commit().await?;
    }
    Ok(stored)
}

/// Best effort: artwork failure must not turn a verified import into a failed
/// download. The cover route can retry extraction after a restart.
pub async fn imported(
    state: &AppState,
    book_id: i64,
    path: &Path,
    format: BookFormat,
    digest: &str,
) {
    if let Err(error) = extract_embedded(state, book_id, path, format, digest).await {
        tracing::warn!(%error, book_id, "library.cover.extract_failed");
    }
}

pub async fn restore(state: &AppState, book_id: i64) -> Result<Option<PathBuf>, AppError> {
    let files: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT f.path, f.format, f.sha256 FROM book_files f
         JOIN editions e ON e.id = f.edition_id WHERE e.book_id = ?
         ORDER BY CASE f.format WHEN 'epub' THEN 0 WHEN 'cbz' THEN 1 ELSE 2 END, f.id",
    )
    .bind(book_id)
    .fetch_all(&state.db)
    .await?;
    for (path, format, digest) in files {
        let Some(format) = BookFormat::from_extension(&format) else {
            continue;
        };
        match extract_embedded(state, book_id, Path::new(&path), format, &digest).await {
            Ok(Some(path)) => return Ok(Some(path)),
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, book_id, "library.cover.restore_failed"),
        }
    }
    let source: Option<(String, String)> =
        sqlx::query_as("SELECT provider, cover_id FROM book_cover_sources WHERE book_id = ?")
            .bind(book_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((provider, cover_id)) = source else {
        return Ok(None);
    };
    let Some(client) = state.registry.covers(&provider) else {
        return Ok(None);
    };
    let covers_dir = state.paths.config_dir.join("artwork").join("covers");
    let cache =
        crate::routes::discover::provider_cover_cache_path(state, &provider, &cover_id, false);
    let bytes = match tokio::fs::read(cache).await {
        Ok(bytes) if usable_cover(&bytes) => Some(bytes),
        _ => client
            .fetch_cover(&cover_id)
            .await
            .map_err(|error| AppError::Unavailable(error.to_string()))?,
    };
    let Some(bytes) = bytes.filter(|bytes| usable_cover(bytes)) else {
        return Ok(None);
    };
    let extension = crate::routes::discover::image_extension(&bytes);
    let path = covers_dir.join(format!(
        "{}.{}",
        hex::encode(Sha256::digest(&bytes)),
        extension
    ));
    crate::routes::image_cache::write_atomic(&path, &bytes).await?;
    let mut tx = state.db.begin().await?;
    metadata_fields::automatic(
        &mut tx,
        Scope::Book(book_id),
        Field::Cover,
        json!(path.to_string_lossy()),
        &provider,
        Some(&cover_id),
        true,
    )
    .await?;
    tx.commit().await?;
    Ok(Some(path))
}
