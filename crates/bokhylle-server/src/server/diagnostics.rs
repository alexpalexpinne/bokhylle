use super::{BuildIdentity, RestartChange};
use crate::{AppState, backup::BackupStatus, error::AppError, settings};
use serde::Serialize;

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub format_version: u32,
    pub generated_at: i64,
    pub build: BuildIdentity,
    pub started_at: i64,
    pub uptime_seconds: u64,
    pub platform: String,
    pub database_ok: bool,
    pub integrations: Vec<IntegrationConfiguration>,
    pub storage: Vec<StorageSummary>,
    pub backups: BackupStatus,
    pub restart_required: Vec<RestartChange>,
    pub library: LibraryIntegritySummary,
    pub recent_errors: Vec<crate::observability::DiagnosticEvent>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct IntegrationConfiguration {
    pub name: String,
    pub configured: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageSummary {
    pub locations: Vec<String>,
    pub unwritable_locations: Vec<String>,
    pub available_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub low_space: bool,
    pub capacity_available: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryIntegritySummary {
    pub books: i64,
    pub files: usize,
    pub missing_files: usize,
    pub inaccessible_files: usize,
    pub books_without_files: i64,
}

pub async fn collect(state: &AppState) -> Result<Diagnostics, AppError> {
    let status = super::status(state).await?;
    let mut integrations = Vec::new();
    for (name, key) in [
        ("Prowlarr", settings::PROWLARR_URL),
        ("Torznab", settings::TORZNAB_URL),
        ("Newznab", settings::NEWZNAB_URL),
        ("qBittorrent", settings::QBITTORRENT_URL),
        ("SABnzbd", settings::SABNZBD_URL),
        ("Email delivery", settings::SMTP_HOST),
    ] {
        integrations.push(IntegrationConfiguration {
            name: name.into(),
            configured: !state.settings.get_string(key, "").await?.trim().is_empty(),
        });
    }
    let paths: Vec<String> = sqlx::query_scalar("SELECT path FROM book_files")
        .fetch_all(&state.db)
        .await?;
    let files = paths.len();
    let (missing_files, inaccessible_files) = tokio::task::spawn_blocking(move || {
        let mut missing = 0;
        let mut inaccessible = 0;
        for path in paths {
            match std::fs::metadata(path) {
                Ok(metadata) if metadata.is_file() => {}
                Ok(_) => missing += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => missing += 1,
                Err(_) => inaccessible += 1,
            }
        }
        (missing, inaccessible)
    })
    .await
    .map_err(|_| AppError::Unavailable("Library checks could not finish".into()))?;
    let books = sqlx::query_scalar("SELECT count(*) FROM books")
        .fetch_one(&state.db)
        .await?;
    let books_without_files = sqlx::query_scalar("SELECT count(*) FROM books b WHERE NOT EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id WHERE e.book_id = b.id)").fetch_one(&state.db).await?;
    Ok(Diagnostics {
        format_version: 1,
        generated_at: super::now(),
        build: status.build,
        started_at: status.started_at,
        uptime_seconds: status.uptime_seconds,
        platform: format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
        database_ok: status.database_ok,
        integrations,
        storage: status
            .storage
            .into_iter()
            .map(|group| StorageSummary {
                locations: group
                    .locations
                    .iter()
                    .map(|location| location.label.clone())
                    .collect(),
                unwritable_locations: group
                    .locations
                    .iter()
                    .filter(|location| !location.writable)
                    .map(|location| location.label.clone())
                    .collect(),
                available_bytes: group.available_bytes,
                total_bytes: group.total_bytes,
                low_space: group.low_space,
                capacity_available: group.error.is_none(),
            })
            .collect(),
        backups: crate::backup::status_at(state, super::now()).await?,
        restart_required: status.restart_required,
        library: LibraryIntegritySummary {
            books,
            files,
            missing_files,
            inaccessible_files,
            books_without_files,
        },
        recent_errors: crate::observability::recent_errors(),
    })
}
