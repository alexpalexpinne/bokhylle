use std::path::Path;

use lopdf::{Dictionary, Document};

use super::{Extracted, ExtractedMetadata};
use crate::error::LibraryError;

pub fn extract(path: &Path) -> Result<Extracted, LibraryError> {
    extract_with_filename(path, path)
}

pub(crate) fn extract_with_filename(
    path: &Path,
    filename: &Path,
) -> Result<Extracted, LibraryError> {
    let document = Document::load(path)?;

    let mut metadata = ExtractedMetadata::default();

    if let Ok(info) = document.trailer.get_deref(b"Info", &document)
        && let Ok(info) = info.as_dict()
    {
        metadata.title = string_entry(&document, info, b"Title");
        metadata.authors = string_entry(&document, info, b"Author")
            .into_iter()
            .collect();
        metadata.publisher = string_entry(&document, info, b"Producer");
        metadata.year = string_entry(&document, info, b"CreationDate")
            .as_deref()
            .and_then(year_from_pdf_date);
    }

    let (fallback_title, fallback_authors) = super::fallback_from_filename(filename);
    if metadata.title.is_none() {
        metadata.title = fallback_title;
    }
    if metadata.authors.is_empty() {
        metadata.authors = fallback_authors;
    }

    Ok(Extracted {
        metadata,
        cover: None,
    })
}

fn string_entry(document: &Document, info: &Dictionary, key: &[u8]) -> Option<String> {
    let object = info.get_deref(key, document).ok()?;
    let bytes = object.as_str().ok()?;
    let text = decode_pdf_string(bytes);
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());

    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn decode_pdf_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) && bytes.len().is_multiple_of(2) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_be_bytes(*chunk))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn year_from_pdf_date(value: &str) -> Option<i32> {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 4 {
        return None;
    }
    digits[..4].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf16be_pdf_strings() {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in "Brontë".encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        assert_eq!(decode_pdf_string(&bytes), "Brontë");
    }

    #[test]
    fn parses_year_from_pdf_date() {
        assert_eq!(year_from_pdf_date("D:20210101120000Z"), Some(2021));
        assert_eq!(year_from_pdf_date("D:2021"), Some(2021));
        assert_eq!(year_from_pdf_date("junk"), None);
    }
}
