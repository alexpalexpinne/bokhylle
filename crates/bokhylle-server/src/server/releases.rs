//! Optional, bounded release awareness. Never downloads images or changes the installation.
use crate::{AppState, error::AppError, settings};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::time::Duration;
use tokio::sync::Mutex;

const ENDPOINT: &str = "https://api.github.com/repos/alexpalexpinne/bokhylle/releases/latest";
const MAX_BYTES: usize = 128 * 1024;
const SUCCESS_INTERVAL: i64 = 24 * 3600;
const FAILURE_INTERVAL: i64 = 3600;
const MANUAL_INTERVAL: i64 = 60;

pub struct ReleaseChecker {
    client: reqwest::Client,
    endpoint: String,
    gate: Mutex<()>,
}

impl Default for ReleaseChecker {
    fn default() -> Self {
        Self::with_endpoint(ENDPOINT.to_owned())
    }
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    NotChecked,
    UpdateAvailable,
    UpToDate,
    NewerBuild,
    Unavailable,
    NoRelease,
}

#[derive(Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub state: UpdateState,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub checked_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub error: Option<String>,
    pub automatic_checks: bool,
}

#[derive(FromRow)]
struct CachedRelease {
    checked_at: Option<i64>,
    last_success_at: Option<i64>,
    latest_version: Option<String>,
    release_url: Option<String>,
    error: Option<String>,
}

