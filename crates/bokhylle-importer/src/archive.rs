use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::ImportError;
use crate::Limits;

const COPY_BUFFER: usize = 64 * 1024;

const ZIP_MAGIC: &[&[u8]] = &[b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"];
const RAR_MAGIC: &[&[u8]] = &[b"Rar!\x1a\x07\x00", b"Rar!\x1a\x07\x01\x00"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Rar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Ebook,
    Archive,
}

pub fn detect_archive(path: &Path) -> Option<ArchiveKind> {
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut magic = [0u8; 8];
        let read = file.read(&mut magic).unwrap_or(0);
        let magic = &magic[..read];

        if ZIP_MAGIC.iter().any(|prefix| magic.starts_with(prefix)) {
            return Some(ArchiveKind::Zip);
        }
        if RAR_MAGIC.iter().any(|prefix| magic.starts_with(prefix)) {
            return Some(ArchiveKind::Rar);
        }
    }

    match extension_of(path).as_str() {
        "zip" => Some(ArchiveKind::Zip),
        "rar" => Some(ArchiveKind::Rar),
        _ => None,
    }
}

pub fn entry_kind(path: &Path) -> Option<EntryKind> {
    match extension_of(path).as_str() {
        "epub" | "pdf" | "cbz" => Some(EntryKind::Ebook),
        "zip" | "rar" => Some(EntryKind::Archive),
        _ => None,
    }
}

pub fn extract_safely(
    archive_path: &Path,
    destination: &Path,
    limits: &Limits,
    extracted_total: &mut u64,
) -> Result<Vec<PathBuf>, ImportError> {
    match detect_archive(archive_path) {
        Some(ArchiveKind::Zip) => {
            extract_zip_safely(archive_path, destination, limits, extracted_total)
        }
        Some(ArchiveKind::Rar) => {
            extract_rar_safely(archive_path, destination, limits, extracted_total)
        }
        None => Ok(Vec::new()),
    }
}

pub fn extract_zip_safely(
    zip_path: &Path,
    destination: &Path,
    limits: &Limits,
    extracted_total: &mut u64,
) -> Result<Vec<PathBuf>, ImportError> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    if archive.len() > limits.max_entries {
        return Err(ImportError::ArchiveLimit(format!(
            "archive contains {} entries which exceeds the limit of {}",
            archive.len(),
            limits.max_entries
        )));
    }

    std::fs::create_dir_all(destination)?;

    let mut extracted = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;

        let Some(name) = entry.enclosed_name().and_then(|path| sanitize_path(&path)) else {
            continue;
        };

        if entry.is_dir() || entry_kind(&name).is_none() {
            continue;
        }

        if entry.size() > limits.max_file_bytes {
            return Err(ImportError::ArchiveLimit(format!(
                "archive entry '{}' exceeds the per-file limit of {} bytes",
                name.display(),
                limits.max_file_bytes
            )));
        }

        *extracted_total = extracted_total.saturating_add(entry.size());
        if *extracted_total > limits.max_uncompressed_bytes {
            return Err(ImportError::ArchiveLimit(format!(
                "archive exceeds the uncompressed size limit of {} bytes",
                limits.max_uncompressed_bytes
            )));
        }

        let target = destination.join(&name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        stream_entry(&mut entry, &target, limits.max_file_bytes)?;
        extracted.push(target);
    }

    Ok(extracted)
}

