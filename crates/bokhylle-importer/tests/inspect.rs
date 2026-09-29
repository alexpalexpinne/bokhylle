use std::io::Write;
use std::path::Path;

use bokhylle_importer::{ExpectedImport, Inspection, Limits, ReviewReason, inspect};

fn expected_hail_mary() -> ExpectedImport {
    ExpectedImport {
        title: "Project Hail Mary".to_string(),
        authors: vec!["Andy Weir".to_string()],
        isbn: Some("9780593135204".to_string()),
        language: Some("en".to_string()),
    }
}

fn fixture_files() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let written = bokhylle_library::fixtures::generate_library(dir.path(), 2).unwrap();

    let hail_mary = written
        .iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains("Project Hail Mary"))
        })
        .unwrap()
        .clone();
    let other = written
        .iter()
        .find(|path| path != &&hail_mary && path.extension().is_some_and(|ext| ext == "epub"))
        .unwrap()
        .clone();

    (dir, hail_mary, other)
}

fn write_zip(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.zip");

    let file = std::fs::File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();

    for (name, bytes) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();

    (dir, path)
}

#[test]
fn selects_a_strong_single_candidate() {
    let (_dir, hail_mary, _other) = fixture_files();
    let content = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();

    let target = content.path().join("book.epub");
    std::fs::copy(&hail_mary, &target).unwrap();

    let inspection = inspect(
        content.path(),
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::Selected(candidate) => {
            assert_eq!(candidate.format, "epub");
            assert_eq!(candidate.confidence, 1.0);
            assert!(candidate.reasons.contains(&"isbn match".to_string()));
            assert!(
                candidate
                    .reasons
                    .contains(&"strong title match".to_string())
            );
        }
        other => panic!("expected selection, got {other:?}"),
    }
}

#[test]
fn extracts_zip_archives_before_scoring() {
    let (_dir, hail_mary, _other) = fixture_files();
    let staging = tempfile::tempdir().unwrap();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_zip_dir, zip_path) = write_zip(&[("nested/Project Hail Mary.epub", &epub_bytes)]);

    let inspection = inspect(
        &zip_path,
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::Selected(candidate) => {
            assert!(candidate.path.starts_with(staging.path()));
            assert_eq!(candidate.confidence, 1.0);
        }
        other => panic!("expected selection, got {other:?}"),
    }
}

#[test]
fn nested_cbz_is_offered_for_review() {
    let comic_dir = tempfile::tempdir().unwrap();
    let comic_path = comic_dir.path().join("Fictional Comic.cbz");
    let mut comic = zip::ZipWriter::new(std::fs::File::create(&comic_path).unwrap());
    comic
        .start_file("page1.png", zip::write::SimpleFileOptions::default())
        .unwrap();
    comic.write_all(b"\x89PNG\r\n\x1a\nfictional").unwrap();
    comic.finish().unwrap();
    let comic_bytes = std::fs::read(&comic_path).unwrap();
    let (_outer_dir, outer) = write_zip(&[("Fictional Comic.cbz", &comic_bytes)]);
    let staging = tempfile::tempdir().unwrap();
    let expected = ExpectedImport {
        title: "Fictional Comic".into(),
        authors: Vec::new(),
        isbn: None,
        language: None,
    };
    let inspected = inspect(&outer, staging.path(), &expected, &Limits::default()).unwrap();
    match inspected {
        Inspection::NeedsReview { reason, candidates } => {
            assert_eq!(reason, ReviewReason::LowConfidence);
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].format, "cbz");
            assert!(candidates[0].path.starts_with(staging.path()));
        }
        other => panic!("expected a reviewable CBZ, got {other:?}"),
    }
}

#[test]
fn zip_slip_entries_are_ignored() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (zip_dir, zip_path) = write_zip(&[("../evil.epub", &epub_bytes)]);
    let staging = tempfile::tempdir().unwrap();

    let inspection = inspect(
        &zip_path,
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    assert!(matches!(inspection, Inspection::Empty { .. }));
    assert!(!zip_dir.path().join("evil.epub").exists());
    assert!(!staging.path().parent().unwrap().join("evil.epub").exists());
}

