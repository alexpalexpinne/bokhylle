use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::settings::{self, Settings};

#[derive(Debug)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub library_root: PathBuf,
    pub downloads_dir: PathBuf,
    pub web_root: PathBuf,
}

impl Paths {
    pub async fn resolve(settings: &Settings, config_dir: PathBuf) -> Result<Self, AppError> {
        let default_library = config_dir.join("library").to_string_lossy().into_owned();
        let default_downloads = config_dir.join("downloads").to_string_lossy().into_owned();

        let library_root = PathBuf::from(
            settings
                .get_string(settings::LIBRARY_ROOT, &default_library)
                .await?,
        );
        let downloads_dir = PathBuf::from(
            settings
                .get_string(settings::DOWNLOADS_DIR, &default_downloads)
                .await?,
        );
        let web_root = PathBuf::from(
            std::env::var("BOKHYLLE_WEB_ROOT").unwrap_or_else(|_| "frontend/dist".to_string()),
        );

        ensure_dir(&config_dir, "config directory")?;
        ensure_dir(&library_root, "library root")?;
        ensure_dir(&downloads_dir, "downloads directory")?;

        Ok(Self {
            config_dir,
            library_root,
            downloads_dir,
            web_root,
        })
    }
}

fn ensure_dir(path: &Path, label: &str) -> Result<(), AppError> {
    if path.exists() {
        if !path.is_dir() {
            return Err(AppError::Unprocessable(format!(
                "{label} {} exists but is not a directory",
                path.display()
            )));
        }
        return Ok(());
    }

    std::fs::create_dir_all(path).map_err(|error| {
        AppError::Unprocessable(format!(
            "failed to create {label} {}: {error}",
            path.display()
        ))
    })
}

/// True when `path` resolves inside `root`. Both sides are canonicalized, so
/// a symlink cannot escape the root; a missing path is never contained. Used
/// before trusting a download client's content path or importing from disk.
pub fn is_within(root: &Path, path: &Path) -> bool {
    let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize()) else {
        return false;
    };
    path.starts_with(root)
}

/// Resolve a destination without allowing existing symlinks to route directory
/// creation outside the configured root. Recovery uses the same check without
/// creating missing directories.
pub(crate) fn library_target(
    root: &Path,
    target: &Path,
    create: bool,
) -> Result<PathBuf, AppError> {
    use std::path::Component;
    let absolute = |path: &Path| -> std::io::Result<PathBuf> {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            Ok(std::env::current_dir()?.join(path))
        }
    };
    let configured = absolute(root)?;
    let canonical = root.canonicalize()?;
    let target = absolute(target)?;
    let relative = target
        .strip_prefix(&configured)
        .or_else(|_| target.strip_prefix(&canonical))
        .map_err(|_| AppError::Unprocessable("import target is outside the library".into()))?;
    let parts: Vec<_> = relative.components().collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(AppError::Unprocessable("invalid import target path".into()));
    }
    let mut parent = canonical.clone();
    for part in &parts[..parts.len() - 1] {
        let next = parent.join(part.as_os_str());
        match std::fs::symlink_metadata(&next) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                match std::fs::create_dir(&next) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) => return Err(error.into()),
        }
        parent = next.canonicalize()?;
        if !parent.starts_with(&canonical) || !parent.is_dir() {
            return Err(AppError::Unprocessable(
                "import target is outside the library".into(),
            ));
        }
    }
    let resolved = parent.join(parts.last().expect("nonempty path").as_os_str());
    match std::fs::symlink_metadata(&resolved) {
        Ok(metadata) if !metadata.file_type().is_file() || !is_within(&canonical, &resolved) => {
            return Err(AppError::Unprocessable(
                "import target is not a contained regular file".into(),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containment_requires_the_path_to_resolve_inside_the_root() {
        let root = tempfile::tempdir().unwrap();
        let inside = root.path().join("book.epub");
        std::fs::write(&inside, b"book").unwrap();
        assert!(is_within(root.path(), &inside));

        let outside = tempfile::tempdir().unwrap();
        let other = outside.path().join("book.epub");
        std::fs::write(&other, b"book").unwrap();
        assert!(!is_within(root.path(), &other));

        assert!(
            !is_within(root.path(), &root.path().join("missing.epub")),
            "a path that does not resolve is not contained"
        );

        #[cfg(unix)]
        {
            let link = root.path().join("escape.epub");
            std::os::unix::fs::symlink(&other, &link).unwrap();
            assert!(
                !is_within(root.path(), &link),
                "a symlink out of the root is not contained"
            );
        }
    }
}
