use std::cmp::Ordering;
use std::io::Read;
use std::path::{Component, Path};

use zip::ZipArchive;

use super::{Cover, Extracted, ExtractedMetadata, fallback_from_filename};
use crate::error::LibraryError;

const MAX_ENTRIES: usize = 4_000;
const MAX_PAGES: usize = 2_000;
const MAX_PAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Page {
    pub zip_index: usize,
    pub name: String,
    pub mime: &'static str,
}

fn invalid(path: &Path, reason: &str) -> LibraryError {
    LibraryError::Invalid {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    }
}

fn image_mime(name: &str) -> Option<&'static str> {
    match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        _ => None,
    }
}

fn image_signature(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        _ => false,
    }
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if left[i].is_ascii_digit() && right[j].is_ascii_digit() {
            let (start_i, start_j) = (i, j);
            while i < left.len() && left[i].is_ascii_digit() {
                i += 1;
            }
            while j < right.len() && right[j].is_ascii_digit() {
                j += 1;
            }
            let number_i = &left[start_i..i];
            let number_j = &right[start_j..j];
            let significant_i = number_i
                .iter()
                .position(|b| *b != b'0')
                .unwrap_or(number_i.len() - 1);
            let significant_j = number_j
                .iter()
                .position(|b| *b != b'0')
                .unwrap_or(number_j.len() - 1);
            let number_i = &number_i[significant_i..];
            let number_j = &number_j[significant_j..];
            let order = number_i
                .len()
                .cmp(&number_j.len())
                .then_with(|| number_i.cmp(number_j));
            if order != Ordering::Equal {
                return order;
            }
        } else {
            let order = left[i]
                .to_ascii_lowercase()
                .cmp(&right[j].to_ascii_lowercase());
            if order != Ordering::Equal {
                return order;
            }
            i += 1;
            j += 1;
        }
    }
    left.len().cmp(&right.len())
}

pub fn pages(path: &Path) -> Result<Vec<Page>, LibraryError> {
    let file = std::fs::File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() > MAX_ENTRIES {
        return Err(invalid(path, "too many CBZ entries"));
    }
    let mut total = 0u64;
    let mut pages = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        total = total.saturating_add(entry.size());
        if total > MAX_TOTAL_BYTES {
            return Err(invalid(path, "CBZ expands beyond the size limit"));
        }
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let safe = Path::new(&name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
        if !safe {
            return Err(invalid(path, "CBZ contains an unsafe path"));
        }
        let Some(mime) = image_mime(&name) else {
            continue;
        };
        if entry.size() > MAX_PAGE_BYTES {
            return Err(invalid(path, "CBZ page exceeds the size limit"));
        }
        let mut signature = [0u8; 12];
        let read = entry.read(&mut signature)?;
        if !image_signature(mime, &signature[..read]) {
            return Err(invalid(path, "CBZ page content is not an image"));
        }
        pages.push(Page {
            zip_index: index,
            name,
            mime,
        });
        if pages.len() > MAX_PAGES {
            return Err(invalid(path, "too many CBZ pages"));
        }
    }
    if pages.is_empty() {
        return Err(invalid(path, "CBZ has no readable images"));
    }
    pages.sort_by(|left, right| natural_cmp(&left.name, &right.name));
    Ok(pages)
}

pub fn read_page(path: &Path, page_number: usize) -> Result<(Vec<u8>, &'static str), LibraryError> {
    let pages = pages(path)?;
    let Some(page) = pages.get(page_number.checked_sub(1).unwrap_or(usize::MAX)) else {
        return Err(invalid(path, "CBZ page does not exist"));
    };
    let file = std::fs::File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut entry = archive.by_index(page.zip_index)?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .by_ref()
        .take(MAX_PAGE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PAGE_BYTES {
        return Err(invalid(path, "CBZ page exceeds the size limit"));
    }
    if !image_signature(page.mime, &bytes) {
        return Err(invalid(path, "CBZ page content is not an image"));
    }
    Ok((bytes, page.mime))
}

pub fn extract(path: &Path) -> Result<Extracted, LibraryError> {
    extract_with_filename(path, path)
}

pub(crate) fn extract_with_filename(
    path: &Path,
    filename: &Path,
) -> Result<Extracted, LibraryError> {
    let (title, authors) = fallback_from_filename(filename);
    let (bytes, mime) = read_page(path, 1)?;
    let cover = (bytes.len() <= 8 * 1024 * 1024).then(|| Cover::new(bytes, mime));
    Ok(Extracted {
        metadata: ExtractedMetadata {
            title,
            authors,
            ..Default::default()
        },
        cover,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn orders_pages_naturally_and_rejects_fake_images() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Example.cbz");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for name in ["page10.png", "page2.png", "page1.png"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"\x89PNG\r\n\x1a\nfictional").unwrap();
        }
        zip.finish().unwrap();
        let names: Vec<String> = pages(&path)
            .unwrap()
            .into_iter()
            .map(|page| page.name)
            .collect();
        assert_eq!(names, ["page1.png", "page2.png", "page10.png"]);
        assert_eq!(read_page(&path, 2).unwrap().1, "image/png");
        assert_eq!(
            extract(&path).unwrap().metadata.title.as_deref(),
            Some("Example")
        );
    }

    #[test]
    fn rejects_a_fake_image_after_the_cover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Fake page.cbz");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, bytes) in [
            ("page1.png", &b"\x89PNG\r\n\x1a\nfictional"[..]),
            ("page2.png", &b"not an image"[..]),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        assert!(pages(&path).is_err());
        assert!(extract(&path).is_err());
    }
}
