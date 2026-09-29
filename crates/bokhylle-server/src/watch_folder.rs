//! Polls a household drop folder and sends complete publication files through
//! the same verified placement and library indexing used by other imports.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bokhylle_core::BookFormat;
use bokhylle_importer::Limits;
use sqlx::Row;
use uuid::Uuid;

use crate::{AppState, error::AppError, import_pipeline, library, paths, settings};

const SETTLE_AGE: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatus {
    pub enabled: bool,
    pub path: String,
    pub pending: i64,
    pub cleanup_pending: i64,
    pub review_files: usize,
    pub last_error: Option<String>,
}

pub async fn status(state: &AppState) -> Result<WatchStatus, AppError> {
    let enabled = state
        .settings
        .get_bool(settings::WATCH_ENABLED, false)
        .await?;
    let root = configured_root(state).await?;
    let path = root.to_string_lossy().into_owned();
    let review_files = tokio::task::spawn_blocking(move || {
        let review = root.join("review");
        if !review.exists() {
            return Ok::<_, AppError>(0);
        }
        if !paths::is_within(&root, &review) {
            return Err(AppError::Unprocessable(
                "watch review path is outside its folder".into(),
            ));
        }
        Ok(std::fs::read_dir(review)?
            .take(10_000)
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .count())
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))??;
    let (pending, cleanup_pending): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(sum(status = 'placing'), 0), COALESCE(sum(status = 'imported' AND cleanup_pending = 1), 0) FROM watch_imports",
    ).fetch_one(&state.db).await?;
    let last_error: Option<String> = sqlx::query_scalar("SELECT error_message FROM watch_imports WHERE error_message IS NOT NULL ORDER BY updated_at DESC LIMIT 1")
        .fetch_optional(&state.db).await?.flatten();
    Ok(WatchStatus {
        enabled,
        path,
        pending,
        cleanup_pending,
        review_files,
        last_error,
    })
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = tick(&state).await {
                tracing::warn!(%error, "watch_folder.tick_failed");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

pub async fn tick(state: &AppState) -> Result<usize, AppError> {
    if !state.imports.try_acquire("watch-folder") {
        return Ok(0);
    }
    let result = tick_locked(state).await;
    state.imports.release("watch-folder");
    result
}

async fn tick_locked(state: &AppState) -> Result<usize, AppError> {
    recover(state).await?;
    if !state
        .settings
        .get_bool(settings::WATCH_ENABLED, false)
        .await?
    {
        return Ok(0);
    }
    let root = configured_root(state).await?;
    let files = tokio::task::spawn_blocking(move || eligible_files(&root))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))??;
    let mut imported = 0;
    for (root, path, format) in files {
        match import_one(state, &root, &path, format).await {
            Ok(true) => imported += 1,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "watch_folder.import_failed")
            }
        }
    }
    Ok(imported)
}

pub async fn recover(state: &AppState) -> Result<(), AppError> {
    let rows = sqlx::query(
        "SELECT id, source_path, staged_path, target_path, sha256, status FROM watch_imports WHERE status = 'placing' OR cleanup_pending = 1 ORDER BY status DESC, created_at LIMIT 100",
    )
    .fetch_all(&state.db)
    .await?;
    for row in rows {
        let id: String = row.get("id");
        let source = PathBuf::from(row.get::<String, _>("source_path"));
        let staged = PathBuf::from(row.get::<String, _>("staged_path"));
        let target = PathBuf::from(row.get::<String, _>("target_path"));
        let digest: String = row.get("sha256");
        let result = if row.get::<String, _>("status") == "imported" {
            cleanup_completed(state, &id, &source, &staged, &digest).await
        } else {
            finish(state, &id, &source, &staged, &target, &digest).await
        };
        if let Err(error) = result {
            record_error(state, &id, &error).await?;
            tracing::warn!(import_id = %id, %error, "watch_folder.recovery_failed");
        }
    }
    let protected: Vec<String> = sqlx::query_scalar(
        "SELECT staged_path FROM watch_imports WHERE status = 'placing' OR cleanup_pending = 1",
    )
    .fetch_all(&state.db)
    .await?;
    let staging = state.paths.config_dir.join("staging/watch");
    let config = state.paths.config_dir.clone();
    tokio::task::spawn_blocking(move || clean_orphans(&config, &staging, &protected))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))??;
    Ok(())
}

