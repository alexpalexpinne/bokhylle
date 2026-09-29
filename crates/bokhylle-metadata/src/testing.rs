use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;

use crate::{
    AuthorCandidate, AuthorProfile, MetadataError, MetadataProvider, MetadataQuery, MetadataResult,
};

#[derive(Default)]
pub struct FakeMetadataProvider {
    results: Mutex<Vec<MetadataResult>>,
    cover: Mutex<Option<Vec<u8>>>,
    calls: AtomicUsize,
    last_limit: AtomicUsize,
    name: Option<&'static str>,
    failing: Mutex<bool>,
    author_olids: Mutex<std::collections::HashMap<String, String>>,
    author_photos: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    author_candidates: Mutex<Vec<AuthorCandidate>>,
    author_profiles: Mutex<std::collections::HashMap<String, AuthorProfile>>,
    author_profile_calls: AtomicUsize,
    query_filter: AtomicBool,
    paging: AtomicBool,
}

impl FakeMetadataProvider {
    pub fn new(results: Vec<MetadataResult>) -> Self {
        Self {
            results: Mutex::new(results),
            ..Default::default()
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Gives the fake a provider name so registry tests can distinguish
    /// primary from fallback.
    pub fn named(mut self, name: &'static str) -> Self {
        self.name = Some(name);
        self
    }

    /// The `limit` of the most recent search, for candidate-count tests.
    pub fn last_limit(&self) -> usize {
        self.last_limit.load(Ordering::SeqCst)
    }

    /// Makes `search` honour the query instead of returning every fixture,
    /// so tests can prove fallbacks that depend on an empty result.
    pub fn with_query_filter(mut self) -> Self {
        self.query_filter = AtomicBool::new(true);
        self
    }

    pub fn with_paging(mut self) -> Self {
        self.paging = AtomicBool::new(true);
        self
    }

    pub fn set_failing(&self, failing: bool) {
        *self.failing.lock().expect("fake provider lock") = failing;
    }

    pub fn set_cover(&self, bytes: Vec<u8>) {
        *self.cover.lock().expect("fake provider lock") = Some(bytes);
    }

    pub fn set_author_candidates(&self, candidates: Vec<AuthorCandidate>) {
        *self.author_candidates.lock().expect("fake provider lock") = candidates;
    }

    pub fn set_author_profile(&self, key: &str, profile: AuthorProfile) {
        self.author_profiles
            .lock()
            .expect("fake provider lock")
            .insert(key.to_string(), profile);
    }

    pub fn author_profile_calls(&self) -> usize {
        self.author_profile_calls.load(Ordering::SeqCst)
    }

    pub fn set_author(&self, name: &str, olid: &str, photo: Vec<u8>) {
        self.author_olids
            .lock()
            .expect("fake provider lock")
            .insert(name.to_string(), olid.to_string());
        self.author_photos
            .lock()
            .expect("fake provider lock")
            .insert(olid.to_string(), photo);
    }
}

#[async_trait]
impl MetadataProvider for FakeMetadataProvider {
    async fn get_author_profile(
        &self,
        provider_key: &str,
    ) -> Result<Option<AuthorProfile>, MetadataError> {
        self.author_profile_calls.fetch_add(1, Ordering::SeqCst);
        if *self.failing.lock().expect("fake provider lock") {
            return Err(MetadataError::Status(503));
        }
        Ok(self
            .author_profiles
            .lock()
            .expect("fake provider lock")
            .get(provider_key)
            .cloned())
    }

    fn name(&self) -> &'static str {
        self.name.unwrap_or("fake")
    }

    async fn search(&self, query: &MetadataQuery) -> Result<Vec<MetadataResult>, MetadataError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.last_limit.store(query.limit, Ordering::SeqCst);

        if *self.failing.lock().expect("fake provider lock") {
            return Err(MetadataError::Status(503));
        }

        let results = self.results.lock().expect("fake provider lock").clone();
        if !self.query_filter.load(Ordering::SeqCst) {
            return Ok(results);
        }
        let needle = query
            .title
            .as_deref()
            .or(query.author.as_deref())
            .or(query.free_text.as_deref())
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if let Some(series) = needle
            .strip_prefix("series:\"")
            .and_then(|value| value.strip_suffix('"'))
        {
            return Ok(results
                .into_iter()
                .filter(|result| {
                    result
                        .series
                        .as_deref()
                        .is_some_and(|value| value.to_lowercase() == series)
                })
                .collect());
        }
        Ok(results
            .into_iter()
            .filter(|result| result.title.to_lowercase().contains(&needle))
            .collect())
    }

    async fn search_page(&self, query: &MetadataQuery) -> Result<crate::SearchPage, MetadataError> {
        let items = self.search(query).await?;
        if !self.paging.load(Ordering::SeqCst) {
            return Ok(crate::SearchPage { items, next: None });
        }
        let offset = query
            .continuation
            .as_deref()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = query.limit.clamp(1, 50);
        let end = (offset + limit).min(items.len());
        let page = items.get(offset..end).unwrap_or_default().to_vec();
        let next = (end < items.len()).then(|| end.to_string());
        Ok(crate::SearchPage { items: page, next })
    }

    async fn get_book(&self, provider_key: &str) -> Result<Option<MetadataResult>, MetadataError> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        if *self.failing.lock().expect("fake provider lock") {
            return Err(MetadataError::Status(503));
        }

        Ok(self
            .results
            .lock()
            .expect("fake provider lock")
            .iter()
            .find(|result| result.provider_key == provider_key)
            .cloned())
    }

    async fn fetch_cover(&self, cover_id: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = cover_id;
        if *self.failing.lock().expect("fake provider lock") {
            return Err(MetadataError::Status(503));
        }
        Ok(self.cover.lock().expect("fake provider lock").clone())
    }

    async fn fetch_cover_by_isbn(&self, _isbn: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        Ok(self.cover.lock().expect("fake provider lock").clone())
    }

    async fn resolve_author_olid(&self, name: &str) -> Result<Option<String>, MetadataError> {
        Ok(self
            .author_olids
            .lock()
            .expect("fake provider lock")
            .get(name)
            .cloned())
    }

    async fn search_authors(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<AuthorCandidate>, MetadataError> {
        if *self.failing.lock().expect("fake provider lock") {
            return Err(MetadataError::Status(503));
        }
        Ok(self
            .author_candidates
            .lock()
            .expect("fake provider lock")
            .clone())
    }

    async fn fetch_author_photo(&self, olid: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if *self.failing.lock().expect("fake provider lock") {
            return Err(MetadataError::Status(503));
        }
        Ok(self
            .author_photos
            .lock()
            .expect("fake provider lock")
            .get(olid)
            .cloned())
    }
}
