use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use bokhylle_acquisition::qbittorrent::{DEFAULT_CATEGORY, TAG_PREFIX, TorrentInfo};
use bokhylle_core::BookFormat;
use bokhylle_core::identity::{core_title, normalize_text};
use bokhylle_importer::{ExpectedImport, ImportCandidate, Inspection, Limits};

use crate::AppState;
use crate::error::AppError;
use crate::settings;

const MAX_REPORTED: usize = 25;
/// Collection torrents (anthologies, "top 100" bundles) can hold hundreds of
/// books; the archive limits still guard against decompression bombs.
const MAX_ADOPTED_FILES: usize = 1_000;

#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportJobStatus {
    pub running: bool,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub imported: u64,
    pub already: u64,
    pub skipped: u64,
    pub failed: u64,
    pub files: Vec<String>,
    pub failures: Vec<ImportFailure>,
    pub error: Option<String>,
}

static STATUS: OnceLock<Mutex<ImportJobStatus>> = OnceLock::new();

fn status_lock() -> &'static Mutex<ImportJobStatus> {
    STATUS.get_or_init(|| Mutex::new(ImportJobStatus::default()))
}

pub fn status() -> ImportJobStatus {
    status_lock().lock().expect("import job lock").clone()
}

fn update(f: impl FnOnce(&mut ImportJobStatus)) {
    let mut status = status_lock().lock().expect("import job lock");
    f(&mut status);
}

pub fn start(state: AppState) -> bool {
    {
        let mut status = status_lock().lock().expect("import job lock");
        if status.running {
            return false;
        }
        *status = ImportJobStatus {
            running: true,
            started_at: Some(now_epoch()),
            ..Default::default()
        };
    }

    tokio::spawn(async move {
        let result = run(&state).await;
        update(|status| {
            status.running = false;
            status.finished_at = Some(now_epoch());
            status.error = result.err().map(|error| error.to_string());
        });
    });

    true
}

/// Imports completed downloads that Bokhylle did not request itself (manual
/// grabs in the download client's book category). Single-book torrents bring
/// in their one book; collection torrents bring in every book that is not
/// already in the library. Files are placed with the same atomic,
/// digest-checked placement as normal imports; the library scan that runs
/// afterwards creates the book records, which is also why no
/// `pending_imports` journal is needed here: a crash leaves a file the scan
/// will pick up, not a half-recorded book.
pub async fn run(state: &AppState) -> Result<(), AppError> {
    let Some(factory) = state.providers.as_ref() else {
        return Err(AppError::Unprocessable(
            "the download client is not configured".to_string(),
        ));
    };
    let Some(downloader) = factory.downloader(state).await? else {
        return Err(AppError::Unprocessable(
            "the download client is not configured".to_string(),
        ));
    };

    let category = {
        let value = state
            .settings
            .get_string(settings::QBITTORRENT_CATEGORY, DEFAULT_CATEGORY)
            .await?;
        let value = value.trim();
        if value.is_empty() {
            DEFAULT_CATEGORY.to_string()
        } else {
            value.to_string()
        }
    };

    let torrents = downloader
        .list_category(&category)
        .await
        .map_err(|error| AppError::Unprocessable(format!("could not list downloads: {error}")))?;

    let staging = state
        .paths
        .config_dir
        .join("staging")
        .join("import-downloads");
    let mut placed_digests: Vec<String> = Vec::new();

    for torrent in torrents {
        match adopt_torrent(state, &torrent, &staging, &placed_digests).await {
            Ok(Outcome::Processed { imported, already }) => {
                for book in &imported {
                    placed_digests.push(book.digest.clone());
                }
                update(|status| {
                    status.imported += imported.len() as u64;
                    status.already += already;
                    for book in &imported {
                        if status.files.len() < MAX_REPORTED {
                            status.files.push(book.title.clone());
                        }
                    }
                });
            }
            Ok(Outcome::Skipped(reason)) => {
                tracing::debug!(torrent = %torrent.name, reason, "import.skipped");
                update(|status| status.skipped += 1);
            }
            Err(error) => update(|status| {
                status.failed += 1;
                if status.failures.len() < MAX_REPORTED {
                    status.failures.push(ImportFailure {
                        name: torrent.name.clone(),
                        reason: error.to_string(),
                    });
                }
            }),
        }
    }

    crate::library::scan_state::start_scan(state);
    Ok(())
}

enum Outcome {
    Processed {
        imported: Vec<Adopted>,
        already: u64,
    },
    Skipped(String),
}

struct Adopted {
    title: String,
    digest: String,
}

