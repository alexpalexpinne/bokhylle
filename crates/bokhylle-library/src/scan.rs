use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use bokhylle_core::BookFormat;
use walkdir::WalkDir;

use crate::error::LibraryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub format: BookFormat,
    pub size: u64,
    pub modified: Option<i64>,
}

pub fn scan(root: &Path) -> Result<Vec<ScannedFile>, LibraryError> {
    let mut files = Vec::new();

    let entries = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !is_ignored_dir(&entry.file_name().to_string_lossy())
        });

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy();
        if is_ignored_file(&name) {
            continue;
        }

        let extension = entry
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        let Some(format) = BookFormat::from_extension(extension) else {
            continue;
        };

        let metadata = entry.metadata().ok();
        let size = metadata
            .as_ref()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let modified = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(system_time_to_epoch);

        files.push(ScannedFile {
            path: entry.path().to_path_buf(),
            format,
            size,
            modified,
        });
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "@eaDir" | "$RECYCLE.BIN" | "System Volume Information" | "lost+found"
        )
}

fn is_ignored_file(name: &str) -> bool {
    name.starts_with('.') || name.starts_with("._") || matches!(name, "Thumbs.db" | "desktop.ini")
}

fn system_time_to_epoch(time: std::time::SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"content").unwrap();
    }

    #[test]
    fn finds_supported_files_and_skips_junk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        write_file(&root.join("Andy Weir/Project Hail Mary.epub"));
        write_file(&root.join("Frank Herbert/Dune.PDF"));
        write_file(&root.join("notes.txt"));
        write_file(&root.join("cover.jpg"));
        write_file(&root.join(".DS_Store"));
        write_file(&root.join("Thumbs.db"));
        write_file(&root.join("._hidden.epub"));
        write_file(&root.join("@eaDir/Thumbs.db"));
        write_file(&root.join(".hidden/secret.epub"));

        let files = scan(root).unwrap();

        let paths: Vec<String> = files
            .iter()
            .map(|file| file.path.strip_prefix(root).unwrap().display().to_string())
            .collect();

        assert_eq!(
            paths,
            vec!["Andy Weir/Project Hail Mary.epub", "Frank Herbert/Dune.PDF"]
        );
        assert_eq!(files[0].format, BookFormat::Epub);
        assert_eq!(files[1].format, BookFormat::Pdf);
        assert!(files[0].size > 0);
    }

    #[test]
    fn missing_root_yields_empty_scan() {
        let dir = tempfile::tempdir().unwrap();
        let files = scan(&dir.path().join("does-not-exist")).unwrap();
        assert!(files.is_empty());
    }
}
