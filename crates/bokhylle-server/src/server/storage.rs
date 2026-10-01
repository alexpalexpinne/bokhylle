use crate::paths::Paths;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageLocation {
    pub label: String,
    pub path: String,
    pub writable: bool,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageGroup {
    pub locations: Vec<StorageLocation>,
    pub available_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub low_space: bool,
    pub error: Option<String>,
}

pub fn inspect(paths: &Paths) -> Vec<StorageGroup> {
    let mut groups: Vec<(Option<u64>, StorageGroup)> = Vec::new();
    for (label, path) in [
        ("Config", &paths.config_dir),
        ("Library", &paths.library_root),
        ("Downloads", &paths.downloads_dir),
    ] {
        let identity = filesystem_id(path);
        let (available_bytes, total_bytes, error) = match capacity(path) {
            Ok((available, total)) => (Some(available), Some(total), None),
            Err(_) => (
                None,
                None,
                Some("Filesystem capacity is unavailable".into()),
            ),
        };
        let writable = write_probe(path);
        let location = StorageLocation {
            label: label.into(),
            path: path.to_string_lossy().into_owned(),
            writable,
            error: (!writable).then(|| "Directory is missing or not writable".into()),
        };
        if let Some((_, group)) = groups
            .iter_mut()
            .find(|(id, _)| identity.is_some() && *id == identity)
        {
            group.locations.push(location);
        } else {
            let low_space = available_bytes
                .zip(total_bytes)
                .is_some_and(|(available, total)| {
                    available < 1024 * 1024 * 1024
                        || (total > 0 && (available as f64 / total as f64) < 0.05)
                });
            groups.push((
                identity,
                StorageGroup {
                    locations: vec![location],
                    available_bytes,
                    total_bytes,
                    low_space,
                    error,
                },
            ));
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

fn write_probe(path: &Path) -> bool {
    let probe = path.join(format!(
        ".bokhylle-storage-check-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    else {
        return false;
    };
    let writable = std::io::Write::write_all(&mut file, b"bokhylle").is_ok();
    drop(file);
    std::fs::remove_file(probe).is_ok() && writable
}

#[cfg(unix)]
fn filesystem_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|metadata| metadata.dev())
}

#[cfg(not(unix))]
fn filesystem_id(_: &Path) -> Option<u64> {
    None
}

#[cfg(unix)]
fn capacity(path: &Path) -> std::io::Result<(u64, u64)> {
    use std::{ffi::CString, mem::MaybeUninit, os::unix::ffi::OsStrExt};
    let path =
        CString::new(path.as_os_str().as_bytes()).map_err(|_| std::io::ErrorKind::InvalidInput)?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: the CString is NUL-terminated and the output pointer is valid.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: statvfs initialized the structure after returning success.
    let stats = unsafe { stats.assume_init() };
    #[allow(clippy::unnecessary_cast)]
    let (block, available, total) = (
        stats.f_frsize as u64,
        stats.f_bavail as u64,
        stats.f_blocks as u64,
    );
    Ok((available.saturating_mul(block), total.saturating_mul(block)))
}

#[cfg(not(unix))]
fn capacity(_: &Path) -> std::io::Result<(u64, u64)> {
    Err(std::io::ErrorKind::Unsupported.into())
}