async fn adopt_torrent(
    state: &AppState,
    torrent: &TorrentInfo,
    staging_root: &Path,
    placed_digests: &[String],
) -> Result<Outcome, AppError> {
    if torrent.progress < 1.0 {
        return Ok(Outcome::Skipped("still downloading".to_string()));
    }
    if torrent
        .tag_list()
        .iter()
        .any(|tag| tag.starts_with(TAG_PREFIX))
    {
        return Ok(Outcome::Skipped("added by Bokhylle".to_string()));
    }

    let tracked: i64 =
        sqlx::query_scalar("SELECT count(*) FROM acquisitions WHERE provider_download_id = ?")
            .bind(&torrent.hash)
            .fetch_one(&state.db)
            .await?;
    if tracked > 0 {
        return Ok(Outcome::Skipped("already tracked".to_string()));
    }

    let content = torrent
        .content_path
        .clone()
        .filter(|path| !path.trim().is_empty())
        .unwrap_or_else(|| format!("{}/{}", torrent.save_path, torrent.name));
    let content = Path::new(&content);
    if !content.exists() {
        return Err(AppError::Unprocessable(
            "the download path is not visible to Bokhylle; check that the downloads mount matches the download client's path".to_string(),
        ));
    }
    if !crate::paths::is_within(&state.paths.downloads_dir, content) {
        return Err(AppError::Unprocessable(
            "the download path is outside the downloads directory".to_string(),
        ));
    }

    let staging = staging_root.join(&torrent.hash);
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    std::fs::create_dir_all(&staging)?;

    let expected = ExpectedImport {
        title: torrent.name.clone(),
        authors: Vec::new(),
        isbn: None,
        language: None,
    };
    let limits = Limits {
        max_files: MAX_ADOPTED_FILES,
        ..Limits::default()
    };
    let inspection =
        bokhylle_importer::inspect(content, &staging, &expected, &limits).map_err(|error| {
            AppError::Unprocessable(format!("could not inspect the download: {error}"))
        })?;

    let candidates = match inspection {
        Inspection::Selected(candidate) => vec![candidate],
        Inspection::NeedsReview { candidates, .. } => candidates,
        Inspection::Empty { reason } => return Ok(Outcome::Skipped(reason)),
    };
    if candidates.is_empty() {
        return Ok(Outcome::Skipped("no supported ebook files".to_string()));
    }

    let mut imported: Vec<Adopted> = Vec::new();
    let mut already: u64 = 0;

    for candidate in best_per_book(candidates) {
        let digest = crate::library::hash_file(&candidate.path)?;
        let known: i64 = sqlx::query_scalar("SELECT count(*) FROM book_files WHERE sha256 = ?")
            .bind(&digest)
            .fetch_one(&state.db)
            .await?;
        if known > 0 || placed_digests.contains(&digest) {
            already += 1;
            continue;
        }
        if already_catalogued(state, &candidate).await? {
            already += 1;
            continue;
        }

        let title = candidate_title(&candidate, torrent);
        let format = BookFormat::from_extension(&candidate.format).unwrap_or(BookFormat::Epub);
        let author = candidate.authors.first().cloned();
        let target =
            crate::import_pipeline::canonical_path_from(state, &title, author.as_deref(), format)?;

        crate::import_pipeline::place_file(state, &candidate.path, &target, &digest).await?;
        if let Err(error) = crate::library::index_placed_file(state, &target).await {
            // The scan at the end of the job is the safety net.
            tracing::warn!(
                path = %target.display(),
                %error,
                "import.adopted.index_failed"
            );
        }
        tracing::info!(
            torrent = %torrent.name,
            target = %target.display(),
            "import.adopted"
        );

        imported.push(Adopted { title, digest });
    }

    Ok(Outcome::Processed { imported, already })
}

/// One file per book: when the same title appears as several supported
/// formats, keep the EPUB if there is one, otherwise the best-scoring file.
fn best_per_book(candidates: Vec<ImportCandidate>) -> Vec<ImportCandidate> {
    let mut groups: Vec<(String, ImportCandidate)> = Vec::new();

    for candidate in candidates {
        let title = candidate
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_default();
        let author = candidate.authors.first().cloned().unwrap_or_default();
        let key = format!(
            "{}|{}",
            normalize_text(&core_title(&title)),
            normalize_text(&author)
        );

        match groups.iter_mut().find(|(existing, _)| *existing == key) {
            Some((_, existing)) if prefer(&candidate, existing) => *existing = candidate,
            Some(_) => {}
            None => groups.push((key, candidate)),
        }
    }

    groups.sort_by(|left, right| left.0.cmp(&right.0));
    groups.into_iter().map(|(_, candidate)| candidate).collect()
}

fn prefer(candidate: &ImportCandidate, existing: &ImportCandidate) -> bool {
    if candidate.format == existing.format {
        candidate.score > existing.score
    } else {
        candidate.format == "epub"
    }
}

fn candidate_title(candidate: &ImportCandidate, torrent: &TorrentInfo) -> String {
    candidate
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| torrent.name.clone())
}

/// True when the book already exists in the library under another file:
/// matching ISBN, or matching normalized title and primary author.
async fn already_catalogued(
    state: &AppState,
    candidate: &ImportCandidate,
) -> Result<bool, AppError> {
    if let Some(isbn) = candidate
        .isbn
        .as_deref()
        .map(|isbn| isbn.replace(['-', ' '], ""))
        .filter(|isbn| !isbn.is_empty())
    {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM editions e
             JOIN book_files f ON f.edition_id = e.id
             WHERE e.isbn13 = ? OR e.isbn10 = ?",
        )
        .bind(&isbn)
        .bind(&isbn)
        .fetch_one(&state.db)
        .await?;
        if count > 0 {
            return Ok(true);
        }
    }

    let Some(title) = candidate
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
    else {
        return Ok(false);
    };
    let Some(author) = candidate.authors.first() else {
        return Ok(false);
    };
    let author = normalize_text(author);
    if author.is_empty() {
        return Ok(false);
    }

    for variant in [normalize_text(&core_title(title)), normalize_text(title)] {
        if variant.is_empty() {
            continue;
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM books b
             WHERE b.normalized_title = ?
               AND EXISTS (
                   SELECT 1 FROM book_files f
                   JOIN editions e ON e.id = f.edition_id
                   WHERE e.book_id = b.id
               )
               AND EXISTS (
                   SELECT 1 FROM book_authors ba
                   JOIN authors a ON a.id = ba.author_id
                   WHERE ba.book_id = b.id AND a.normalized_name = ?
               )",
        )
        .bind(&variant)
        .bind(&author)
        .fetch_one(&state.db)
        .await?;
        if count > 0 {
            return Ok(true);
        }
    }

    Ok(false)
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