fn extract_rar_safely(
    rar_path: &Path,
    destination: &Path,
    limits: &Limits,
    extracted_total: &mut u64,
) -> Result<Vec<PathBuf>, ImportError> {
    let options =
        rars::ArchiveReadOptions::new().with_rar50_buffered_decode_limit(limits.max_file_bytes);
    let archive = rars::ArchiveReader::read_path_with_options(rar_path, options)?;

    let mut planned: Vec<(Vec<u8>, PathBuf)> = Vec::new();
    let mut entries: usize = 0;
    let mut archive_uncompressed: u64 = 0;

    for member in archive.members() {
        entries += 1;
        if entries > limits.max_entries {
            return Err(ImportError::ArchiveLimit(format!(
                "archive contains {} entries which exceeds the limit of {}",
                entries, limits.max_entries
            )));
        }

        if member.meta.is_directory {
            continue;
        }

        archive_uncompressed = archive_uncompressed.saturating_add(member.meta.unpacked_size);
        if archive_uncompressed > limits.max_uncompressed_bytes {
            return Err(ImportError::ArchiveLimit(format!(
                "archive exceeds the uncompressed size limit of {} bytes",
                limits.max_uncompressed_bytes
            )));
        }

        if member.meta.unpacked_size > limits.max_file_bytes {
            return Err(ImportError::ArchiveLimit(format!(
                "archive entry '{}' exceeds the per-file limit of {} bytes",
                member.meta.name_lossy(),
                limits.max_file_bytes
            )));
        }

        let Some(name) = rar_name_path(&member.meta.name).and_then(|path| sanitize_path(&path))
        else {
            continue;
        };
        if entry_kind(&name).is_none() {
            continue;
        }

        *extracted_total = extracted_total.saturating_add(member.meta.unpacked_size);
        if *extracted_total > limits.max_uncompressed_bytes {
            return Err(ImportError::ArchiveLimit(format!(
                "archive exceeds the uncompressed size limit of {} bytes",
                limits.max_uncompressed_bytes
            )));
        }

        planned.push((member.meta.name, name));
    }

    std::fs::create_dir_all(destination)?;

    let mut temporary_targets: Vec<(PathBuf, PathBuf)> = Vec::new();
    let extraction = archive.extract_to_with_options(options, |meta| {
        let Some((_, name)) = planned.iter().find(|(raw_name, _)| *raw_name == meta.name) else {
            return Ok(Box::new(std::io::sink()) as Box<dyn Write>);
        };

        let target = destination.join(name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file_name = target
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "entry".to_string());
        let temp =
            target.with_file_name(format!(".{file_name}.{}.partial", temporary_targets.len()));
        let output = std::fs::File::create(&temp)?;
        temporary_targets.push((temp, target));
        Ok(Box::new(output) as Box<dyn Write>)
    });

    if let Err(error) = extraction {
        for (temp, _) in &temporary_targets {
            let _ = std::fs::remove_file(temp);
        }
        return Err(error.into());
    }

    let mut extracted = Vec::new();
    for (temp, target) in temporary_targets {
        if let Err(error) = std::fs::rename(&temp, &target) {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        extracted.push(target);
    }

    Ok(extracted)
}

fn rar_name_path(name: &[u8]) -> Option<PathBuf> {
    String::from_utf8(name.to_vec()).ok().map(PathBuf::from)
}

fn sanitize_path(name: &Path) -> Option<PathBuf> {
    let mut sanitized = PathBuf::new();

    for component in name.components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(part) => {
                let part = part.to_str()?;
                if part.contains('\\') || part.contains('\0') {
                    return None;
                }
                sanitized.push(part);
            }
            _ => return None,
        }
    }

    if sanitized.as_os_str().is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn stream_entry<R: Read + ?Sized>(
    entry: &mut zip::read::ZipFile<'_, R>,
    target: &Path,
    max_file_bytes: u64,
) -> Result<(), ImportError> {
    let file_name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "entry".to_string());
    let temp = target.with_file_name(format!(".{file_name}.partial"));

    let result = (|| -> Result<(), ImportError> {
        let mut output = std::fs::File::create(&temp)?;
        let mut buffer = vec![0u8; COPY_BUFFER];
        let mut written: u64 = 0;

        loop {
            let read = entry.read(&mut buffer)?;
            if read == 0 {
                break;
            }

            written = written.saturating_add(read as u64);
            if written > max_file_bytes {
                return Err(ImportError::ArchiveLimit(format!(
                    "archive entry '{}' exceeds the per-file limit of {} bytes",
                    target.display(),
                    max_file_bytes
                )));
            }

            output.write_all(&buffer[..read])?;
        }

        output.flush()?;
        drop(output);
        std::fs::rename(&temp, target)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }

    result
}