#[test]
fn archive_limits_are_enforced() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_zip_dir, zip_path) = write_zip(&[("book.epub", &epub_bytes)]);

    let limits = Limits {
        max_uncompressed_bytes: 16,
        ..Default::default()
    };
    let staging = tempfile::tempdir().unwrap();

    let error = inspect(&zip_path, staging.path(), &expected_hail_mary(), &limits).unwrap_err();
    assert!(matches!(
        error,
        bokhylle_importer::ImportError::ArchiveLimit(_)
    ));
}

#[test]
fn empty_content_reports_empty() {
    let content = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();

    let inspection = inspect(
        content.path(),
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::Empty { reason } => assert!(reason.contains("no supported")),
        other => panic!("expected empty, got {other:?}"),
    }
}

#[test]
fn mismatched_book_needs_review() {
    let (_dir, _hail_mary, other) = fixture_files();
    let content = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    std::fs::copy(&other, content.path().join("other.epub")).unwrap();

    let inspection = inspect(
        content.path(),
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::NeedsReview { reason, candidates } => {
            assert_eq!(reason, ReviewReason::LowConfidence);
            assert_eq!(candidates.len(), 1);
            assert!(candidates[0].confidence < 0.7);
        }
        other => panic!("expected review, got {other:?}"),
    }
}

#[test]
fn two_equal_candidates_need_review() {
    let (_dir, hail_mary, _other) = fixture_files();
    let content = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    std::fs::copy(&hail_mary, content.path().join("a.epub")).unwrap();
    std::fs::copy(&hail_mary, content.path().join("b.epub")).unwrap();

    let inspection = inspect(
        content.path(),
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::NeedsReview { reason, candidates } => {
            assert_eq!(reason, ReviewReason::MultipleCandidates);
            assert_eq!(candidates.len(), 2);
        }
        other => panic!("expected review, got {other:?}"),
    }
}

#[test]
fn junk_files_are_ignored() {
    let (_dir, hail_mary, _other) = fixture_files();
    let content = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    std::fs::copy(&hail_mary, content.path().join("Project Hail Mary.epub")).unwrap();
    std::fs::write(content.path().join("cover.jpg"), b"not an ebook").unwrap();
    std::fs::write(content.path().join(".DS_Store"), b"junk").unwrap();
    std::fs::create_dir_all(content.path().join(".hidden")).unwrap();
    std::fs::copy(&hail_mary, content.path().join(".hidden").join("dupe.epub")).unwrap();

    let inspection = inspect(
        content.path(),
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::Selected(candidate) => {
            assert!(Path::new(&candidate.path).ends_with("Project Hail Mary.epub"));
        }
        other => panic!("expected selection, got {other:?}"),
    }
}

#[test]
fn per_file_limit_is_enforced_while_streaming() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_zip_dir, zip_path) = write_zip(&[("book.epub", &epub_bytes)]);
    let staging = tempfile::tempdir().unwrap();

    let limits = Limits {
        max_file_bytes: 16,
        ..Default::default()
    };

    let error = inspect(&zip_path, staging.path(), &expected_hail_mary(), &limits).unwrap_err();
    assert!(matches!(
        error,
        bokhylle_importer::ImportError::ArchiveLimit(_)
    ));

    fn count_files(dir: &std::path::Path) -> usize {
        let mut count = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                count += count_files(&entry.path());
            } else {
                count += 1;
            }
        }
        count
    }

    assert_eq!(
        count_files(staging.path()),
        0,
        "failed extraction must not leave partial files"
    );
}

