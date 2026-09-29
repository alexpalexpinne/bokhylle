use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::model::{ExpectedBook, ReleaseCandidate};
use crate::provider::{
    DownloadError, DownloadProvider, DownloadSource, IndexerError, IndexerProvider, SearchOutcome,
};
use crate::qbittorrent::TorrentInfo;

enum FakeIndexerOutcome {
    Candidates(Vec<ReleaseCandidate>),
    Failing,
}

#[derive(Default)]
pub struct FakeIndexerProvider {
    outcome: Mutex<Option<FakeIndexerOutcome>>,
    torrent: Mutex<Option<Arc<Vec<u8>>>>,
}

impl FakeIndexerProvider {
    pub fn with_candidates(candidates: Vec<ReleaseCandidate>) -> Self {
        Self {
            outcome: Mutex::new(Some(FakeIndexerOutcome::Candidates(candidates))),
            torrent: Mutex::new(None),
        }
    }

    pub fn failing() -> Self {
        Self {
            outcome: Mutex::new(Some(FakeIndexerOutcome::Failing)),
            torrent: Mutex::new(None),
        }
    }

    pub fn set_candidates(&self, candidates: Vec<ReleaseCandidate>) {
        *self.outcome.lock().expect("fake indexer lock") =
            Some(FakeIndexerOutcome::Candidates(candidates));
    }

    pub fn set_failing(&self) {
        *self.outcome.lock().expect("fake indexer lock") = Some(FakeIndexerOutcome::Failing);
    }

    pub fn set_torrent(&self, bytes: Vec<u8>) {
        *self.torrent.lock().expect("fake indexer lock") = Some(Arc::new(bytes));
    }
}

#[async_trait]
impl IndexerProvider for FakeIndexerProvider {
    fn name(&self) -> &'static str {
        "fake-indexer"
    }

    async fn search_book(&self, _book: &ExpectedBook) -> Result<SearchOutcome, IndexerError> {
        let outcome = self.outcome.lock().expect("fake indexer lock");

        match outcome.as_ref() {
            None => Ok(SearchOutcome {
                queries: vec!["fake (0 candidates)".to_string()],
                candidates: Vec::new(),
            }),
            Some(FakeIndexerOutcome::Candidates(candidates)) => Ok(SearchOutcome {
                queries: vec![format!("fake ({} candidates)", candidates.len())],
                candidates: candidates.clone(),
            }),
            Some(FakeIndexerOutcome::Failing) => Err(IndexerError::NotConfigured),
        }
    }

    async fn fetch_torrent(
        &self,
        _release: &ReleaseCandidate,
    ) -> Result<Arc<Vec<u8>>, IndexerError> {
        self.torrent
            .lock()
            .expect("fake indexer lock")
            .clone()
            .ok_or(IndexerError::NoDownloadLink)
    }

    async fn test_connection(&self) -> Result<String, IndexerError> {
        Ok("fake-indexer 1.0".to_string())
    }
}

#[derive(Debug, Clone)]
pub struct AddedRelease {
    pub category: String,
    pub tag: String,
    pub magnet: bool,
}

#[derive(Default)]
pub struct FakeDownloadProvider {
    added: Mutex<Vec<AddedRelease>>,
    failing: Mutex<bool>,
    category_failing: Mutex<bool>,
    missing: Mutex<bool>,
    hash: Mutex<Option<String>>,
    progress: Mutex<f64>,
    state: Mutex<String>,
    content_path: Mutex<Option<String>>,
    canceled: Mutex<Vec<String>>,
    category_torrents: Mutex<Vec<TorrentInfo>>,
}

impl FakeDownloadProvider {
    pub fn added(&self) -> Vec<AddedRelease> {
        self.added.lock().expect("fake downloader lock").clone()
    }

    pub fn canceled(&self) -> Vec<String> {
        self.canceled.lock().expect("fake downloader lock").clone()
    }

    pub fn set_failing(&self, failing: bool) {
        *self.failing.lock().expect("fake downloader lock") = failing;
    }

    pub fn set_missing(&self, missing: bool) {
        *self.missing.lock().expect("fake downloader lock") = missing;
    }

    /// Fails `list_category` only, leaving the connection healthy, so tests
    /// can tell a path problem apart from an unreachable client.
    pub fn set_category_failing(&self, failing: bool) {
        *self.category_failing.lock().expect("fake downloader lock") = failing;
    }

