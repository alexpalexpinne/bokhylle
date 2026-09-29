//! Profile-scoped access and browser positions for a specific library file.

use std::path::PathBuf;

use bokhylle_core::BookFormat;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::User;
use crate::error::AppError;

pub struct ReadableFile {
    pub path: PathBuf,
    pub sha256: String,
    pub format: BookFormat,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReadingDirection {
    Ltr,
    Rtl,
}

impl ReadingDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetReadingDirection {
    pub direction: Option<ReadingDirection>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadingDirectionState {
    pub book_direction: Option<String>,
    pub series_direction: Option<String>,
    pub direction_override: Option<String>,
}

async fn direction_state(
    state: &AppState,
    user_id: i64,
    book_id: i64,
) -> Result<ReadingDirectionState, AppError> {
    let (book_direction, series_direction): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT b.reading_direction, s.default_reading_direction
             FROM books b LEFT JOIN series s ON s.id = b.series_id WHERE b.id = ?",
    )
    .bind(book_id)
    .fetch_one(&state.db)
    .await?;
    let direction_override: Option<String> = sqlx::query_scalar(
        "SELECT direction FROM reader_direction_overrides WHERE user_id = ? AND book_id = ?",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(ReadingDirectionState {
        book_direction,
        series_direction,
        direction_override,
    })
}

pub async fn set_direction(
    state: &AppState,
    user: &User,
    book_id: i64,
    file_id: i64,
    update: SetReadingDirection,
) -> Result<ReadingDirectionState, AppError> {
    readable_file(state, user, book_id, file_id).await?;
    if let Some(direction) = update.direction {
        sqlx::query(
            "INSERT INTO reader_direction_overrides (user_id, book_id, direction)
             VALUES (?, ?, ?)
             ON CONFLICT(user_id, book_id) DO UPDATE SET
                 direction = excluded.direction, updated_at = unixepoch()",
        )
        .bind(user.id)
        .bind(book_id)
        .bind(direction.as_str())
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query("DELETE FROM reader_direction_overrides WHERE user_id = ? AND book_id = ?")
            .bind(user.id)
            .bind(book_id)
            .execute(&state.db)
            .await?;
    }
    direction_state(state, user.id, book_id).await
}

/// Recheck the current shelf and file on every content or position request.
/// Adults may read household files; a child may read only an assigned book.
pub async fn readable_file(
    state: &AppState,
    user: &User,
    book_id: i64,
    file_id: i64,
) -> Result<ReadableFile, AppError> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT f.path, f.sha256, f.format
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ? AND f.id = ?",
    )
    .bind(book_id)
    .bind(file_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((path, sha256, format)) = row else {
        return Err(AppError::NotFound("readable file not found".into()));
    };
    let format = BookFormat::from_db(&format)
        .ok_or_else(|| AppError::NotFound("readable file not found".into()))?;
    if crate::auth::profile_type(&state.db, user.id).await? == "child"
        && !crate::user_books::contains(&state.db, user.id, book_id).await?
    {
        return Err(AppError::NotFound("readable file not found".into()));
    }

    // A deleted or replaced file must fail before a position can be returned
    // or saved. Canonical paths also prevent a library symlink from escaping.
    let path = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| AppError::NotFound("readable file is missing on disk".into()))?;
    let root = tokio::fs::canonicalize(&state.paths.library_root).await?;
    if !path.starts_with(root)
        || !tokio::fs::metadata(&path)
            .await
            .is_ok_and(|metadata| metadata.is_file())
    {
        return Err(AppError::NotFound(
            "readable file is missing on disk".into(),
        ));
    }
    Ok(ReadableFile {
        path,
        sha256,
        format,
    })
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPosition {
    pub locator: String,
    pub percentage: f64,
    pub completed: bool,
    pub revision: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPositionState {
    pub sha256: String,
    pub format: &'static str,
    pub position: Option<BrowserPosition>,
    pub external: Option<ExternalPosition>,
    pub direction: ReadingDirectionState,
    pub book_completed: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookCompletionState {
    pub completed: bool,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SetBookCompletion {
    pub completed: bool,
}

async fn readable_book(state: &AppState, user: &User, book_id: i64) -> Result<(), AppError> {
    let exists: Option<i64> = sqlx::query_scalar(
        "SELECT b.id FROM books b WHERE b.id = ?
         AND EXISTS (SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id
                     WHERE e.book_id = b.id)",
    )
    .bind(book_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none()
        || (crate::auth::profile_type(&state.db, user.id).await? == "child"
            && !crate::user_books::contains(&state.db, user.id, book_id).await?)
    {
        return Err(AppError::NotFound("book not found".into()));
    }
    Ok(())
}

async fn completed_at(
    state: &AppState,
    user_id: i64,
    book_id: i64,
) -> Result<Option<i64>, AppError> {
    Ok(sqlx::query_scalar(
        "SELECT completed_at FROM user_book_completions WHERE user_id = ? AND book_id = ?",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_optional(&state.db)
    .await?)
}

pub async fn book_completion(
    state: &AppState,
    user: &User,
    book_id: i64,
) -> Result<BookCompletionState, AppError> {
    readable_book(state, user, book_id).await?;
    let completed_at = completed_at(state, user.id, book_id).await?;
    Ok(BookCompletionState {
        completed: completed_at.is_some(),
        completed_at,
    })
}

pub async fn set_book_completion(
    state: &AppState,
    user: &User,
    book_id: i64,
    update: SetBookCompletion,
) -> Result<BookCompletionState, AppError> {
    readable_book(state, user, book_id).await?;
    let mut tx = state.db.begin().await?;
    if update.completed {
        sqlx::query(
            "INSERT INTO user_book_completions (user_id, book_id) VALUES (?, ?)
             ON CONFLICT(user_id, book_id) DO UPDATE SET completed_at = unixepoch()",
        )
        .bind(user.id)
        .bind(book_id)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query("DELETE FROM user_book_completions WHERE user_id = ? AND book_id = ?")
            .bind(user.id)
            .bind(book_id)
            .execute(&mut *tx)
            .await?;
    }
    // Keep existing locators while making an explicit series-page choice agree
    // with the reader's per-file finished indicator. Revision bumps make an
    // already open reader notice a change made in another tab. Bump even
    // unchanged rows so an in-flight save cannot undo an explicit restart.
    sqlx::query(
        "UPDATE browser_reading_positions SET completed = ?,
                revision = revision + 1, updated_at = unixepoch()
         WHERE user_id = ? AND book_file_id IN
               (SELECT f.id FROM book_files f JOIN editions e ON e.id = f.edition_id
                WHERE e.book_id = ?)",
    )
    .bind(update.completed)
    .bind(user.id)
    .bind(book_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    book_completion(state, user, book_id).await
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalPosition {
    pub locator: String,
    pub percentage: f64,
    pub revision: i64,
    pub updated_at: i64,
    pub source: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveBrowserPosition {
    pub sha256: String,
    pub locator: String,
    pub percentage: f64,
    pub completed: bool,
    pub expected_revision: i64,
    #[serde(default)]
    pub expected_external_revision: i64,
    pub external_locator: Option<String>,
}

async fn document_id(path: PathBuf) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || crate::partial_md5(&path))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))?
        .ok_or_else(|| AppError::NotFound("readable file is missing on disk".into()))
}

pub async fn position(
    state: &AppState,
    user: &User,
    book_id: i64,
    file_id: i64,
) -> Result<BrowserPositionState, AppError> {
    let file = readable_file(state, user, book_id, file_id).await?;
    let row: Option<(String, f64, bool, i64, i64)> = sqlx::query_as(
        "SELECT locator, percentage, completed, revision, updated_at
         FROM browser_reading_positions
         WHERE user_id = ? AND book_file_id = ? AND sha256 = ?",
    )
    .bind(user.id)
    .bind(file_id)
    .bind(&file.sha256)
    .fetch_optional(&state.db)
    .await?;
    let external = {
        let document = document_id(file.path).await?;
        let row: Option<(String, f64, i64, i64, String)> = sqlx::query_as(
            "SELECT locator, percentage, revision, updated_at, source
             FROM reading_progress WHERE user_id = ? AND document = ?",
        )
        .bind(user.id)
        .bind(document)
        .fetch_optional(&state.db)
        .await?;
        row.and_then(|(locator, percentage, revision, updated_at, source)| {
            let valid = match file.format {
                BookFormat::Epub => locator.starts_with("/body/DocFragment"),
                BookFormat::Pdf | BookFormat::Cbz => {
                    locator.parse::<u32>().is_ok_and(|page| page > 0)
                }
            };
            valid.then_some(ExternalPosition {
                locator,
                percentage,
                revision,
                updated_at,
                source,
            })
        })
    };
    Ok(BrowserPositionState {
        sha256: file.sha256,
        format: file.format.as_str(),
        position: row.map(|(locator, percentage, completed, revision, updated_at)| {
            BrowserPosition {
                locator,
                percentage,
                completed,
                revision,
                updated_at,
            }
        }),
        external,
        direction: direction_state(state, user.id, book_id).await?,
        book_completed: completed_at(state, user.id, book_id).await?.is_some(),
    })
}

pub async fn save_position(
    state: &AppState,
    user: &User,
    book_id: i64,
    file_id: i64,
    update: SaveBrowserPosition,
) -> Result<BrowserPositionState, AppError> {
    let file = readable_file(state, user, book_id, file_id).await?;
    if update.sha256 != file.sha256 {
        return Err(AppError::Conflict(
            "file has changed; reload the reader".into(),
        ));
    }
    let valid_locator = match file.format {
        BookFormat::Epub => true,
        BookFormat::Pdf | BookFormat::Cbz => {
            update.locator.parse::<u32>().is_ok_and(|page| page > 0)
                && !update.locator.starts_with('0')
        }
    };
    if update.locator.is_empty()
        || update.locator.len() > 4096
        || update.locator.chars().any(char::is_control)
        || !valid_locator
        || !update.percentage.is_finite()
        || !(0.0..=1.0).contains(&update.percentage)
        || update.expected_revision < 0
        || update.expected_external_revision < 0
    {
        return Err(AppError::BadRequest(
            "invalid browser reading position".into(),
        ));
    }

    let external_locator = match file.format {
        BookFormat::Epub => update.external_locator.as_deref(),
        BookFormat::Pdf | BookFormat::Cbz => Some(update.locator.as_str()),
    };
    if let Some(external_locator) = external_locator
        && (external_locator.len() > 4096
            || external_locator.chars().any(char::is_control)
            || (file.format == BookFormat::Epub
                && !external_locator.starts_with("/body/DocFragment")))
    {
        return Err(AppError::BadRequest(
            "invalid external reading position".into(),
        ));
    }
    let document = if external_locator.is_some() {
        Some(document_id(file.path).await?)
    } else {
        None
    };
    let mut transaction = state.db.begin().await?;
    if let Some(document) = &document {
        let external_revision: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM reading_progress WHERE user_id = ? AND document = ?",
        )
        .bind(user.id)
        .bind(document)
        .fetch_optional(&mut *transaction)
        .await?;
        if external_revision.unwrap_or(0) != update.expected_external_revision {
            return Err(AppError::Conflict(
                "KOReader position changed; reload the saved position".into(),
            ));
        }
    }
    let result = if update.expected_revision == 0 {
        // A new file identity can replace an older locator, but two tabs
        // starting the same identity cannot both claim revision zero.
        sqlx::query(
            "INSERT INTO browser_reading_positions
             (user_id, book_file_id, sha256, format, locator, percentage, completed, revision)
             VALUES (?, ?, ?, ?, ?, ?, ?, 1)
             ON CONFLICT(user_id, book_file_id) DO UPDATE SET
                 sha256 = excluded.sha256,
                 format = excluded.format,
                 locator = excluded.locator,
                 percentage = excluded.percentage,
                 completed = excluded.completed,
                 revision = browser_reading_positions.revision + 1,
                 updated_at = unixepoch()
             WHERE browser_reading_positions.sha256 != excluded.sha256",
        )
        .bind(user.id)
        .bind(file_id)
        .bind(&file.sha256)
        .bind(file.format.as_str())
        .bind(&update.locator)
        .bind(update.percentage)
        .bind(update.completed)
        .execute(&mut *transaction)
        .await?
    } else {
        sqlx::query(
            "UPDATE browser_reading_positions SET
                 locator = ?, percentage = ?, completed = ?,
                 revision = revision + 1, updated_at = unixepoch()
             WHERE user_id = ? AND book_file_id = ? AND sha256 = ? AND revision = ?",
        )
        .bind(&update.locator)
        .bind(update.percentage)
        .bind(update.completed)
        .bind(user.id)
        .bind(file_id)
        .bind(&file.sha256)
        .bind(update.expected_revision)
        .execute(&mut *transaction)
        .await?
    };
    if result.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "reading position changed in another tab; reload the saved position".into(),
        ));
    }
    if update.completed {
        sqlx::query(
            "INSERT INTO user_book_completions (user_id, book_id) VALUES (?, ?)
             ON CONFLICT(user_id, book_id) DO UPDATE SET completed_at = unixepoch()",
        )
        .bind(user.id)
        .bind(book_id)
        .execute(&mut *transaction)
        .await?;
    }
    if let Some(document) = document {
        sqlx::query(
            "INSERT INTO reading_progress
             (user_id, document, book_id, book_file_id, percentage, locator, source, source_device)
             VALUES (?, ?, ?, ?, ?, ?, 'bokhylle', 'Bokhylle browser')
             ON CONFLICT(user_id, document) DO UPDATE SET
                 book_id = excluded.book_id,
                 book_file_id = excluded.book_file_id,
                 percentage = excluded.percentage,
                 locator = excluded.locator,
                 source = excluded.source,
                 source_device = excluded.source_device,
                 revision = reading_progress.revision + 1,
                 updated_at = unixepoch()",
        )
        .bind(user.id)
        .bind(document)
        .bind(book_id)
        .bind(file_id)
        .bind(update.percentage)
        .bind(external_locator.expect("document requires an external locator"))
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    position(state, user, book_id, file_id).await
}
