pub mod archive;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use bokhylle_core::BookFormat;
use bokhylle_core::identity::{
    core_title, isbn10_to_isbn13, normalize_text, parse_isbn, phrase_ratio,
};
use bokhylle_library::extract;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("rar error: {0}")]
    Rar(#[from] rars::Error),
    #[error("library error: {0}")]
    Library(#[from] bokhylle_library::LibraryError),
    #[error("archive limit exceeded: {0}")]
    ArchiveLimit(String),
}

#[derive(Debug, Clone)]
pub struct ExpectedImport {
    pub title: String,
    pub authors: Vec<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    pub path: PathBuf,
    pub format: String,
    pub size: u64,
    pub score: i32,
    pub confidence: f32,
    pub reasons: Vec<String>,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub isbn: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewReason {
    LowConfidence,
    MultipleCandidates,
}

#[derive(Debug)]
pub enum Inspection {
    Selected(ImportCandidate),
    NeedsReview {
        reason: ReviewReason,
        candidates: Vec<ImportCandidate>,
    },
    Empty {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_entries: usize,
    pub max_file_bytes: u64,
    pub max_uncompressed_bytes: u64,
    pub max_files: usize,
    pub max_archive_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 1_000,
            max_file_bytes: 512 * 1024 * 1024,
            max_uncompressed_bytes: 4 * 1024 * 1024 * 1024,
            max_files: 200,
            max_archive_depth: 4,
        }
    }
}

pub const SELECTION_CONFIDENCE: f32 = 0.7;
pub const AMBIGUITY_GAP: f32 = 0.15;

pub fn inspect(
    content: &Path,
    staging: &Path,
    expected: &ExpectedImport,
    limits: &Limits,
) -> Result<Inspection, ImportError> {
    let mut files = collect_files(content, staging, limits)?;
    files.sort();
    files.dedup();

    if files.len() > limits.max_files {
        return Err(ImportError::ArchiveLimit(format!(
            "{} ebook files found which exceeds the limit of {}",
            files.len(),
            limits.max_files
        )));
    }

    if files.is_empty() {
        return Ok(Inspection::Empty {
            reason: "no supported ebook files were found".to_string(),
        });
    }

    let mut candidates: Vec<ImportCandidate> = files
        .into_iter()
        .filter_map(|path| score_candidate(&path, expected).ok())
        .collect();

    candidates.sort_by(|left, right| {
        right.score.cmp(&left.score).then(
            right
                .confidence
                .partial_cmp(&left.confidence)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    let Some(best) = candidates.first() else {
        return Ok(Inspection::Empty {
            reason: "no readable ebook files were found".to_string(),
        });
    };

    if best.confidence < SELECTION_CONFIDENCE {
        return Ok(Inspection::NeedsReview {
            reason: ReviewReason::LowConfidence,
            candidates,
        });
    }

    if candidates.len() > 1 {
        let runner_up = &candidates[1];
        if best.confidence - runner_up.confidence < AMBIGUITY_GAP
            && runner_up.confidence >= SELECTION_CONFIDENCE
        {
            return Ok(Inspection::NeedsReview {
                reason: ReviewReason::MultipleCandidates,
                candidates,
            });
        }
    }

    Ok(Inspection::Selected(best.clone()))
}

fn collect_files(
    content: &Path,
    staging: &Path,
    limits: &Limits,
) -> Result<Vec<PathBuf>, ImportError> {
    let mut files = Vec::new();
    let mut extracted_total: u64 = 0;
    let mut archives: usize = 0;

    collect_entries(
        content,
        staging,
        limits,
        0,
        &mut archives,
        &mut extracted_total,
        &mut files,
    )?;

    Ok(files)
}

fn collect_entries(
    path: &Path,
    staging: &Path,
    limits: &Limits,
    depth: usize,
    archives: &mut usize,
    extracted_total: &mut u64,
    files: &mut Vec<PathBuf>,
) -> Result<(), ImportError> {
    if path.is_file() {
        return collect_file(
            path,
            staging,
            limits,
            depth,
            archives,
            extracted_total,
            files,
        );
    }

    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !is_ignored(&entry.file_name().to_string_lossy())
        })
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if !entry.file_type().is_file() {
            continue;
        }

        collect_file(
            entry.path(),
            staging,
            limits,
            depth,
            archives,
            extracted_total,
            files,
        )?;
    }

    Ok(())
}

fn collect_file(
    path: &Path,
    staging: &Path,
    limits: &Limits,
    depth: usize,
    archives: &mut usize,
    extracted_total: &mut u64,
    files: &mut Vec<PathBuf>,
) -> Result<(), ImportError> {
    let kind = match archive::entry_kind(path) {
        Some(kind) => Some(kind),
        None if archive::detect_archive(path).is_some() => Some(archive::EntryKind::Archive),
        None => None,
    };

    match kind {
        Some(archive::EntryKind::Ebook) => files.push(path.to_path_buf()),
        Some(archive::EntryKind::Archive) => {
            if depth >= limits.max_archive_depth {
                return Ok(());
            }

            *archives += 1;
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "archive".to_string());
            let destination = staging.join(format!("{archives}-{stem}"));

            let extracted = archive::extract_safely(path, &destination, limits, extracted_total)?;

            for entry in extracted {
                collect_file(
                    &entry,
                    staging,
                    limits,
                    depth + 1,
                    archives,
                    extracted_total,
                    files,
                )?;
            }
        }
        None => {}
    }

    Ok(())
}