#[test]
fn entries_larger_than_the_limit_are_rejected_before_writing() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_zip_dir, zip_path) = write_zip(&[("big.epub", &epub_bytes)]);
    let staging = tempfile::tempdir().unwrap();

    let limits = Limits {
        max_file_bytes: 4,
        max_uncompressed_bytes: 1_000_000,
        ..Default::default()
    };

    assert!(inspect(&zip_path, staging.path(), &expected_hail_mary(), &limits).is_err());
}

fn crc32(init: u32, data: &[u8]) -> u32 {
    let mut crc = init;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB88320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

fn rar_block(block_type: u8, flags: u16, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(5 + payload.len());
    body.push(block_type);
    body.extend_from_slice(&flags.to_le_bytes());
    body.extend_from_slice(&((7 + payload.len()) as u16).to_le_bytes());
    body.extend_from_slice(payload);

    let checksum = (!crc32(0xffff_ffff, &body)) & 0xffff;

    let mut block = Vec::with_capacity(2 + body.len());
    block.extend_from_slice(&(checksum as u16).to_le_bytes());
    block.extend_from_slice(&body);
    block
}

fn write_epub(dir: &Path, filename: &str, title: &str, author: &str) -> std::path::PathBuf {
    let path = dir.join(filename);
    let file = std::fs::File::create(&path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default();

    let opf = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>{title}</dc:title>
    <dc:creator>{author}</dc:creator>
    <dc:language>en</dc:language>
    <dc:identifier id="bookid">urn:uuid:00000000-0000-0000-0000-000000000001</dc:identifier>
  </metadata>
  <manifest>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="chapter1"/>
  </spine>
</package>
"#
    );

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
        .write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>x</p></body></html>"#)
        .unwrap();
    writer.finish().unwrap();

    path
}

fn expected_dune() -> ExpectedImport {
    ExpectedImport {
        title: "Dune".to_string(),
        authors: vec!["Frank Herbert".to_string()],
        isbn: None,
        language: Some("en".to_string()),
    }
}

fn write_rar(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("archive.rar");

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"Rar!\x1a\x07\x00");
    bytes.extend_from_slice(&rar_block(0x73, 0, &[0u8; 6]));

    for (name, data) in entries {
        let name = name.as_bytes();
        let mut payload = Vec::new();
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.push(3);
        payload.extend_from_slice(&(crc32(0xffff_ffff, data) ^ 0xffff_ffff).to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.push(29);
        payload.push(0x30);
        payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
        payload.extend_from_slice(&0x81A4u32.to_le_bytes());
        payload.extend_from_slice(name);

        bytes.extend_from_slice(&rar_block(0x74, 0x8020, &payload));
        bytes.extend_from_slice(data);
    }

    bytes.extend_from_slice(&rar_block(0x7b, 0, &[]));
    std::fs::write(&path, bytes).unwrap();

    (dir, path)
}

#[test]
fn single_word_titles_do_not_match_longer_metadata_titles() {
    let content = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    write_epub(content.path(), "book.epub", "Dune Messiah", "Frank Herbert");

    let inspection = inspect(
        content.path(),
        staging.path(),
        &expected_dune(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::NeedsReview { candidates, .. } => {
            assert!(candidates[0].confidence < 0.7, "confidence too high");
        }
        other => panic!("expected review, got {other:?}"),
    }
}

#[test]
fn single_word_titles_match_exact_metadata_titles() {
    for title in ["Dune", "Dune: A Novel", "Dune (Dune Chronicles #1)"] {
        let content = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        write_epub(content.path(), "book.epub", title, "Frank Herbert");

        let inspection = inspect(
            content.path(),
            staging.path(),
            &expected_dune(),
            &Limits::default(),
        )
        .unwrap();

        match inspection {
            Inspection::Selected(candidate) => assert!(candidate.confidence >= 0.7),
            other => panic!("expected selection for {title}, got {other:?}"),
        }
    }
}

#[test]
fn extracts_epub_from_nested_zip() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_inner_dir, inner_zip) = write_zip(&[("book.epub", &epub_bytes)]);
    let inner_bytes = std::fs::read(&inner_zip).unwrap();
    let (_outer_dir, outer_zip) = write_zip(&[("pack/bbc5ajza.zip", &inner_bytes)]);
    let staging = tempfile::tempdir().unwrap();

    let inspection = inspect(
        &outer_zip,
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::Selected(candidate) => {
            assert!(candidate.path.starts_with(staging.path()));
            assert_eq!(candidate.confidence, 1.0);
        }
        other => panic!("expected selection, got {other:?}"),
    }
}

