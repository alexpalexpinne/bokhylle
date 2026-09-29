pub mod disabled;
pub mod google_books;
pub mod open_library;
pub mod testing;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use bokhylle_core::identity::normalize_text;

#[derive(Debug, Clone, Default)]
pub struct MetadataQuery {
    pub title: Option<String>,
    pub author: Option<String>,
    pub isbn: Option<String>,
    pub free_text: Option<String>,
    pub limit: usize,
    /// Opaque provider cursor returned as `SearchPage::next`.
    pub continuation: Option<String>,
}

/// A provider-side author candidate with a stable provider key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct AuthorCandidate {
    pub name: String,
    pub provider: String,
    pub provider_key: String,
}

/// Optional biographical details tied to a provider's author identifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorProfile {
    pub bio: Option<String>,
    pub birth_date: Option<String>,
    pub death_date: Option<String>,
}

/// One page of provider results plus the cursor for the next page.
#[derive(Debug, Default)]
pub struct SearchPage {
    pub items: Vec<MetadataResult>,
    pub next: Option<String>,
}

impl MetadataQuery {
    pub fn cache_fragment(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        if let Some(title) = self
            .title
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(format!("t={}", normalize_text(title)));
        }
        if let Some(author) = self
            .author
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(format!("a={}", normalize_text(author)));
        }
        if let Some(isbn) = self
            .isbn
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(format!("i={}", isbn.trim()));
        }
        if let Some(text) = self
            .free_text
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            parts.push(format!("q={}", normalize_text(text)));
        }

        parts.join("&")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataResult {
    pub provider: String,
    pub provider_key: String,
    pub edition_key: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    /// The primary language, kept for existing consumers; `languages` is the
    /// complete normalized set when the provider exposes one.
    pub language: Option<String>,
    /// Every language the work is available in, normalized to Bokhylle codes.
    #[serde(default)]
    pub languages: Vec<String>,
    pub isbn10: Option<String>,
    pub isbn13: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub cover_id: Option<String>,
    #[serde(default)]
    pub subjects: Vec<String>,
    /// Engagement signals for search ranking. Providers that do not expose
    /// one simply leave it empty; a missing signal contributes no bonus.
    #[serde(default)]
    pub rating_average: Option<f64>,
    #[serde(default)]
    pub rating_count: Option<i64>,
    #[serde(default)]
    pub edition_count: Option<i64>,
    /// Provider-defined engagement proxy (want-to-read / reading-log counts).
    #[serde(default)]
    pub popularity: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("provider returned status {0}")]
    Status(u16),
    #[error("invalid provider response: {0}")]
    Invalid(String),
}

/// The roles a provider may play for the catalogue. Defaults describe the
/// primary catalogue provider: durable, persistently cached, able to serve
/// covers and author searches. Enrichment-only, ratings-only and disabled
/// providers override what they must not do.
#[derive(Debug, Clone, Copy)]
pub struct MetadataCapabilities {
    /// May own a durable catalogue identity (a book/author's primary link).
    pub durable_identity: bool,
    /// Provider records may be persisted for the long metadata cache TTL.
    pub persistent_metadata: bool,
    pub covers: bool,
    pub ratings: bool,
    pub author_search: bool,
}

impl Default for MetadataCapabilities {
    fn default() -> Self {
        Self {
            durable_identity: true,
            persistent_metadata: true,
            covers: true,
            ratings: false,
            author_search: true,
        }
    }
}

#[async_trait]
pub trait MetadataProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Roles this provider may play; policy belongs here rather than in the
    /// routes that happen to call it.
    fn capabilities(&self) -> MetadataCapabilities {
        MetadataCapabilities::default()
    }

    async fn search(&self, query: &MetadataQuery) -> Result<Vec<MetadataResult>, MetadataError>;

    async fn get_book(&self, provider_key: &str) -> Result<Option<MetadataResult>, MetadataError>;

    /// True author search: candidate names with stable provider keys, kept
    /// separate from the exact `resolve_author_olid` resolver.
    async fn search_authors(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<AuthorCandidate>, MetadataError> {
        Ok(Vec::new())
    }

    async fn get_author_profile(
        &self,
        _provider_key: &str,
    ) -> Result<Option<AuthorProfile>, MetadataError> {
        Ok(None)
    }

    /// Pageable search; providers that cannot page return everything once
    /// and no continuation.
    async fn search_page(&self, query: &MetadataQuery) -> Result<SearchPage, MetadataError> {
        Ok(SearchPage {
            items: self.search(query).await?,
            next: None,
        })
    }

    /// Average rating and rating count for a work, when the provider has
    /// them. Kept separate from `MetadataResult` so the source is explicit.
    async fn fetch_ratings(&self, provider_key: &str) -> Result<Option<(f64, i64)>, MetadataError> {
        let _ = provider_key;
        Ok(None)
    }

    async fn fetch_cover(&self, cover_id: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        let _ = cover_id;
        Ok(None)
    }

    /// A cover for search cards. Providers without image variants can reuse
    /// their normal cover response.
    async fn fetch_cover_thumbnail(
        &self,
        cover_id: &str,
    ) -> Result<Option<Vec<u8>>, MetadataError> {
        self.fetch_cover(cover_id).await
    }

    async fn fetch_cover_by_isbn(&self, isbn: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        let _ = isbn;
        Ok(None)
    }

    /// Resolve an author's Open Library id from their name (exact match only).
    async fn resolve_author_olid(&self, name: &str) -> Result<Option<String>, MetadataError> {
        let _ = name;
        Ok(None)
    }

    async fn fetch_author_photo(&self, olid: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        let _ = olid;
        Ok(None)
    }
}
