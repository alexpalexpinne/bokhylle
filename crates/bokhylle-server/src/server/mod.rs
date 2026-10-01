//! Administration uses the running configuration, never guesses from saved edits.
pub mod diagnostics;
pub mod releases;
pub mod storage;

use crate::{
    AppState,
    error::AppError,
    paths::Paths,
    settings::{self, Settings},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Instant, SystemTime},
};
use tokio::sync::{Mutex, Notify};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|time| time.as_secs() as i64)
        .unwrap_or_default()
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildIdentity {
    pub version: String,
    pub commit: Option<String>,
    pub dirty: Option<bool>,
    pub built_at: Option<i64>,
    pub installation: String,
}

pub fn build_identity() -> BuildIdentity {
    BuildIdentity {
        version: bokhylle_core::VERSION.into(),
        commit: option_env!("BOKHYLLE_BUILD_SHA")
            .filter(|value| {
                matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            .map(str::to_owned),
        dirty: option_env!("BOKHYLLE_BUILD_DIRTY").and_then(|value| value.parse().ok()),
        built_at: option_env!("BOKHYLLE_BUILD_TIME").and_then(|value| value.parse().ok()),
        installation: option_env!("BOKHYLLE_BUILD_INSTALLATION")
            .unwrap_or("source")
            .into(),
    }
}

pub struct ServerRuntime {
    pub started_at: i64,
    started: Instant,
    startup: Vec<StartupValue>,
    pub backup_gate: Mutex<()>,
    pub backup_wakeup: Notify,
    pub release_wakeup: Notify,
    pub releases: Arc<releases::ReleaseChecker>,
}

struct StartupValue {
    key: &'static str,
    label: &'static str,
    default: String,
    fingerprint: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct RestartChange {
    pub key: String,
    pub label: String,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub build: BuildIdentity,
    pub started_at: i64,
    pub uptime_seconds: u64,
    pub restart_required: Vec<RestartChange>,
    pub storage: Vec<storage::StorageGroup>,
    pub database_ok: bool,
}

impl ServerRuntime {
    pub async fn capture(settings: &Settings, paths: &Paths) -> Result<Self, AppError> {
        let definitions = [
            (
                settings::LIBRARY_ROOT,
                "Library path",
                paths.library_root.to_string_lossy().into_owned(),
            ),
            (
                settings::DOWNLOADS_DIR,
                "Downloads path",
                paths.downloads_dir.to_string_lossy().into_owned(),
            ),
            (
                settings::METADATA_PROVIDER,
                "Metadata provider",
                "automatic".into(),
            ),
            (
                settings::RATINGS_PROVIDER,
                "Ratings provider",
                "same_as_metadata".into(),
            ),
            (
                settings::GOOGLE_BOOKS_API_KEY,
                "Google Books credential",
                "".into(),
            ),
        ];
        let mut startup = Vec::new();
        for (key, label, default) in definitions {
            let value = settings.get_string(key, &default).await?;
            startup.push(StartupValue {
                key,
                label,
                default,
                fingerprint: fingerprint(&value),
            });
        }
        Ok(Self {
            started_at: now(),
            started: Instant::now(),
            startup,
            backup_gate: Mutex::new(()),
            backup_wakeup: Notify::new(),
            release_wakeup: Notify::new(),
            releases: Arc::new(releases::ReleaseChecker::default()),
        })
    }

    pub fn uptime(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub async fn pending_restart(
        &self,
        settings: &Settings,
    ) -> Result<Vec<RestartChange>, AppError> {
        let mut changes = Vec::new();
        for item in &self.startup {
            let current = settings.get_string(item.key, &item.default).await?;
            if fingerprint(&current) != item.fingerprint {
                changes.push(RestartChange {
                    key: item.key.into(),
                    label: item.label.into(),
                });
            }
        }
        Ok(changes)
    }
}

fn fingerprint(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub async fn status(state: &AppState) -> Result<ServerStatus, AppError> {
    let paths = state.paths.clone();
    let storage = tokio::task::spawn_blocking(move || storage::inspect(&paths))
        .await
        .map_err(|_| AppError::Unavailable("Storage checks could not finish".into()))?;
    Ok(ServerStatus {
        build: build_identity(),
        started_at: state.server.started_at,
        uptime_seconds: state.server.uptime(),
        restart_required: state.server.pending_restart(&state.settings).await?,
        storage,
        database_ok: sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&state.db)
            .await
            .is_ok(),
    })
}
