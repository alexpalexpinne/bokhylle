//! A configurable Newznab indexer. Candidate events contain only the GUID;
//! the API key is added to the retrieval URL after selection.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use bokhylle_core::VERSION;
use reqwest::{Client, Url};
use roxmltree::Node;

use crate::{
    evaluator,
    model::{AcquisitionMethod, ExpectedBook, ReleaseCandidate, SourceIdentity},
    provider::{IndexerError, IndexerProvider, SearchOutcome},
    response::read_limited,
};

#[derive(Debug, thiserror::Error)]
pub enum NewznabError {
    #[error("invalid Newznab configuration")]
    Configuration,
    #[error("Newznab could not be reached")]
    Request,
    #[error("Newznab returned HTTP {0}")]
    Status(u16),
    #[error("invalid Newznab response")]
    InvalidResponse,
}

pub struct NewznabClient {
    endpoint: Url,
    api_key: String,
    categories: Vec<i32>,
    client: Client,
}

impl NewznabClient {
    pub fn new(endpoint: &str, api_key: &str, categories: Vec<i32>) -> Result<Self, NewznabError> {
        let mut endpoint = Url::parse(endpoint.trim()).map_err(|_| NewznabError::Configuration)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint
                .query_pairs()
                .any(|(key, _)| matches!(key.as_ref(), "apikey" | "t" | "q" | "cat"))
        {
            return Err(NewznabError::Configuration);
        }
        endpoint.set_fragment(None);
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .user_agent(format!("Bokhylle/{VERSION}"))
            .build()
            .map_err(|_| NewznabError::Configuration)?;
        Ok(Self {
            endpoint,
            api_key: api_key.to_owned(),
            categories,
            client,
        })
    }

    fn url(&self, action: &str, query: Option<&str>) -> Url {
        let mut url = self.endpoint.clone();
        {
            let mut params = url.query_pairs_mut();
            params.append_pair("t", action);
            if !self.api_key.is_empty() {
                params.append_pair("apikey", &self.api_key);
            }
            if let Some(query) = query {
                params.append_pair("q", query);
                params.append_pair("limit", "100");
                if !self.categories.is_empty() {
                    params.append_pair(
                        "cat",
                        &self
                            .categories
                            .iter()
                            .map(i32::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
            }
        }
        url
    }

    async fn xml(&self, url: Url) -> Result<Vec<u8>, NewznabError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|_| NewznabError::Request)?;
        if !response.status().is_success() {
            return Err(NewznabError::Status(response.status().as_u16()));
        }
        read_limited(response, 8 * 1024 * 1024)
            .await
            .map_err(|_| NewznabError::InvalidResponse)
    }

    fn get_url(&self, guid: &str) -> Result<String, NewznabError> {
        if guid.is_empty() || guid.len() > 1024 {
            return Err(NewznabError::InvalidResponse);
        }
        let mut url = self.url("get", None);
        url.query_pairs_mut().append_pair("id", guid);
        Ok(url.to_string())
    }
}

fn parse_feed(xml: &[u8]) -> Result<Vec<ReleaseCandidate>, NewznabError> {
    let text = std::str::from_utf8(xml).map_err(|_| NewznabError::InvalidResponse)?;
    let doc = roxmltree::Document::parse(text).map_err(|_| NewznabError::InvalidResponse)?;
    if !doc.root_element().has_tag_name("rss") {
        return Err(NewznabError::InvalidResponse);
    }
    let mut releases = Vec::new();
    for item in doc
        .descendants()
        .filter(|node| node.has_tag_name("item"))
        .take(100)
    {
        let Some(title) = child(item, "title")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(guid) = child(item, "guid").and_then(safe_guid) else {
            continue;
        };
        let size_bytes = item
            .children()
            .find(|node| node.has_tag_name("enclosure"))
            .and_then(|node| node.attribute("length"))
            .or_else(|| {
                item.descendants()
                    .find(|node| {
                        node.has_tag_name("attr") && node.attribute("name") == Some("size")
                    })
                    .and_then(|node| node.attribute("value"))
            })
            .and_then(|size| size.parse().ok())
            .unwrap_or(0);
        releases.push(ReleaseCandidate {
            id: guid.clone(),
            source: Some(SourceIdentity {
                kind: "newznab".into(),
                name: "Newznab".into(),
                key: guid.clone(),
            }),
            method: Some(AcquisitionMethod::Nzb { guid }),
            title: title.to_owned(),
            indexer: Some("Newznab".into()),
            size_bytes,
            seeders: None,
            leechers: None,
            download_url: None,
            magnet_url: None,
            info_url: None,
            detected_title: None,
            detected_author: None,
            detected_format: None,
            detected_language: None,
            detected_volume: None,
            is_collection: false,
            is_audiobook: false,
            is_comic: false,
        });
    }
    Ok(releases)
}

fn safe_guid(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let value = if let Ok(url) = Url::parse(raw) {
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        url.query_pairs()
            .find(|(key, _)| key == "id" || key == "guid")
            .map(|(_, value)| value.into_owned())
            .or_else(|| {
                url.path_segments()
                    .and_then(|mut parts| parts.next_back().map(str::to_owned))
            })?
    } else {
        raw.to_owned()
    };
    if value.is_empty()
        || value.len() > 1024
        || value
            .chars()
            .any(|ch| matches!(ch, '?' | '&' | '=' | '/' | '\\'))
        || value.to_ascii_lowercase().contains("apikey")
    {
        return None;
    }
    Some(value)
}

fn child<'a>(item: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    item.children()
        .find(|node| node.is_element() && node.tag_name().name() == name)
        .and_then(|node| node.text())
}

