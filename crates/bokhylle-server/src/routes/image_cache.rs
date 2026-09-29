use std::path::{Path, PathBuf};
use std::time::Duration;

// A confirmed missing provider image is worth retrying later. Network errors
// never create a marker, so a provider outage cannot hide available artwork.
const MISSING_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

pub(super) async fn recently_missing(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age < MISSING_TTL)
}

pub(super) async fn mark_missing(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    write_atomic(path, &[]).await
}

pub(crate) async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    if let Err(error) = tokio::fs::write(&temporary, bytes).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    if let Err(error) = tokio::fs::rename(&temporary, path).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }
    Ok(())
}

/// Only these disposable directories are pruned. Legacy `cache/covers` can
/// contain durable paths stored in the database and must never be swept.
pub async fn prune(config_dir: &Path) -> std::io::Result<u64> {
    const MAX_AGE: Duration = Duration::from_secs(90 * 24 * 3600);
    const MAX_BYTES: u64 = 256 * 1024 * 1024;
    let mut files: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();
    let mut removed = 0;
    for folder in ["provider-covers", "authors"] {
        let dir = config_dir.join("cache").join(folder);
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_file() {
                continue;
            }
            let metadata = entry.metadata().await?;
            let modified = metadata.modified()?;
            let age = modified.elapsed().unwrap_or_default();
            let ttl = if entry.path().extension().is_some_and(|ext| ext == "missing") {
                MISSING_TTL
            } else {
                MAX_AGE
            };
            if age >= ttl {
                tokio::fs::remove_file(entry.path()).await?;
                removed += 1;
            } else {
                files.push((entry.path(), modified, metadata.len()));
            }
        }
    }
    let mut total: u64 = files.iter().map(|(_, _, len)| len).sum();
    files.sort_by_key(|(_, modified, _)| *modified);
    for (path, _, len) in files {
        if total <= MAX_BYTES {
            break;
        }
        tokio::fs::remove_file(path).await?;
        total -= len;
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn prune_never_touches_library_artwork_or_legacy_covers() {
        let temp = tempfile::tempdir().unwrap();
        let durable = temp.path().join("artwork/covers/book.jpg");
        let legacy = temp.path().join("cache/covers/book.jpg");
        let disposable = temp.path().join("cache/provider-covers/old.jpg");
        for path in [&durable, &legacy, &disposable] {
            write_atomic(path, b"cover").await.unwrap();
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_times(
                std::fs::FileTimes::new().set_modified(
                    std::time::SystemTime::now() - Duration::from_secs(91 * 24 * 3600),
                ),
            )
            .unwrap();
        }
        assert_eq!(prune(temp.path()).await.unwrap(), 1);
        assert!(durable.exists());
        assert!(legacy.exists());
        assert!(!disposable.exists());
    }
}