async fn record_error(state: &AppState, id: &str, error: &AppError) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE watch_imports SET error_message = ?, updated_at = unixepoch() WHERE id = ?",
    )
    .bind(error.to_string())
    .bind(id)
    .execute(&state.db)
    .await?;
    Ok(())
}

fn clean_orphans(config: &Path, staging: &Path, protected: &[String]) -> Result<(), AppError> {
    if !staging.exists() {
        return Ok(());
    }
    if !paths::is_within(config, staging) {
        return Err(AppError::Unprocessable(
            "watch staging path is outside config".into(),
        ));
    }
    let protected: std::collections::HashSet<&Path> = protected.iter().map(Path::new).collect();
    let mut removed = 0;
    for entry in std::fs::read_dir(staging)?.take(10_000) {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() || !paths::is_within(staging, &path) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if Uuid::parse_str(stem).is_err()
            || !matches!(extension, "partial" | "epub" | "pdf" | "cbz")
        {
            continue;
        }
        let publication = ["epub", "pdf", "cbz"]
            .into_iter()
            .any(|format| protected.contains(path.with_extension(format).as_path()));
        if protected.contains(path.as_path()) || publication {
            continue;
        }
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= Duration::from_secs(3600));
        if stale {
            std::fs::remove_file(path)?;
            removed += 1;
            if removed >= 100 {
                break;
            }
        }
    }
    Ok(())
}

async fn configured_root(state: &AppState) -> Result<PathBuf, AppError> {
    let value = state
        .settings
        .get_string(settings::WATCH_FOLDER, "")
        .await?;
    let config = state.paths.config_dir.clone();
    let library = state.paths.library_root.clone();
    let downloads = state.paths.downloads_dir.clone();
    tokio::task::spawn_blocking(move || {
        let root = if value.trim().is_empty() {
            config.canonicalize()?.join("ingest")
        } else {
            PathBuf::from(value.trim())
        };
        if !root.is_absolute() {
            return Err(AppError::Unprocessable(
                "watch folder must be an absolute path".into(),
            ));
        }
        std::fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        let library = library.canonicalize()?;
        let downloads = downloads.canonicalize()?;
        if root.starts_with(&library)
            || library.starts_with(&root)
            || root.starts_with(&downloads)
            || downloads.starts_with(&root)
        {
            return Err(AppError::Unprocessable(
                "watch folder must not overlap the library or downloads directory".into(),
            ));
        }
        Ok(root)
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))?
}

fn eligible_files(root: &Path) -> Result<Vec<(PathBuf, PathBuf, BookFormat)>, AppError> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root)?.take(10_000) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "watch_folder.entry_failed");
                continue;
            }
        };
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() || !paths::is_within(root, &path) {
            continue;
        }
        let Some(format) = path
            .extension()
            .and_then(|part| part.to_str())
            .and_then(BookFormat::from_extension)
        else {
            continue;
        };
        let age = metadata
            .modified()
            .ok()
            .and_then(|time| SystemTime::now().duration_since(time).ok());
        if age.is_some_and(|age| age >= SETTLE_AGE) {
            files.push((root.to_path_buf(), path, format));
        }
    }
    // Rotate bounded batches so a broken file or pending placement cannot
    // indefinitely keep later directory entries from being imported.
    if files.len() > 100 {
        files.sort_by(|a, b| a.1.cmp(&b.1));
        let slot = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / POLL_INTERVAL.as_secs();
        let offset = (slot as usize * 100) % files.len();
        files.rotate_left(offset);
        files.truncate(100);
    }
    Ok(files)
}