impl CachedRelease {
    fn interval(&self) -> i64 {
        if self
            .error
            .as_deref()
            .is_some_and(|error| error != "no_release")
        {
            FAILURE_INTERVAL
        } else {
            SUCCESS_INTERVAL
        }
    }
    fn status(self, automatic_checks: bool) -> UpdateStatus {
        let state = match self.error.as_deref() {
            Some("no_release") => UpdateState::NoRelease,
            Some(_) => UpdateState::Unavailable,
            None if self.checked_at.is_none() => UpdateState::NotChecked,
            None => match self
                .latest_version
                .as_deref()
                .and_then(|value| semver::Version::parse(value).ok())
            {
                Some(latest) => match latest.cmp_precedence(
                    &semver::Version::parse(bokhylle_core::VERSION).expect("package version"),
                ) {
                    std::cmp::Ordering::Greater => UpdateState::UpdateAvailable,
                    std::cmp::Ordering::Equal => UpdateState::UpToDate,
                    std::cmp::Ordering::Less => UpdateState::NewerBuild,
                },
                None => UpdateState::Unavailable,
            },
        };
        let error = self
            .error
            .as_deref()
            .filter(|error| *error != "no_release")
            .map(|code| {
                match code {
                    "rate_limited" => "GitHub is limiting update checks. Try again later.",
                    "invalid_response" => {
                        "GitHub returned an unrecognised release. The library remains available."
                    }
                    _ => "Could not check GitHub releases. The library remains available.",
                }
                .into()
            });
        UpdateStatus {
            current_version: bokhylle_core::VERSION.into(),
            state,
            latest_version: self.latest_version,
            release_url: self.release_url,
            checked_at: self.checked_at,
            last_success_at: self.last_success_at,
            error,
            automatic_checks,
        }
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

impl ReleaseChecker {
    /// The URL is constructor-injected for fake providers in tests, never accepted by an HTTP route.
    pub fn with_endpoint(endpoint: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .connect_timeout(Duration::from_secs(4))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("Bokhylle/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("release HTTP client");
        Self {
            client,
            endpoint,
            gate: Mutex::new(()),
        }
    }

    async fn cached(pool: &SqlitePool) -> Result<CachedRelease, AppError> {
        Ok(sqlx::query_as("SELECT checked_at, last_success_at, latest_version, release_url, error FROM server_release_state WHERE id = 1").fetch_one(pool).await?)
    }

    pub async fn status(&self, state: &AppState) -> Result<UpdateStatus, AppError> {
        Ok(Self::cached(&state.db).await?.status(
            state
                .settings
                .get_bool(settings::UPDATE_CHECKS, true)
                .await?,
        ))
    }

    pub async fn check_at(
        &self,
        state: &AppState,
        manual: bool,
        now: i64,
    ) -> Result<UpdateStatus, AppError> {
        let _guard = self.gate.lock().await;
        let cached = Self::cached(&state.db).await?;
        let interval = if manual {
            MANUAL_INTERVAL
        } else {
            cached.interval()
        };
        if cached
            .checked_at
            .is_some_and(|at| now.saturating_sub(at) < interval)
        {
            return self.status(state).await;
        }
        match self.fetch().await {
            Ok(Some((version, url))) => {
                sqlx::query("UPDATE server_release_state SET checked_at = ?, last_success_at = ?, latest_version = ?, release_url = ?, error = NULL WHERE id = 1")
                    .bind(now).bind(now).bind(version).bind(url).execute(&state.db).await?;
            }
            Ok(None) => {
                sqlx::query("UPDATE server_release_state SET checked_at = ?, last_success_at = ?, latest_version = NULL, release_url = NULL, error = 'no_release' WHERE id = 1")
                    .bind(now).bind(now).execute(&state.db).await?;
            }
            Err(code) => {
                sqlx::query(
                    "UPDATE server_release_state SET checked_at = ?, error = ? WHERE id = 1",
                )
                .bind(now)
                .bind(code)
                .execute(&state.db)
                .await?;
                tracing::warn!("release.check.failed");
            }
        }
        self.status(state).await
    }

    async fn fetch(&self) -> Result<Option<(String, String)>, &'static str> {
        let mut response = self
            .client
            .get(&self.endpoint)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
            .send()
            .await
            .map_err(|_| "unavailable")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if matches!(response.status().as_u16(), 403 | 429) {
            return Err("rate_limited");
        }
        if !response.status().is_success() {
            return Err("unavailable");
        }
        if response
            .content_length()
            .is_some_and(|bytes| bytes > MAX_BYTES as u64)
        {
            return Err("invalid_response");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "unavailable")? {
            if bytes.len() + chunk.len() > MAX_BYTES {
                return Err("invalid_response");
            }
            bytes.extend_from_slice(&chunk);
        }
        let release: GithubRelease =
            serde_json::from_slice(&bytes).map_err(|_| "invalid_response")?;
        let version = semver::Version::parse(
            release
                .tag_name
                .strip_prefix('v')
                .unwrap_or(&release.tag_name),
        )
        .map_err(|_| "invalid_response")?;
        if release.draft
            || release.prerelease
            || !version.pre.is_empty()
            || !version.build.is_empty()
        {
            return Err("invalid_response");
        }
        Ok(Some((
            version.to_string(),
            format!(
                "https://github.com/alexpalexpinne/bokhylle/releases/tag/{}",
                release.tag_name
            ),
        )))
    }
}

pub fn spawn_scheduler(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            let notified = state.server.release_wakeup.notified();
            let enabled = state
                .settings
                .get_bool(settings::UPDATE_CHECKS, true)
                .await
                .unwrap_or(false);
            if enabled
                && let Err(error) = state
                    .server
                    .releases
                    .check_at(&state, false, super::now())
                    .await
            {
                tracing::warn!(%error, "release.cache.failed");
            }
            let delay = if enabled {
                ReleaseChecker::cached(&state.db)
                    .await
                    .ok()
                    .map(|cached| {
                        cached
                            .checked_at
                            .unwrap_or(super::now())
                            .saturating_add(cached.interval())
                            .saturating_sub(super::now())
                            .max(60) as u64
                    })
                    .unwrap_or(FAILURE_INTERVAL as u64)
            } else {
                SUCCESS_INTERVAL as u64
            };
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(delay)) => {},
                _ = notified => {},
            }
        }
    });
}
