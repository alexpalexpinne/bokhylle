use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

use crate::error::LibraryError;

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

pub fn generate_library(root: &Path, count: usize) -> Result<Vec<PathBuf>, LibraryError> {
    let mut written = Vec::new();
    std::fs::create_dir_all(root)?;

    for index in 0..count {
        let book = fixture_book(index);
        let directory = root.join(&book.author);
        std::fs::create_dir_all(&directory)?;

        let filename = format!(
            "{} - {}.{}",
            sanitize(&book.title),
            sanitize(&book.author),
            book.format
        );
        let path = directory.join(filename);

        let bytes = if book.format == "pdf" {
            pdf_bytes(&book.title, &book.author)
        } else {
            epub_bytes(&book)
        };

        std::fs::write(&path, bytes)?;
        written.push(path);
    }

    std::fs::write(root.join("notes.txt"), b"not a book")?;
    std::fs::write(root.join("cover.jpg"), b"fake image")?;
    std::fs::write(root.join(".DS_Store"), b"junk")?;
    std::fs::write(root.join("Broken.epub"), b"this is not a zip archive")?;

    if let Some(first) = written.first() {
        let bytes = std::fs::read(first)?;
        let duplicate = root.join("duplicates").join(first.file_name().unwrap());
        std::fs::create_dir_all(duplicate.parent().unwrap())?;
        std::fs::write(&duplicate, bytes)?;
    }

    let bare = root.join("Anonymous").join("Mystery Book.epub");
    std::fs::create_dir_all(bare.parent().unwrap())?;
    std::fs::write(&bare, epub_bytes_without_metadata())?;
    written.push(bare);

    Ok(written)
}

struct FixtureBook {
    title: String,
    author: String,
    language: Option<String>,
    isbn: Option<String>,
    series: Option<String>,
    series_number: Option<String>,
    year: i32,
    format: &'static str,
    include_metadata: bool,
}

fn fixture_book(index: usize) -> FixtureBook {
    if index == 0 {
        return FixtureBook {
            title: "Project Hail Mary".to_string(),
            author: "Andy Weir".to_string(),
            language: Some("en".to_string()),
            isbn: Some("9780593135204".to_string()),
            series: None,
            series_number: None,
            year: 2021,
            format: "epub",
            include_metadata: true,
        };
    }

    let first = [
        "Silent", "Hidden", "Crimson", "Northern", "Golden", "Broken", "Infinite", "Last",
    ];
    let second = [
        "River", "Empire", "Garden", "Machine", "Winter", "Library", "Signal", "Orchard",
    ];
    let authors = [
        "Astrid Lind",
        "Erik Vale",
        "Maria Storm",
        "Jonas Berg",
        "Elena Frost",
    ];

    let title = format!(
        "{} {} {}",
        first[index % first.len()],
        second[(index / first.len()) % second.len()],
        roman(index + 1)
    );

    let language = match index % 4 {
        0 => Some("en".to_string()),
        1 => Some("sv".to_string()),
        2 => Some("de".to_string()),
        _ => None,
    };
    let isbn = match index % 3 {
        0 => Some(generated_isbn13(index)),
        1 => Some(generated_isbn10(index)),
        _ => None,
    };

    FixtureBook {
        title,
        author: authors[index % authors.len()].to_string(),
        language,
        isbn,
        series: index
            .is_multiple_of(5)
            .then(|| format!("The {} Cycle", second[index % second.len()])),
        series_number: index
            .is_multiple_of(5)
            .then(|| format!("{}", index % 7 + 1)),
        year: 1990 + (index as i32 % 35),
        format: if index % 4 == 3 { "pdf" } else { "epub" },
        include_metadata: index % 7 != 6,
    }
}

fn epub_bytes(book: &FixtureBook) -> Vec<u8> {
    let metadata = if book.include_metadata {
        let mut parts = String::new();
        if let Some(isbn) = &book.isbn {
            parts.push_str(&format!(
                "    <dc:identifier id=\"bookid\">urn:isbn:{isbn}</dc:identifier>\n"
            ));
        }
        parts.push_str(&format!(
            "    <dc:title>{}</dc:title>\n",
            escape_xml(&book.title)
        ));
        parts.push_str(&format!(
            "    <dc:creator>{}</dc:creator>\n",
            escape_xml(&book.author)
        ));
        if let Some(language) = &book.language {
            parts.push_str(&format!(
                "    <dc:language>{}</dc:language>\n",
                escape_xml(language)
            ));
        }
        parts.push_str("    <dc:publisher>Fixture Press</dc:publisher>\n");
        parts.push_str(&format!("    <dc:date>{}-01-01</dc:date>\n", book.year));
        if let Some(series) = &book.series {
            parts.push_str(&format!(
                "    <meta property=\"belongs-to-collection\">{}</meta>\n",
                escape_xml(series)
            ));
        }
        if let Some(number) = &book.series_number {
            parts.push_str(&format!(
                "    <meta property=\"group-position\">{number}</meta>\n"
            ));
        }
        parts
    } else {
        String::new()
    };

    let opf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
{metadata}  </metadata>
  <manifest>
    <item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chapter1"/>
  </spine>
</package>
"#
    );

    let container = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

    let chapter = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter</title></head>