async fn import_one(
    state: &AppState,
    root: &Path,
    source: &Path,
    format: BookFormat,
) -> Result<bool, AppError> {
    let pending: Option<String> = sqlx::query_scalar(
        "SELECT id FROM watch_imports WHERE source_path = ? AND status = 'placing' LIMIT 1",
    )
    .bind(source.to_string_lossy().as_ref())
    .fetch_optional(&state.db)
    .await?;
    if pending.is_some() {
        return Ok(false);
    }
    if !paths::is_within(root, source) {
        return Err(AppError::Unprocessable(
            "watch file left its configured folder".into(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    let staging_root = state.paths.config_dir.join("staging").join("watch");
    std::fs::create_dir_all(&staging_root)?;
    if !paths::is_within(&state.paths.config_dir, &staging_root) {
        return Err(AppError::Unprocessable(
            "watch staging path is outside config".into(),
        ));
    }
    let staged = staging_root.join(format!("{id}.{}", format.as_str()));
    let source_copy = source.to_path_buf();
    let staged_copy = staged.clone();
    let snapshot =
        tokio::task::spawn_blocking(move || snapshot(&source_copy, &staged_copy, format))
            .await
            .map_err(|error| AppError::Unavailable(error.to_string()))?;
    let (digest, title, author) = match snapshot {
        Ok(value) => value,
        Err(error) => {
            let rejected_digest = if matches!(error, AppError::BadRequest(_)) {
                let path = staged.clone();
                tokio::task::spawn_blocking(move || library::hash_file(&path))
                    .await
                    .ok()
                    .and_then(Result::ok)
            } else {
                None
            };
            let _ = std::fs::remove_file(&staged);
            let _ = std::fs::remove_file(staged.with_extension("partial"));
            if let Some(digest) = rejected_digest {
                let root = root.to_path_buf();
                let source = source.to_path_buf();
                let _ =
                    tokio::task::spawn_blocking(move || quarantine(&root, &source, &digest)).await;
            }
            return Err(error);
        }
    };
    let duplicate: Option<String> =
        sqlx::query_scalar("SELECT path FROM book_files WHERE sha256 = ?")
            .bind(&digest)
            .fetch_optional(&state.db)
            .await?;
    if let Some(duplicate) = duplicate {
        let library_root = state.paths.library_root.clone();
        let expected = digest.clone();
        let verified = tokio::task::spawn_blocking(move || {
            let path = Path::new(&duplicate);
            paths::is_within(&library_root, path)
                && library::hash_file(path).is_ok_and(|actual| actual == expected)
        })
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))?;
        if !verified {
            let _ = tokio::fs::remove_file(&staged).await;
            return Err(AppError::Unprocessable(
                "the recorded library duplicate is missing or changed; the watch file was retained"
                    .into(),
            ));
        }
        remove_source_if_unchanged(root, source, &digest).await?;
        std::fs::remove_file(&staged)?;
        return Ok(false);
    }
    let state_copy = state.clone();
    let target = tokio::task::spawn_blocking(move || {
        import_pipeline::canonical_path_from(&state_copy, &title, author.as_deref(), format)
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))??;
    sqlx::query("INSERT INTO watch_imports (id, source_path, staged_path, target_path, sha256, status) VALUES (?, ?, ?, ?, ?, 'placing')")
        .bind(&id).bind(source.to_string_lossy().as_ref()).bind(staged.to_string_lossy().as_ref())
        .bind(target.to_string_lossy().as_ref()).bind(&digest).execute(&state.db).await?;
    if let Err(error) = finish(state, &id, source, &staged, &target, &digest).await {
        record_error(state, &id, &error).await?;
        return Err(error);
    }
    Ok(true)
}

fn snapshot(
    source: &Path,
    staged: &Path,
    format: BookFormat,
) -> Result<(String, String, Option<String>), AppError> {
    let before = std::fs::metadata(source)?;
    if !before.is_file() || before.len() == 0 || before.len() > Limits::default().max_file_bytes {
        return Err(AppError::Unprocessable(
            "watch file is empty or exceeds the import size limit".into(),
        ));
    }
    let partial = staged.with_extension("partial");
    #[cfg(unix)]
    let input = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(source)?
    };
    #[cfg(not(unix))]
    let input = std::fs::File::open(source)?;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let copied = std::io::copy(
        &mut input.take(Limits::default().max_file_bytes + 1),
        &mut output,
    )?;
    output.sync_all()?;
    if copied != before.len() || copied > Limits::default().max_file_bytes {
        let _ = std::fs::remove_file(&partial);
        return Err(AppError::Unprocessable(
            "watch file changed while being copied".into(),
        ));
    }
    let after = std::fs::metadata(source)?;
    if after.len() != before.len() || after.modified()? != before.modified()? {
        let _ = std::fs::remove_file(&partial);
        return Err(AppError::Unprocessable(
            "watch file changed while being copied".into(),
        ));
    }
    std::fs::rename(&partial, staged)?;
    let extracted = bokhylle_library::extract::extract_with_filename(staged, format, source)
        .map_err(|_| AppError::BadRequest("watch file is not a valid publication".into()))?;
    let digest = library::hash_file(staged)?;
    let title = extracted
        .metadata
        .title
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            source
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Untitled".into());
    let author = extracted.metadata.authors.into_iter().next();
    Ok((digest, title, author))
}

fn quarantine(root: &Path, source: &Path, digest: &str) -> Result<(), AppError> {
    if !paths::is_within(root, source) || !std::fs::symlink_metadata(source)?.file_type().is_file()
    {
        return Ok(());
    }
    if library::hash_file(source)? != digest {
        return Ok(());
    }
    let review = root.join("review");
    std::fs::create_dir_all(&review)?;
    if !paths::is_within(root, &review) {
        return Err(AppError::Unprocessable(
            "watch review path is outside its folder".into(),
        ));
    }
    let name = source.file_name().unwrap_or_default().to_string_lossy();
    let target = review.join(format!("{}-{name}", Uuid::new_v4()));
    std::fs::rename(source, target)?;
    Ok(())
}

async fn finish(
    state: &AppState,
    id: &str,
    source: &Path,
    staged: &Path,
    target: &Path,
    digest: &str,
) -> Result<(), AppError> {
    if staged.exists() && !paths::is_within(&state.paths.config_dir.join("staging/watch"), staged) {
        return Err(AppError::Unprocessable(
            "watch staging file is outside its staging area".into(),
        ));
    }
    if !paths::is_within(&state.paths.library_root, target.parent().unwrap_or(target))
        || (target.exists() && !paths::is_within(&state.paths.library_root, target))
    {
        return Err(AppError::Unprocessable(
            "watch target is outside the library".into(),
        ));
    }
    if !target.exists() {
        if !paths::is_within(&state.paths.config_dir.join("staging/watch"), staged) {
            return Err(AppError::Unprocessable(
                "watch staging file is missing or outside config".into(),
            ));
        }
        import_pipeline::place_file(state, staged, target, digest).await?;
    }
    let target_copy = target.to_path_buf();
    let actual = tokio::task::spawn_blocking(move || library::hash_file(&target_copy))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))??;
    if actual != digest {
        return Err(AppError::Unprocessable(
            "watch target digest does not match the staged file".into(),
        ));
    }
    library::index_placed_file_with_filename(state, target, source).await?;
    sqlx::query(
        "UPDATE watch_imports SET status = 'imported', error_message = NULL, cleanup_pending = 1, updated_at = unixepoch() WHERE id = ?",
    )
    .bind(id)
    .execute(&state.db)
    .await?;
    cleanup_completed(state, id, source, staged, digest).await
}

