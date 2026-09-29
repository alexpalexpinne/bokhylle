//! Per-capability provider policy. Configuration chooses providers; the
//! registry decides which provider a capability is allowed to contact.
//!
//! - metadata: the configured primary, plus the fallback in Automatic mode
//! - ratings: the configured ratings provider (or Disabled)
//! - covers: the provider that owns the identifier, but only when it is
//!   configured for metadata; artwork never reaches a provider that is only
//!   configured for ratings

use std::sync::Arc;

use bokhylle_metadata::{MetadataCapabilities, MetadataProvider};

pub struct MetadataRegistry {
    metadata: Arc<dyn MetadataProvider>,
    fallback: Option<Arc<dyn MetadataProvider>>,
    ratings: Arc<dyn MetadataProvider>,
}

impl MetadataRegistry {
    pub fn new(
        metadata: Arc<dyn MetadataProvider>,
        fallback: Option<Arc<dyn MetadataProvider>>,
        ratings: Arc<dyn MetadataProvider>,
    ) -> Self {
        Self {
            metadata,
            fallback,
            ratings,
        }
    }

    pub fn metadata(&self) -> &Arc<dyn MetadataProvider> {
        &self.metadata
    }

    pub fn fallback(&self) -> Option<&Arc<dyn MetadataProvider>> {
        self.fallback.as_ref()
    }

    pub fn ratings(&self) -> &Arc<dyn MetadataProvider> {
        &self.ratings
    }

    /// The provider that owns a name, when it is configured for metadata.
    /// Resolution is provider-aware: a fallback result stays a fallback
    /// result for detail, likes and acquisition.
    pub fn provider(&self, provider_name: &str) -> Option<Arc<dyn MetadataProvider>> {
        if self.metadata.name() == provider_name {
            return Some(self.metadata.clone());
        }
        if let Some(fallback) = &self.fallback
            && fallback.name() == provider_name
        {
            return Some(fallback.clone());
        }
        None
    }

    /// The declared roles of a configured metadata provider.
    pub fn capabilities(&self, provider_name: &str) -> Option<MetadataCapabilities> {
        self.provider(provider_name).map(|p| p.capabilities())
    }

    pub fn durable_identity(&self, provider_name: &str) -> bool {
        self.capabilities(provider_name)
            .is_some_and(|capabilities| capabilities.durable_identity)
    }

    pub fn persistent_metadata(&self, provider_name: &str) -> bool {
        self.capabilities(provider_name)
            .is_some_and(|capabilities| capabilities.persistent_metadata)
    }

    /// Providers that may own a primary catalogue link, in preference order.
    pub fn durable_providers(&self) -> Vec<&'static str> {
        let mut providers: Vec<&'static str> = Vec::new();
        if self.metadata.capabilities().durable_identity {
            providers.push(self.metadata.name());
        }
        if let Some(fallback) = &self.fallback
            && fallback.capabilities().durable_identity
        {
            providers.push(fallback.name());
        }
        providers
    }

    /// Cover retrieval is a metadata capability: only providers that declare
    /// it may serve artwork.
    pub fn covers(&self, provider_name: &str) -> Option<Arc<dyn MetadataProvider>> {
        self.provider(provider_name)
            .filter(|provider| provider.capabilities().covers)
    }

    /// The cover provider to assume when a request has no explicit one.
    pub fn default_cover_provider(&self) -> &'static str {
        self.metadata.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use bokhylle_metadata::{MetadataError, MetadataQuery, MetadataResult};

    struct Named(&'static str);

    struct Enrichment(&'static str);

    #[async_trait]
    impl MetadataProvider for Enrichment {
        fn name(&self) -> &'static str {
            self.0
        }

        fn capabilities(&self) -> MetadataCapabilities {
            MetadataCapabilities {
                durable_identity: false,
                persistent_metadata: false,
                covers: true,
                ratings: false,
                author_search: true,
            }
        }

        async fn search(
            &self,
            _query: &MetadataQuery,
        ) -> Result<Vec<MetadataResult>, MetadataError> {
            Ok(Vec::new())
        }

        async fn get_book(
            &self,
            _provider_key: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(None)
        }
    }

    #[async_trait]
    impl MetadataProvider for Named {
        fn name(&self) -> &'static str {
            self.0
        }

        async fn search(
            &self,
            _query: &MetadataQuery,
        ) -> Result<Vec<MetadataResult>, MetadataError> {
            Ok(Vec::new())
        }

        async fn get_book(
            &self,
            _provider_key: &str,
        ) -> Result<Option<MetadataResult>, MetadataError> {
            Ok(None)
        }
    }

    #[test]
    fn covers_only_come_from_metadata_providers() {
        let registry = MetadataRegistry::new(
            Arc::new(Named("google_books")),
            None,
            Arc::new(Named("openlibrary")),
        );
        assert!(registry.covers("google_books").is_some());
        assert!(
            registry.covers("openlibrary").is_none(),
            "a ratings-only provider must never serve artwork"
        );
        assert_eq!(registry.default_cover_provider(), "google_books");
    }

    #[test]
    fn durable_providers_follow_declared_capabilities() {
        let registry = MetadataRegistry::new(
            Arc::new(Named("openlibrary")),
            Some(Arc::new(Enrichment("google_books"))),
            Arc::new(Named("openlibrary")),
        );
        assert_eq!(registry.durable_providers(), vec!["openlibrary"]);
        assert!(registry.durable_identity("openlibrary"));
        assert!(!registry.durable_identity("google_books"));
        assert!(registry.persistent_metadata("openlibrary"));
        assert!(!registry.persistent_metadata("google_books"));
        assert!(
            registry.covers("google_books").is_some(),
            "enrichment providers may still serve covers"
        );
        assert!(
            !registry.durable_identity("unknown"),
            "an unknown provider never owns identity"
        );
    }

    #[test]
    fn automatic_allows_the_fallback_for_covers() {
        let registry = MetadataRegistry::new(
            Arc::new(Named("openlibrary")),
            Some(Arc::new(Named("google_books"))),
            Arc::new(Named("openlibrary")),
        );
        assert!(registry.covers("google_books").is_some());
        assert!(registry.covers("openlibrary").is_some());
    }
}
