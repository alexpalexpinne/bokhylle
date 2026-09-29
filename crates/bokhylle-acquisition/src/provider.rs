use std::sync::Arc;

use async_trait::async_trait;

use crate::model::{ExpectedBook, ReleaseCandidate};
use crate::newznab::NewznabError;
use crate::prowlarr::ProwlarrError;
use crate::qbittorrent::{QbittorrentError, TorrentInfo};
use crate::torznab::TorznabError;

#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    #[error(transparent)]
    Prowlarr(#[from] ProwlarrError),
    #[error(transparent)]
    Torznab(#[from] TorznabError),
    #[error(transparent)]
    Newznab(#[from] NewznabError),
    #[error("the indexer is not configured")]
    NotConfigured,
    #[error("no download link is available for this release")]
    NoDownloadLink,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Qbittorrent(#[from] QbittorrentError),
    #[error("the download client is not configured")]
    NotConfigured,
}

impl From<reqwest::Error> for IndexerError {
    fn from(error: reqwest::Error) -> Self {
        IndexerError::Prowlarr(ProwlarrError::Request(error))
    }
}

impl From<reqwest::Error> for DownloadError {
    fn from(error: reqwest::Error) -> Self {
        DownloadError::Qbittorrent(QbittorrentError::Request(error))
    }
}

pub enum DownloadSource {
    Magnet(String),
    TorrentFile {
        bytes: Arc<Vec<u8>>,
        filename: String,
    },
}

#[derive(Debug)]
pub struct SearchOutcome {
    pub queries: Vec<String>,
    pub candidates: Vec<ReleaseCandidate>,
}

#[async_trait]
pub trait IndexerProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn search_book(&self, book: &ExpectedBook) -> Result<SearchOutcome, IndexerError>;

    async fn fetch_torrent(&self, release: &ReleaseCandidate)
    -> Result<Arc<Vec<u8>>, IndexerError>;

    async fn test_connection(&self) -> Result<String, IndexerError>;

    fn nzb_url(&self, _release: &ReleaseCandidate) -> Result<String, IndexerError> {
        Err(IndexerError::NoDownloadLink)
    }
}

#[async_trait]
pub trait DownloadProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn add(
        &self,
        source: DownloadSource,
        category: &str,
        tag: &str,
    ) -> Result<(), DownloadError>;

    async fn status(&self, id: &str) -> Result<Option<TorrentInfo>, DownloadError>;

    async fn find_by_tag(&self, tag: &str) -> Result<Vec<TorrentInfo>, DownloadError>;

    /// Every torrent in a category, used by the import-existing-downloads job.
    async fn list_category(&self, category: &str) -> Result<Vec<TorrentInfo>, DownloadError>;

    async fn cancel_owned(
        &self,
        id: &str,
        category: &str,
        delete_files: bool,
    ) -> Result<bool, DownloadError>;

    async fn test_connection(&self) -> Result<String, DownloadError>;
}