<body><p>Public domain fixture text.</p></body></html>
"#;

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default();

        writer.start_file("mimetype", stored).unwrap();
        writer.write_all(b"application/epub+zip").unwrap();

        writer
            .start_file("META-INF/container.xml", deflated)
            .unwrap();
        writer.write_all(container.as_bytes()).unwrap();

        writer.start_file("OEBPS/content.opf", deflated).unwrap();
        writer.write_all(opf.as_bytes()).unwrap();

        writer.start_file("OEBPS/cover.png", deflated).unwrap();
        writer.write_all(PNG_1X1).unwrap();

        writer.start_file("OEBPS/chapter1.xhtml", deflated).unwrap();
        writer.write_all(chapter.as_bytes()).unwrap();

        writer.finish().unwrap();
    }

    buffer.into_inner()
}

fn epub_bytes_without_metadata() -> Vec<u8> {
    let opf = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"></metadata>
  <manifest>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="chapter1"/></spine>
</package>
"#;

    let container = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let stale = SimpleFileOptions::default();
        writer.start_file("META-INF/container.xml", stale).unwrap();
        writer.write_all(container.as_bytes()).unwrap();
        writer.start_file("OEBPS/content.opf", stale).unwrap();
        writer.write_all(opf.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buffer.into_inner()
}

fn pdf_bytes(title: &str, author: &str) -> Vec<u8> {
    let content = format!("BT /F1 24 Tf 72 700 Td ({}) Tj ET", escape_pdf(title));
    let objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            content
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        format!(
            "<< /Title ({}) /Author ({}) /CreationDate (D:20200101000000Z) >>",
            escape_pdf(title),
            escape_pdf(author)
        ),
    ];

    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }

    let xref_start = out.len();
    out.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
    out.push_str("0000000000 65535 f \n");
    for offset in &offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref_start}\n%%EOF\n",
        objects.len() + 1
    ));

    out.into_bytes()
}

fn generated_isbn13(index: usize) -> String {
    let body = format!("978{:010}", index + 1);
    let mut sum: u32 = 0;
    for (position, byte) in body.bytes().enumerate() {
        let value = u32::from(byte - b'0');
        let weight = if position.is_multiple_of(2) { 1 } else { 3 };
        sum += weight * value;
    }
    let check = (10 - (sum % 10)) % 10;
    format!("{body}{check}")
}

fn generated_isbn10(index: usize) -> String {
    let body = format!("{:09}", 1_000_000 + index);
    let mut sum: u32 = 0;
    for (position, byte) in body.bytes().enumerate() {
        sum += (10 - position as u32) * u32::from(byte - b'0');
    }
    let check = (11 - (sum % 11)) % 11;
    let check_digit = if check == 10 {
        'X'.to_string()
    } else {
        check.to_string()
    };
    format!("{body}{check_digit}")
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character == ' ' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn roman(mut number: usize) -> String {
    let numerals = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut out = String::new();
    for (value, numeral) in numerals {
        while number >= value {
            out.push_str(numeral);
            number -= value;
        }
    }
    out
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_pdf(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{self, Extracted};
    use bokhylle_core::BookFormat;

    fn extract_fixture(path: &Path) -> Extracted {
        let format = if path.extension().and_then(|e| e.to_str()) == Some("pdf") {
            BookFormat::Pdf
        } else {
            BookFormat::Epub
        };
        extract::extract(path, format).unwrap()
    }

    #[test]
    fn generates_unique_indexable_library() {
        let dir = tempfile::tempdir().unwrap();
        let written = generate_library(dir.path(), 10).unwrap();
        assert_eq!(written.len(), 11);

        let files = crate::scan::scan(dir.path()).unwrap();
        assert!(files.len() >= 11);
        assert!(files.iter().any(|file| file.format == BookFormat::Pdf));
    }

    #[test]
    fn generated_epub_has_metadata_and_cover() {
        let dir = tempfile::tempdir().unwrap();
        generate_library(dir.path(), 1).unwrap();

        let path = dir
            .path()
            .join("Andy Weir")
            .join("Project Hail Mary - Andy Weir.epub");
        let extracted = extract_fixture(&path);

        assert_eq!(
            extracted.metadata.title.as_deref(),
            Some("Project Hail Mary")
        );
        assert_eq!(extracted.metadata.authors, vec!["Andy Weir"]);
        assert_eq!(extracted.metadata.isbn.as_deref(), Some("9780593135204"));
        assert_eq!(extracted.metadata.language.as_deref(), Some("en"));
        assert_eq!(extracted.metadata.year, Some(2021));
        assert!(extracted.cover.is_some());
    }

    #[test]
    fn generated_pdf_has_metadata() {
        let dir = tempfile::tempdir().unwrap();
        generate_library(dir.path(), 4).unwrap();

        let pdf = crate::scan::scan(dir.path())
            .unwrap()
            .into_iter()
            .find(|file| file.format == BookFormat::Pdf)
            .unwrap();
        let extracted = extract_fixture(&pdf.path);

        assert!(extracted.metadata.title.is_some());
        assert!(!extracted.metadata.authors.is_empty());
    }

    #[test]
    fn metadata_poor_epub_falls_back_to_filename() {
        let dir = tempfile::tempdir().unwrap();
        generate_library(dir.path(), 1).unwrap();

        let path = dir.path().join("Anonymous").join("Mystery Book.epub");
        let extracted = extract_fixture(&path);
        assert_eq!(extracted.metadata.title.as_deref(), Some("Mystery Book"));
    }

    #[test]
    fn broken_epub_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        generate_library(dir.path(), 1).unwrap();

        let path = dir.path().join("Broken.epub");
        assert!(extract::extract(&path, BookFormat::Epub).is_err());
    }
}