#[test]
fn extracts_epub_from_rar() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_rar_dir, rar_path) = write_rar(&[
        ("bb-Strange.Dogs.epub", &epub_bytes),
        ("file_id.diz", b"not an ebook"),
    ]);
    let staging = tempfile::tempdir().unwrap();

    let inspection = inspect(
        &rar_path,
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    match inspection {
        Inspection::Selected(candidate) => {
            assert!(candidate.path.starts_with(staging.path()));
            assert_eq!(candidate.confidence, 1.0);
        }
        other => panic!("expected selection, got {other:?}"),
    }
}

#[test]
fn rar_entries_larger_than_the_limit_are_rejected_before_writing() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_rar_dir, rar_path) = write_rar(&[("book.epub", &epub_bytes)]);
    let staging = tempfile::tempdir().unwrap();

    let limits = Limits {
        max_file_bytes: epub_bytes.len() as u64 - 1,
        ..Default::default()
    };
    let error = inspect(&rar_path, staging.path(), &expected_hail_mary(), &limits).unwrap_err();

    assert!(matches!(
        error,
        bokhylle_importer::ImportError::ArchiveLimit(_)
    ));
    assert!(!staging.path().join("1-archive").exists());
}

#[test]
fn rar_slip_entries_are_ignored() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (rar_dir, rar_path) = write_rar(&[("../evil.epub", &epub_bytes)]);
    let staging = tempfile::tempdir().unwrap();

    let inspection = inspect(
        &rar_path,
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    assert!(matches!(inspection, Inspection::Empty { .. }));
    assert!(!rar_dir.path().join("evil.epub").exists());
    assert!(!staging.path().parent().unwrap().join("evil.epub").exists());
}

#[test]
fn archives_are_detected_by_magic_without_an_extension() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_rar_dir, rar_path) = write_rar(&[("book.epub", &epub_bytes)]);
    let rar_bytes = std::fs::read(&rar_path).unwrap();

    let content = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    std::fs::write(content.path().join("download.bin"), &rar_bytes).unwrap();

    let inspection = inspect(
        content.path(),
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    assert!(matches!(inspection, Inspection::Selected(_)));
}

#[cfg(unix)]
#[test]
fn symlinked_ebooks_are_ignored() {
    let (_dir, hail_mary, _other) = fixture_files();
    let content = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(&hail_mary, content.path().join("book.epub")).unwrap();

    let inspection = inspect(
        content.path(),
        staging.path(),
        &expected_hail_mary(),
        &Limits::default(),
    )
    .unwrap();

    assert!(matches!(inspection, Inspection::Empty { .. }));
}

#[test]
fn archive_depth_is_limited() {
    let (_dir, hail_mary, _other) = fixture_files();
    let epub_bytes = std::fs::read(&hail_mary).unwrap();
    let (_inner_dir, inner_zip) = write_zip(&[("book.epub", &epub_bytes)]);
    let inner_bytes = std::fs::read(&inner_zip).unwrap();
    let (_outer_dir, outer_zip) = write_zip(&[("inner.zip", &inner_bytes)]);
    let staging = tempfile::tempdir().unwrap();

    let limits = Limits {
        max_archive_depth: 1,
        ..Default::default()
    };

    let inspection = inspect(&outer_zip, staging.path(), &expected_hail_mary(), &limits).unwrap();

    assert!(matches!(inspection, Inspection::Empty { .. }));
}
