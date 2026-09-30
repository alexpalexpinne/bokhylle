pub mod cbz;
pub mod epub;
pub mod pdf;

use std::path::Path;

use bokhylle_core::BookFormat;
use bokhylle_core::identity::normalize_text;

use crate::error::LibraryError;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractedMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub title_from_filename: bool,
    pub authors_from_filename: bool,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
    pub year: Option<i32>,
    pub publisher: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    pub bytes: Vec<u8>,
    pub media_type: String,
    pub extension: &'static str,
}

impl Cover {
    pub fn new(bytes: Vec<u8>, media_type: &str) -> Self {
        let extension = match media_type {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            _ => "img",
        };
        Self {
            bytes,
            media_type: media_type.to_string(),
            extension,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub metadata: ExtractedMetadata,
    pub cover: Option<Cover>,
}

pub fn extract(path: &Path, format: BookFormat) -> Result<Extracted, LibraryError> {
    extract_with_filename(path, format, path)
}

/// Read staged bytes while preserving the original filename for missing metadata.
pub fn extract_with_filename(
    path: &Path,
    format: BookFormat,
    filename: &Path,
) -> Result<Extracted, LibraryError> {
    match format {
        BookFormat::Epub => epub::extract_with_filename(path, filename),
        BookFormat::Pdf => pdf::extract_with_filename(path, filename),
        BookFormat::Cbz => cbz::extract_with_filename(path, filename),
    }
}

/// Normalizes a language tag to its lower-case primary subtag, treating
/// placeholders ("und", "undetermined", "unknown", "zxx") as missing.
pub fn normalize_language(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let code = trimmed
        .split(['-', '_'])
        .next()
        .unwrap_or(trimmed)
        .to_ascii_lowercase();

    if code.is_empty() || matches!(code.as_str(), "und" | "undetermined" | "unknown" | "zxx") {
        None
    } else {
        Some(code)
    }
}

/// True when a filename-derived "author" is really the book title with
/// packer noise (for example "And Another Thing . (v5.0)").
pub fn title_like_author(title: &str, author: &str) -> bool {
    let title = normalize_text(title);
    if title.is_empty() {
        return false;
    }

    let normalized_author = normalize_text(author);
    let tokens: Vec<&str> = normalized_author.split_whitespace().collect();
    let mut keep = tokens.len();
    while keep > 0 {
        let token = tokens[keep - 1];
        let numeric = token.strip_prefix('v').unwrap_or(token);
        if !numeric.is_empty() && numeric.chars().all(|character| character.is_ascii_digit()) {
            keep -= 1;
        } else {
            break;
        }
    }

    let stripped = tokens[..keep].join(" ");
    !stripped.is_empty() && stripped == title
}

pub fn fallback_from_filename(path: &Path) -> (Option<String>, Vec<String>) {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return (None, Vec::new());
    };

    let cleaned = stem.replace('_', " ");
    let cleaned = cleaned.trim();

    if let Some((title, author)) = cleaned.split_once(" - ") {
        let title = title.trim();
        let author = author.trim();
        if title_like_author(title, author) {
            return (Some(title.to_string()), Vec::new());
        }
        let author = author.trim();
        if !title.is_empty() && !author.is_empty() {
            return (Some(title.to_string()), vec![author.to_string()]);
        }
    }

    if cleaned.is_empty() {
        (None, Vec::new())
    } else {
        (Some(cleaned.to_string()), Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_and_author_from_filename() {
        let (title, authors) =
            fallback_from_filename(Path::new("/library/Project Hail Mary - Andy Weir.epub"));
        assert_eq!(title.as_deref(), Some("Project Hail Mary"));
        assert_eq!(authors, vec!["Andy Weir"]);
    }

    #[test]
    fn uses_stem_without_separator_as_title() {
        let (title, authors) = fallback_from_filename(Path::new("/library/Dune.pdf"));
        assert_eq!(title.as_deref(), Some("Dune"));
        assert!(authors.is_empty());
    }

    #[test]
    fn replaces_underscores_in_fallback() {
        let (title, _) = fallback_from_filename(Path::new("/library/Project_Hail_Mary.epub"));
        assert_eq!(title.as_deref(), Some("Project Hail Mary"));
    }

    #[test]
    fn drops_filename_authors_that_are_the_title() {
        let (title, authors) = fallback_from_filename(Path::new(
            "/library/And Another Thing __ - And Another Thing . (v5.0).epub",
        ));
        assert_eq!(title.as_deref(), Some("And Another Thing"));
        assert!(authors.is_empty(), "authors: {authors:?}");
    }

    #[test]
    fn keeps_real_filename_authors() {
        let (_, authors) = fallback_from_filename(Path::new(
            "/library/And Another Thing __ - Colfer, Eoin.epub",
        ));
        assert_eq!(authors, vec!["Colfer, Eoin"]);
    }

    #[test]
    fn epub_creator_role_author_is_accepted() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("And Another Thing.epub");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default();

        let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:creator opf:role="author">Colfer, Eoin</dc:creator>
    <dc:title>And Another Thing ...</dc:title>
    <dc:language>en</dc:language>
    <dc:identifier id="id">URN:ISBN: 978-0-14-193299-6</dc:identifier>
  </metadata>
  <manifest>
    <item id="c1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="c1"/></spine>
</package>"#;

        writer.start_file("mimetype", stored).unwrap();
        writer.write_all(b"application/epub+zip").unwrap();
        writer
            .start_file("META-INF/container.xml", deflated)
            .unwrap();
        writer
            .write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();
        writer.start_file("OEBPS/content.opf", deflated).unwrap();
        writer.write_all(opf.as_bytes()).unwrap();
        writer.start_file("OEBPS/chapter1.xhtml", deflated).unwrap();
        writer
            .write_all(
                br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>x</p></body></html>"#,
            )
            .unwrap();
        writer.finish().unwrap();

        let extracted = extract(&path, BookFormat::Epub).unwrap();
        assert_eq!(extracted.metadata.authors, vec!["Colfer, Eoin"]);
    }

    #[test]
    fn language_tags_normalize_and_drop_placeholders() {
        assert_eq!(normalize_language("EN"), Some("en".to_string()));
        assert_eq!(normalize_language("en-US"), Some("en".to_string()));
        assert_eq!(normalize_language(" sv_SE "), Some("sv".to_string()));
        assert_eq!(normalize_language("und"), None);
        assert_eq!(normalize_language("undetermined"), None);
        assert_eq!(normalize_language("unknown"), None);
        assert_eq!(normalize_language("zxx"), None);
        assert_eq!(normalize_language("   "), None);
    }
}