async fn cleanup_completed(
    state: &AppState,
    id: &str,
    source: &Path,
    staged: &Path,
    digest: &str,
) -> Result<(), AppError> {
    if staged.exists() && !paths::is_within(&state.paths.config_dir.join("staging/watch"), staged) {
        return Err(AppError::Unprocessable(
            "watch staging file is outside its staging area".into(),
        ));
    }
    let root = configured_root(state).await?;
    remove_source_if_unchanged(&root, source, digest).await?;
    match tokio::fs::remove_file(staged).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    sqlx::query("UPDATE watch_imports SET cleanup_pending = 0, error_message = NULL, updated_at = unixepoch() WHERE id = ?")
        .bind(id).execute(&state.db).await?;
    Ok(())
}

async fn remove_source_if_unchanged(
    root: &Path,
    source: &Path,
    digest: &str,
) -> Result<(), AppError> {
    if !paths::is_within(root, source)
        || std::fs::symlink_metadata(source)?.file_type().is_symlink()
    {
        return Ok(());
    }
    let source = source.to_path_buf();
    let digest = digest.to_string();
    tokio::task::spawn_blocking(move || {
        if library::hash_file(&source)? == digest {
            std::fs::remove_file(source)?;
        }
        Ok::<_, AppError>(())
    })
    .await
    .map_err(|error| AppError::Unavailable(error.to_string()))?
}