#[async_trait]
impl IndexerProvider for NewznabClient {
    fn name(&self) -> &'static str {
        "newznab"
    }

    async fn search_book(&self, book: &ExpectedBook) -> Result<SearchOutcome, IndexerError> {
        let mut queries = Vec::new();
        let mut best: Option<(Vec<ReleaseCandidate>, f32)> = None;
        for query in crate::prowlarr::query_candidates(book) {
            let candidates = parse_feed(&self.xml(self.url("search", Some(&query))).await?)?;
            queries.push(format!("{query} ({} candidates)", candidates.len()));
            if candidates.is_empty() {
                continue;
            }
            let evaluated = evaluator::rank(book, &candidates);
            if evaluated.iter().any(evaluator::is_confident_match) {
                return Ok(SearchOutcome {
                    queries,
                    candidates,
                });
            }
            let confidence = evaluated
                .iter()
                .filter(|release| !release.rejected())
                .map(|release| release.confidence)
                .fold(0.0_f32, f32::max);
            if best
                .as_ref()
                .is_none_or(|(_, current)| confidence > *current)
            {
                best = Some((candidates, confidence));
            }
        }
        Ok(SearchOutcome {
            queries,
            candidates: best.map(|(candidates, _)| candidates).unwrap_or_default(),
        })
    }

    async fn fetch_torrent(
        &self,
        _release: &ReleaseCandidate,
    ) -> Result<Arc<Vec<u8>>, IndexerError> {
        Err(IndexerError::NoDownloadLink)
    }

    async fn test_connection(&self) -> Result<String, IndexerError> {
        let xml = self.xml(self.url("caps", None)).await?;
        let text = std::str::from_utf8(&xml).map_err(|_| NewznabError::InvalidResponse)?;
        let doc = roxmltree::Document::parse(text).map_err(|_| NewznabError::InvalidResponse)?;
        if !doc.root_element().has_tag_name("caps") {
            return Err(NewznabError::InvalidResponse.into());
        }
        Ok("Newznab".into())
    }

    fn nzb_url(&self, release: &ReleaseCandidate) -> Result<String, IndexerError> {
        let Some(AcquisitionMethod::Nzb { guid }) = &release.method else {
            return Err(IndexerError::NoDownloadLink);
        };
        Ok(self.get_url(guid)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    #[test]
    fn parses_nzb_without_exposing_enclosure_key() {
        let xml = br#"<rss><channel><item><title>Dune.EN.EPUB</title><guid>abc</guid><enclosure url="https://example.test/get?apikey=secret" length="1234" type="application/x-nzb"/></item></channel></rss>"#;
        let candidates = parse_feed(xml).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].size_bytes, 1234);
        assert!(
            !serde_json::to_string(&candidates[0])
                .unwrap()
                .contains("secret")
        );
    }

    #[test]
    fn guid_url_is_reduced_to_the_identifier() {
        assert_eq!(
            safe_guid("https://indexer.test/get?id=abc&apikey=secret"),
            Some("abc".into())
        );
        assert_eq!(safe_guid("abc?apikey=secret"), None);
    }

    #[tokio::test]
    async fn searches_and_builds_private_get_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api")).and(query_param("t", "search"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"<rss><channel><item><title>Frank.Herbert.Dune.EN.EPUB</title><guid>abc</guid><enclosure length="3200000" type="application/x-nzb" url="https://indexer.test/get?apikey=secret"/></item></channel></rss>"#,
                "application/xml",
            )).mount(&server).await;
        let client =
            NewznabClient::new(&format!("{}/api", server.uri()), "secret", vec![7000]).unwrap();
        let result = client
            .search_book(&ExpectedBook {
                title: "Dune".into(),
                authors: vec!["Frank Herbert".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result.candidates.len(), 1);
        let candidate = &result.candidates[0];
        assert!(!serde_json::to_string(candidate).unwrap().contains("secret"));
        let url = client.nzb_url(candidate).unwrap();
        assert!(url.contains("t=get"));
        assert!(url.contains("id=abc"));
        assert!(url.contains("apikey=secret"));
    }
}
