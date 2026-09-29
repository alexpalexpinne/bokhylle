use std::io::Read;
use std::path::Path;

use roxmltree::{Document, Node};
use zip::ZipArchive;

use super::{Cover, Extracted, ExtractedMetadata};
use crate::error::LibraryError;
use bokhylle_core::identity::parse_isbn;

const MAX_ENTRIES: usize = 10_000;
const MAX_EXPANDED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_XML_BYTES: u64 = 4 * 1024 * 1024;
const MAX_COVER_BYTES: u64 = 16 * 1024 * 1024;

pub fn extract(path: &Path) -> Result<Extracted, LibraryError> {
    extract_with_filename(path, path)
}

pub(crate) fn extract_with_filename(
    path: &Path,
    filename: &Path,
) -> Result<Extracted, LibraryError> {
    let file = std::fs::File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() > MAX_ENTRIES {
        return Err(invalid(path, "too many EPUB entries"));
    }
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        expanded = expanded.saturating_add(entry.size());
        if expanded > MAX_EXPANDED_BYTES {
            return Err(invalid(path, "EPUB expands beyond the size limit"));
        }
    }

    let container = read_entry(&mut archive, path, "META-INF/container.xml", MAX_XML_BYTES)?
        .ok_or_else(|| invalid(path, "missing META-INF/container.xml"))?;
    let container_text = String::from_utf8_lossy(&container);
    let container_doc = Document::parse(&container_text)?;

    let opf_path = container_doc
        .descendants()
        .find(|node| node.tag_name().name() == "rootfile")
        .and_then(|node| node.attribute("full-path"))
        .ok_or_else(|| invalid(path, "container.xml has no rootfile"))?
        .to_string();

    let opf_bytes = read_entry(&mut archive, path, &opf_path, MAX_XML_BYTES)?
        .ok_or_else(|| invalid(path, "opf file referenced by container.xml is missing"))?;
    let opf_text = String::from_utf8_lossy(&opf_bytes);
    let opf = Document::parse(&opf_text)?;

    let mut metadata = ExtractedMetadata::default();
    let mut cover_id: Option<String> = None;
    let mut items: Vec<ManifestItem> = Vec::new();

    for node in opf.descendants().filter(Node::is_element) {
        match node.tag_name().name() {
            "title" => {
                if metadata.title.is_none() {
                    metadata.title = text_of(&node);
                }
            }
            "creator" => {
                if let Some(name) = text_of(&node)
                    && !metadata.authors.contains(&name)
                    && creator_is_author(&node)
                {
                    metadata.authors.push(name);
                }
            }
            "language" => {
                if metadata.language.is_none() {
                    metadata.language = text_of(&node);
                }
            }
            "identifier" => {
                if let Some(value) = text_of(&node)
                    && let Some(isbn) = parse_isbn(&value)
                {
                    metadata.isbn.get_or_insert(isbn);
                }
            }
            "publisher" => {
                if metadata.publisher.is_none() {
                    metadata.publisher = text_of(&node);
                }
            }
            "date" => {
                if metadata.year.is_none() {
                    metadata.year = text_of(&node).and_then(|value| year_from_date(&value));
                }
            }
            "description" => {
                if metadata.description.is_none() {
                    metadata.description = text_of(&node);
                }
            }
            "meta" => {
                let name = node.attribute("name");
                let property = node.attribute("property");
                let content = node.attribute("content");

                match (name, property) {
                    (Some("cover"), _) => {
                        cover_id = content.map(str::to_string);
                    }
                    (Some("calibre:series"), _) => {
                        metadata.series = content.map(str::to_string);
                    }
                    (Some("calibre:series_index"), _) => {
                        metadata.series_number = content.map(str::to_string);
                    }
                    (_, Some("belongs-to-collection")) => {
                        metadata.series = text_of(&node);
                    }
                    (_, Some("group-position")) => {
                        metadata.series_number = text_of(&node);
                    }
                    _ => {}
                }
            }
            "item" => {
                items.push(ManifestItem {
                    id: node.attribute("id").unwrap_or_default().to_string(),
                    href: node.attribute("href").unwrap_or_default().to_string(),
                    media_type: node.attribute("media-type").unwrap_or_default().to_string(),
                    properties: node.attribute("properties").unwrap_or_default().to_string(),
                });
            }
            _ => {}
        }
    }

    let cover = find_cover(&mut archive, path, &opf_path, &items, cover_id.as_deref())?;

    let (fallback_title, fallback_authors) = super::fallback_from_filename(filename);
    if metadata.title.is_none() {
        metadata.title = fallback_title;
    }
    if metadata.authors.is_empty() {
        let title = metadata.title.clone().unwrap_or_default();
        metadata.authors = fallback_authors
            .into_iter()
            .filter(|author| !super::title_like_author(&title, author))
            .collect();
    }

    Ok(Extracted { metadata, cover })
}

struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    properties: String,
}

fn find_cover(
    archive: &mut ZipArchive<std::fs::File>,
    path: &Path,
    opf_path: &str,
    items: &[ManifestItem],
    cover_id: Option<&str>,
) -> Result<Option<Cover>, LibraryError> {
    let by_cover_id = cover_id.and_then(|id| items.iter().find(|item| item.id == id));
    let by_property = items.iter().find(|item| {
        item.properties
            .split_whitespace()
            .any(|p| p == "cover-image")
    });
    let by_name = items.iter().find(|item| {
        item.media_type.starts_with("image/")
            && (item.id.to_ascii_lowercase().contains("cover")
                || item.href.to_ascii_lowercase().contains("cover"))
    });

    let Some(item) = by_property.or(by_cover_id).or(by_name) else {
        return Ok(None);
    };

    let Some(entry_path) = resolve_href(opf_path, &item.href) else {
        return Ok(None);
    };
    let bytes = match read_entry(archive, path, &entry_path, MAX_COVER_BYTES)? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };

    let media_type = if item.media_type.is_empty() {
        guess_media_type(&entry_path)
    } else {
        item.media_type.clone()
    };

    Ok(Some(Cover::new(bytes, &media_type)))
}

fn resolve_href(opf_path: &str, href: &str) -> Option<String> {
    let href = href.split('#').next().unwrap_or(href);
    if href.is_empty() {
        return None;
    }

    let decoded = percent_decode(href);
    let base = Path::new(opf_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let joined = base.join(decoded);
    Some(joined.to_string_lossy().replace('\\', "/"))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push(high << 4 | low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn guess_media_type(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "image/jpeg",
    }
    .to_string()
}

fn text_of(node: &Node) -> Option<String> {
    let text: String = node
        .descendants()
        .filter(|child| child.is_text())
        .filter_map(|child| child.text())
        .collect::<String>()
        .trim()
        .to_string();

    if text.is_empty() { None } else { Some(text) }
}

fn creator_is_author(node: &Node) -> bool {
    node.attributes()
        .find(|attribute| attribute.name() == "role")
        .map(|attribute| {
            matches!(
                attribute.value().to_ascii_lowercase().as_str(),
                "aut" | "author"
            )
        })
        .unwrap_or(true)
}

fn year_from_date(value: &str) -> Option<i32> {
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() < 4 {
        return None;
    }
    digits[..4].parse().ok()
}

fn read_entry(
    archive: &mut ZipArchive<std::fs::File>,
    path: &Path,
    name: &str,
    max_bytes: u64,
) -> Result<Option<Vec<u8>>, LibraryError> {
    match archive.by_name(name) {
        Ok(mut entry) => {
            if entry.size() > max_bytes {
                return Err(invalid(
                    path,
                    "EPUB metadata or cover exceeds the size limit",
                ));
            }
            let mut bytes = Vec::new();
            entry.by_ref().take(max_bytes + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > max_bytes {
                return Err(invalid(
                    path,
                    "EPUB metadata or cover exceeds the size limit",
                ));
            }
            Ok(Some(bytes))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn invalid(path: &Path, reason: &str) -> LibraryError {
    LibraryError::Invalid {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn rejects_compressed_oversized_metadata_and_covers() {
        for (name, bytes) in [
            ("META-INF/container.xml", MAX_XML_BYTES + 1),
            ("content.opf", MAX_XML_BYTES + 1),
            ("cover.png", MAX_COVER_BYTES + 1),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("Oversized.epub");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            if name != "META-INF/container.xml" {
                zip.start_file("META-INF/container.xml", options).unwrap();
                zip.write_all(br#"<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>"#).unwrap();
            }
            if name == "cover.png" {
                zip.start_file("content.opf", options).unwrap();
                zip.write_all(br#"<package><metadata><title>Oversized</title></metadata><manifest><item href="cover.png" media-type="image/png" properties="cover-image"/></manifest></package>"#).unwrap();
            }
            zip.start_file(name, options).unwrap();
            let block = [0u8; 8192];
            let mut remaining = bytes;
            while remaining > 0 {
                let count = remaining.min(block.len() as u64) as usize;
                zip.write_all(&block[..count]).unwrap();
                remaining -= count as u64;
            }
            zip.finish().unwrap();
            assert!(std::fs::metadata(&path).unwrap().len() < 128 * 1024);
            let error = extract(&path).unwrap_err();
            assert!(error.to_string().contains("size limit"), "{name}: {error}");
        }
    }
}