    pub fn set_hash(&self, hash: &str) {
        *self.hash.lock().expect("fake downloader lock") = Some(hash.to_string());
    }

    pub fn set_progress(&self, progress: f64) {
        *self.progress.lock().expect("fake downloader lock") = progress;
    }

    pub fn set_state(&self, state: &str) {
        *self.state.lock().expect("fake downloader lock") = state.to_string();
    }

    pub fn set_category_torrents(&self, torrents: Vec<TorrentInfo>) {
        *self.category_torrents.lock().expect("fake downloader lock") = torrents;
    }

    pub fn set_content_path(&self, path: &std::path::Path) {
        *self.content_path.lock().expect("fake downloader lock") =
            Some(path.to_string_lossy().into_owned());
    }
}

#[async_trait]
impl DownloadProvider for FakeDownloadProvider {
    fn name(&self) -> &'static str {
        "fake-downloader"
    }

    async fn add(
        &self,
        source: DownloadSource,
        category: &str,
        tag: &str,
    ) -> Result<(), DownloadError> {
        if *self.failing.lock().expect("fake downloader lock") {
            return Err(DownloadError::NotConfigured);
        }

        self.added
            .lock()
            .expect("fake downloader lock")
            .push(AddedRelease {
                category: category.to_string(),
                tag: tag.to_string(),
                magnet: matches!(source, DownloadSource::Magnet(_)),
            });

        Ok(())
    }

    async fn status(&self, id: &str) -> Result<Option<TorrentInfo>, DownloadError> {
        if *self.failing.lock().expect("fake downloader lock") {
            return Err(DownloadError::NotConfigured);
        }
        if *self.missing.lock().expect("fake downloader lock") {
            return Ok(None);
        }
        Ok(Some(self.torrent(id)))
    }

    async fn find_by_tag(&self, _tag: &str) -> Result<Vec<TorrentInfo>, DownloadError> {
        if *self.failing.lock().expect("fake downloader lock") {
            return Err(DownloadError::NotConfigured);
        }
        if *self.missing.lock().expect("fake downloader lock") || self.added().is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![self.torrent("fakehash")])
    }

    async fn list_category(&self, _category: &str) -> Result<Vec<TorrentInfo>, DownloadError> {
        if *self.failing.lock().expect("fake downloader lock")
            || *self.category_failing.lock().expect("fake downloader lock")
        {
            return Err(DownloadError::NotConfigured);
        }
        Ok(self
            .category_torrents
            .lock()
            .expect("fake downloader lock")
            .clone())
    }

    async fn cancel_owned(
        &self,
        id: &str,
        _category: &str,
        _delete_files: bool,
    ) -> Result<bool, DownloadError> {
        if *self.failing.lock().expect("fake downloader lock") {
            return Err(DownloadError::NotConfigured);
        }
        self.canceled
            .lock()
            .expect("fake downloader lock")
            .push(id.to_string());
        Ok(true)
    }

    async fn test_connection(&self) -> Result<String, DownloadError> {
        if *self.failing.lock().expect("fake downloader lock") {
            return Err(DownloadError::NotConfigured);
        }
        Ok("fake-downloader 1.0".to_string())
    }
}

impl FakeDownloadProvider {
    fn torrent(&self, id: &str) -> TorrentInfo {
        let hash = self
            .hash
            .lock()
            .expect("fake downloader lock")
            .clone()
            .unwrap_or_else(|| id.to_string());
        let progress = *self.progress.lock().expect("fake downloader lock");
        let state = {
            let state = self.state.lock().expect("fake downloader lock").clone();
            if state.is_empty() {
                "downloading".to_string()
            } else {
                state
            }
        };

        let content_path = self
            .content_path
            .lock()
            .expect("fake downloader lock")
            .clone()
            .or_else(|| Some("/downloads/fake torrent".to_string()));

        TorrentInfo {
            hash,
            name: "fake torrent".to_string(),
            state,
            progress,
            size: 1024,
            downloaded: (1024.0 * progress) as i64,
            save_path: "/downloads".to_string(),
            content_path,
            category: Some(crate::qbittorrent::DEFAULT_CATEGORY.to_string()),
            tags: Some(format!("{}{}", crate::qbittorrent::TAG_PREFIX, "fake")),
            eta: Some(60),
            dl_speed: Some(1024),
        }
    }
}
