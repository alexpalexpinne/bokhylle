use std::sync::Arc;

use async_trait::async_trait;
use bokhylle_acquisition::newznab::NewznabClient;
use bokhylle_acquisition::provider::{DownloadProvider, IndexerProvider};
use bokhylle_acquisition::prowlarr::ProwlarrClient;
use bokhylle_acquisition::qbittorrent::{QbittorrentAuth, QbittorrentClient};
use bokhylle_acquisition::sabnzbd::{NzbDownloadProvider, SabnzbdClient};
use bokhylle_acquisition::torznab::TorznabClient;

use crate::AppState;
use crate::error::AppError;
use crate::settings;

#[async_trait]
pub trait ProviderFactory: Send + Sync {
    async fn indexer(&self, state: &AppState)
    -> Result<Option<Arc<dyn IndexerProvider>>, AppError>;

    async fn indexer_named(
        &self,
        state: &AppState,
        kind: &str,
    ) -> Result<Option<Arc<dyn IndexerProvider>>, AppError> {
        if kind == "prowlarr" {
            self.indexer(state).await
        } else {
            Ok(None)
        }
    }

    async fn downloader(
        &self,
        state: &AppState,
    ) -> Result<Option<Arc<dyn DownloadProvider>>, AppError>;

    async fn nzb_downloader(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn NzbDownloadProvider>>, AppError> {
        Ok(None)
    }
}

pub struct SettingsProviderFactory;

#[async_trait]
impl ProviderFactory for SettingsProviderFactory {
    async fn indexer(
        &self,
        state: &AppState,
    ) -> Result<Option<Arc<dyn IndexerProvider>>, AppError> {
        let selected = state
            .settings
            .get_string(settings::INDEXER_PROVIDER, "auto")
            .await?;
        let selected = if selected.is_empty() || selected == "auto" {
            let prowlarr_url = state
                .settings
                .get_string(settings::PROWLARR_URL, "")
                .await?;
            let prowlarr_key = state
                .settings
                .get_string(settings::PROWLARR_API_KEY, "")
                .await?;
            if !prowlarr_url.trim().is_empty() && !prowlarr_key.trim().is_empty() {
                "prowlarr"
            } else {
                let torznab_url = state.settings.get_string(settings::TORZNAB_URL, "").await?;
                if !torznab_url.trim().is_empty() {
                    "torznab"
                } else {
                    "newznab"
                }
            }
        } else {
            selected.as_str()
        };
        self.indexer_named(state, selected).await
    }

    async fn indexer_named(
        &self,
        state: &AppState,
        kind: &str,
    ) -> Result<Option<Arc<dyn IndexerProvider>>, AppError> {
        if kind == "newznab" {
            let url = state.settings.get_string(settings::NEWZNAB_URL, "").await?;
            let key = state
                .settings
                .get_string(settings::NEWZNAB_API_KEY, "")
                .await?;
            if url.trim().is_empty() || key.trim().is_empty() {
                return Ok(None);
            }
            let categories = state
                .settings
                .get_string(settings::NEWZNAB_CATEGORIES, "7000")
                .await?;
            let categories = categories
                .split(',')
                .filter(|value| !value.trim().is_empty())
                .map(|value| {
                    value
                        .trim()
                        .parse::<i32>()
                        .map_err(|_| AppError::Unprocessable("invalid Newznab categories".into()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let client = NewznabClient::new(&url, &key, categories)
                .map_err(|error| AppError::Unprocessable(error.to_string()))?;
            return Ok(Some(Arc::new(client)));
        }
        if kind == "torznab" {
            let url = state.settings.get_string(settings::TORZNAB_URL, "").await?;
            if url.trim().is_empty() {
                return Ok(None);
            }
            let api_key = state
                .settings
                .get_string(settings::TORZNAB_API_KEY, "")
                .await?;
            let categories = state
                .settings
                .get_string(settings::TORZNAB_CATEGORIES, "7000")
                .await?;
            let categories = categories
                .split(',')
                .filter(|value| !value.trim().is_empty())
                .map(|value| {
                    value.trim().parse::<i32>().map_err(|_| {
                        AppError::Unprocessable("invalid Torznab categories".to_string())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let client = TorznabClient::new(&url, &api_key, categories).map_err(|error| {
                AppError::Unprocessable(format!("invalid Torznab configuration: {error}"))
            })?;
            return Ok(Some(Arc::new(client)));
        }
        if kind != "prowlarr" {
            return Err(AppError::Unprocessable(
                "invalid indexer provider".to_string(),
            ));
        }
        let url = state
            .settings
            .get_string(settings::PROWLARR_URL, "")
            .await?;
        let api_key = state
            .settings
            .get_string(settings::PROWLARR_API_KEY, "")
            .await?;

        if url.trim().is_empty() || api_key.trim().is_empty() {
            return Ok(None);
        }

        let client = ProwlarrClient::new(&url, &api_key).map_err(|error| {
            AppError::Unprocessable(format!("invalid Prowlarr configuration: {error}"))
        })?;

        Ok(Some(Arc::new(client)))
    }

    async fn downloader(
        &self,
        state: &AppState,
    ) -> Result<Option<Arc<dyn DownloadProvider>>, AppError> {
        let url = state
            .settings
            .get_string(settings::QBITTORRENT_URL, "")
            .await?;
        if url.trim().is_empty() {
            return Ok(None);
        }

        let api_key = state
            .settings
            .get_string(settings::QBITTORRENT_API_KEY, "")
            .await?;
        let username = state
            .settings
            .get_string(settings::QBITTORRENT_USERNAME, "")
            .await?;
        let password = state
            .settings
            .get_string(settings::QBITTORRENT_PASSWORD, "")
            .await?;

        let auth = if !api_key.trim().is_empty() {
            QbittorrentAuth::ApiKey(api_key)
        } else if !username.trim().is_empty() {
            QbittorrentAuth::Credentials { username, password }
        } else {
            return Ok(None);
        };

        let client = QbittorrentClient::new(&url, auth).map_err(|error| {
            AppError::Unprocessable(format!("invalid qBittorrent configuration: {error}"))
        })?;

        Ok(Some(Arc::new(client)))
    }

    async fn nzb_downloader(
        &self,
        state: &AppState,
    ) -> Result<Option<Arc<dyn NzbDownloadProvider>>, AppError> {
        let url = state.settings.get_string(settings::SABNZBD_URL, "").await?;
        let key = state
            .settings
            .get_string(settings::SABNZBD_API_KEY, "")
            .await?;
        if url.trim().is_empty() || key.trim().is_empty() {
            return Ok(None);
        }
        let client = SabnzbdClient::new(&url, &key)
            .map_err(|error| AppError::Unprocessable(error.to_string()))?;
        Ok(Some(Arc::new(client)))
    }
}

pub struct StaticProviderFactory {
    pub indexer: Option<Arc<dyn IndexerProvider>>,
    pub downloader: Option<Arc<dyn DownloadProvider>>,
}

#[async_trait]
impl ProviderFactory for StaticProviderFactory {
    async fn indexer(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn IndexerProvider>>, AppError> {
        Ok(self.indexer.clone())
    }

    async fn downloader(
        &self,
        _state: &AppState,
    ) -> Result<Option<Arc<dyn DownloadProvider>>, AppError> {
        Ok(self.downloader.clone())
    }
}