fn is_ignored(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with("._")
        || matches!(name, "@eaDir" | "Thumbs.db" | "desktop.ini")
}

fn title_variant_ratio(expected: &str, candidate: &str) -> f32 {
    let expected_tokens: Vec<&str> = expected.split_whitespace().collect();
    if expected_tokens.len() == 1 {
        let candidate_tokens: Vec<&str> = candidate.split_whitespace().collect();
        if candidate_tokens.len() == 1 && candidate_tokens[0] == expected_tokens[0] {
            1.0
        } else {
            0.0
        }
    } else {
        phrase_ratio(expected, candidate)
    }
}

fn score_candidate(path: &Path, expected: &ExpectedImport) -> Result<ImportCandidate, ImportError> {
    let format = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(BookFormat::from_extension)
        .unwrap_or(BookFormat::Epub);

    let extracted = extract::extract(path, format)?;
    let metadata = extracted.metadata;

    let expected_titles = {
        let mut titles = vec![normalize_text(&expected.title)];
        let core = normalize_text(&core_title(&expected.title));
        if !core.is_empty() && !titles.contains(&core) {
            titles.push(core);
        }
        titles
    };
    let normalized_title = metadata
        .title
        .as_deref()
        .map(normalize_text)
        .unwrap_or_default();
    let candidate_titles = {
        let mut titles = vec![normalized_title];
        let core = metadata
            .title
            .as_deref()
            .map(core_title)
            .map(|title| normalize_text(&title))
            .unwrap_or_default();
        if !core.is_empty() && !titles.contains(&core) {
            titles.push(core);
        }
        titles
    };

    let title_ratio = expected_titles
        .iter()
        .flat_map(|expected| {
            candidate_titles
                .iter()
                .map(move |candidate| title_variant_ratio(expected, candidate))
        })
        .fold(0.0_f32, f32::max);

    let author_match = expected.authors.iter().any(|author| {
        let author = normalize_text(author);
        !author.is_empty()
            && metadata
                .authors
                .iter()
                .any(|candidate| normalize_text(candidate).contains(&author))
    });

    let expected_isbn = expected
        .isbn
        .as_deref()
        .and_then(parse_isbn)
        .map(|isbn| (isbn.clone(), isbn10_to_isbn13(&isbn)));
    let candidate_isbn = metadata.isbn.as_deref().and_then(parse_isbn);
    let isbn_match = match (&expected_isbn, &candidate_isbn) {
        (Some((expected10, expected13)), Some(candidate)) => {
            candidate == expected10 || expected13.as_deref() == Some(candidate.as_str())
        }
        _ => false,
    };

    let strong_title = title_ratio >= 1.0;

    let mut score = 0;
    let mut reasons = Vec::new();

    if strong_title {
        score += 40;
        reasons.push("strong title match".to_string());
    } else if title_ratio >= 0.6 {
        score += 20;
        reasons.push("partial title match".to_string());
    }

    if isbn_match {
        score += 50;
        reasons.push("isbn match".to_string());
    }

    if author_match {
        score += 20;
        reasons.push("author match".to_string());
    }

    let confidence = (title_ratio.clamp(0.0, 1.0) * 0.5
        + if isbn_match { 0.3 } else { 0.0 }
        + if author_match { 0.2 } else { 0.0 })
    .clamp(0.0, 1.0);

    let size = std::fs::metadata(path)?.len();

    Ok(ImportCandidate {
        path: path.to_path_buf(),
        format: format.as_str().to_string(),
        size,
        score,
        confidence,
        reasons,
        title: metadata.title,
        authors: metadata.authors,
        isbn: metadata.isbn,
    })
}
