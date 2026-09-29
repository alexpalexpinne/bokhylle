//! A provider that answers nothing, used when a feature is explicitly
//! disabled (for example ratings source = disabled).

use async_trait::async_trait;

use crate::{MetadataCapabilities, MetadataError, MetadataProvider, MetadataQuery, MetadataResult};

pub struct DisabledProvider;

#[async_trait]
impl MetadataProvider for DisabledProvider {
    fn name(&self) -> &'static str {
        "disabled"
    }

    fn capabilities(&self) -> MetadataCapabilities {
        MetadataCapabilities {
            durable_identity: false,
            persistent_metadata: false,
            covers: false,
            ratings: false,
            author_search: false,
        }
    }

    async fn search(&self, _query: &MetadataQuery) -> Result<Vec<MetadataResult>, MetadataError> {
        Ok(Vec::new())
    }

    async fn get_book(&self, _provider_key: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(None)
    }

    async fn fetch_ratings(
        &self,
        _provider_key: &str,
    ) -> Result<Option<(f64, i64)>, MetadataError> {
        Ok(None)
    }
}
