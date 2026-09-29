//! A single configurable Torznab endpoint (Jackett, a direct indexer, or any
//! compatible aggregator). Results use the existing torrent evaluator.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bokhylle_core::VERSION;
use reqwest::{Client, Url, header};
use roxmltree::Node;

use crate::evaluator;
use crate::model::{AcquisitionMethod, ExpectedBook, ReleaseCandidate, SourceIdentity};
use crate::provider::{IndexerError, IndexerProvider, SearchOutcome};
use crate::response::{BodyReadError, read_limited};

const MAX_XML_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TORRENT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum TorznabError {
    #[error("invalid Torznab configuration")]
    Configuration,
    #[error("Torznab could not be reached")]
    Request,
    #[error("Torznab returned HTTP {0}")]
    Status(u16),
    #[error("invalid Torznab response: {0}")]
    Invalid(&'static str),
}

pub struct TorznabClient {
    endpoint: Url,
    api_key: String,
    categories: Vec<i32>,
    allow_private_destinations: bool,
}

impl TorznabClient {
    pub fn new(endpoint: &str, api_key: &str, categories: Vec<i32>) -> Result<Self, TorznabError> {
        let mut endpoint = Url::parse(endpoint.trim()).map_err(|_| TorznabError::Configuration)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint
                .query_pairs()
                .any(|(key, _)| matches!(key.as_ref(), "apikey" | "t" | "q" | "cat"))
        {
            return Err(TorznabError::Configuration);
        }
        endpoint.set_fragment(None);
        Ok(Self {
            endpoint,
            api_key: api_key.to_string(),
            categories,
            allow_private_destinations: false,
        })
    }

    /// Allows loopback fixture servers in tests. Production keeps the strict
    /// foreign-destination rule; the configured endpoint itself is trusted.
    pub fn with_allow_private_destinations(mut self, allow: bool) -> Self {
        self.allow_private_destinations = allow;
        self
    }

    fn api_url(&self, action: &str, query: Option<&str>) -> Url {
        let mut url = self.endpoint.clone();
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("t", action);
            if !self.api_key.is_empty() {
                pairs.append_pair("apikey", &self.api_key);
            }
            if let Some(query) = query {
                pairs.append_pair("q", query);
                pairs.append_pair("limit", "100");
                if !self.categories.is_empty() {
                    pairs.append_pair(
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

    async fn xml(&self, url: Url) -> Result<Vec<u8>, TorznabError> {
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("Bokhylle/{VERSION}"))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|_| TorznabError::Request)?;
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|_| TorznabError::Request)?;
        if !response.status().is_success() {
            return Err(TorznabError::Status(response.status().as_u16()));
        }
        read_limited(response, MAX_XML_BYTES)
            .await
            .map_err(|error| match error {
                BodyReadError::TooLarge => TorznabError::Invalid("feed exceeds the size limit"),
                BodyReadError::Request(_) => TorznabError::Request,
            })
    }

    pub async fn search(&self, query: &str) -> Result<Vec<ReleaseCandidate>, TorznabError> {
        let bytes = self.xml(self.api_url("search", Some(query))).await?;
        parse_search(&bytes, &self.endpoint, &self.api_key)
    }

    pub async fn check(&self) -> Result<String, TorznabError> {
        let bytes = self.xml(self.api_url("caps", None)).await?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| TorznabError::Invalid("capabilities are not UTF-8 XML"))?;
        let document = roxmltree::Document::parse(text)
            .map_err(|_| TorznabError::Invalid("capabilities are not XML"))?;
        if document.root_element().tag_name().name() != "caps" {
            return Err(TorznabError::Invalid(
                "capabilities response is not Torznab",
            ));
        }
        let server = document
            .descendants()
            .find(|node| node.has_tag_name("server"));
        Ok(server
            .and_then(|node| node.attribute("version"))
            .unwrap_or("Torznab")
            .to_string())
    }

    pub async fn search_book(&self, book: &ExpectedBook) -> Result<SearchOutcome, TorznabError> {
        let mut queries = Vec::new();
        let mut best: Option<(Vec<ReleaseCandidate>, f32)> = None;
        for query in crate::prowlarr::query_candidates(book) {
            let candidates = self.search(&query).await?;
            queries.push(format!("{query} ({} candidates)", candidates.len()));
            if candidates.is_empty() {
                continue;
            }
            let evaluated = evaluator::rank(book, &candidates);
            if evaluated
                .iter()
                .any(|release| !release.rejected() && release.confidence >= 0.6)
            {
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

    async fn torrent_bytes(&self, raw: &str) -> Result<Arc<Vec<u8>>, TorznabError> {
        let mut url = Url::parse(raw).map_err(|_| TorznabError::Invalid("invalid torrent URL"))?;
        for hop in 0..=MAX_REDIRECTS {
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(TorznabError::Invalid("torrent URL is not HTTP(S)"));
            }
            let trusted = url.origin() == self.endpoint.origin();
            if !trusted
                && url
                    .query_pairs()
                    .any(|(key, value)| key == "apikey" && value == self.api_key)
            {
                return Err(TorznabError::Invalid("torrent URL exposes the indexer key"));
            }
            let client = self.client_for(&url, trusted).await?;
            let mut request_url = url.clone();
            if trusted
                && !self.api_key.is_empty()
                && !request_url.query_pairs().any(|(key, _)| key == "apikey")
            {
                request_url
                    .query_pairs_mut()
                    .append_pair("apikey", &self.api_key);
            }
            let response = client
                .get(request_url)
                .send()
                .await
                .map_err(|_| TorznabError::Request)?;
            if response.status().is_redirection() {
                if hop == MAX_REDIRECTS {
                    return Err(TorznabError::Invalid("too many torrent redirects"));
                }
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(TorznabError::Invalid("torrent redirect has no location"))?;
                url = url
                    .join(location)
                    .map_err(|_| TorznabError::Invalid("invalid torrent redirect"))?;
                continue;
            }
            if !response.status().is_success() {
                return Err(TorznabError::Status(response.status().as_u16()));
            }
            let bytes =
                read_limited(response, MAX_TORRENT_BYTES)
                    .await
                    .map_err(|error| match error {
                        BodyReadError::TooLarge => {
                            TorznabError::Invalid("torrent exceeds the size limit")
                        }
                        BodyReadError::Request(_) => TorznabError::Request,
                    })?;
            if bytes.first() != Some(&b'd') {
                return Err(TorznabError::Invalid("download is not a torrent file"));
            }
            return Ok(Arc::new(bytes));
        }
        Err(TorznabError::Invalid("too many torrent redirects"))
    }

    async fn client_for(&self, url: &Url, trusted: bool) -> Result<Client, TorznabError> {
        let host = url
            .host_str()
            .ok_or(TorznabError::Invalid("torrent URL has no host"))?;
        let host = host.trim_matches(['[', ']']);
        let port = url.port_or_known_default().unwrap_or(80);
        let addresses: Vec<SocketAddr> = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::net::lookup_host((host, port)),
        )
        .await
        .map_err(|_| TorznabError::Request)?
        .map_err(|_| TorznabError::Request)?
        .collect();
        if addresses.is_empty()
            || (!trusted
                && !self.allow_private_destinations
                && addresses.iter().any(|address| !public_ip(address.ip())))
        {
            return Err(TorznabError::Invalid("torrent destination is not public"));
        }
        Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .resolve(host, addresses[0])
            .build()
            .map_err(|_| TorznabError::Request)
    }
}

fn parse_search(
    bytes: &[u8],
    base: &Url,
    api_key: &str,
) -> Result<Vec<ReleaseCandidate>, TorznabError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| TorznabError::Invalid("search response is not UTF-8 XML"))?;
    let document = roxmltree::Document::parse(text)
        .map_err(|_| TorznabError::Invalid("search response is not XML"))?;
    let root = document.root_element();
    if root.has_tag_name("error") {
        return Err(TorznabError::Invalid("indexer reported an API error"));
    }
    if !root.has_tag_name("rss") {
        return Err(TorznabError::Invalid("search response is not RSS"));
    }
    let mut releases = Vec::new();
    for item in root
        .descendants()
        .filter(|node| node.has_tag_name("item"))
        .take(100)
    {
        if let Some(release) = map_item(item, base, api_key) {
            releases.push(release);
        }
    }
    Ok(releases)
}

fn map_item(item: Node<'_, '_>, base: &Url, api_key: &str) -> Option<ReleaseCandidate> {
    let title = child_text(item, "title")?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let enclosure = item.children().find(|node| node.has_tag_name("enclosure"));
    if enclosure
        .and_then(|node| node.attribute("type"))
        .is_some_and(|mime| mime.contains("nzb"))
    {
        return None;
    }
    let enclosed = enclosure.and_then(|node| node.attribute("url"));
    let magnet = attribute(item, "magneturl")
        .or_else(|| child_text(item, "link").filter(|url| url.starts_with("magnet:?")))
        .or_else(|| enclosed.filter(|url| url.starts_with("magnet:?")))
        .filter(|url| url.starts_with("magnet:?"))
        .map(str::to_string);
    let download_url = enclosed
        .filter(|url| !url.starts_with("magnet:?"))
        .and_then(|url| absolute_http(base, url, api_key));
    if magnet.is_none() && download_url.is_none() {
        return None;
    }
    let id = child_text(item, "guid").unwrap_or(&title).to_string();
    let indexer = attribute(item, "indexer").unwrap_or("Torznab").to_string();
    let size_bytes = enclosure
        .and_then(|node| node.attribute("length"))
        .or_else(|| child_text(item, "size"))
        .or_else(|| attribute(item, "size"))
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let seeders = attribute(item, "seeders").and_then(|value| value.parse().ok());
    let leechers = attribute(item, "peers")
        .or_else(|| attribute(item, "leechers"))
        .and_then(|value| value.parse().ok());
    Some(ReleaseCandidate {
        source: Some(SourceIdentity {
            kind: "torznab".to_string(),
            name: indexer.clone(),
            key: id.clone(),
        }),
        method: Some(AcquisitionMethod::Torrent {
            magnet_url: magnet.clone(),
            download_url: download_url.clone(),
        }),
        id,
        title,
        indexer: Some(indexer),
        size_bytes,
        seeders,
        leechers,
        download_url,
        magnet_url: magnet,
        info_url: child_text(item, "link").and_then(|url| absolute_http(base, url, api_key)),
        detected_title: None,
        detected_author: None,
        detected_format: None,
        detected_language: None,
        detected_volume: None,
        is_collection: false,
        is_audiobook: false,
        is_comic: false,
    })
}

fn child_text<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
        .and_then(|child| child.text())
}

fn attribute<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| {
            child.is_element()
                && child.tag_name().name() == "attr"
                && child.attribute("name") == Some(name)
        })
        .and_then(|child| child.attribute("value"))
}

fn absolute_http(base: &Url, raw: &str, api_key: &str) -> Option<String> {
    let mut url = base.join(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    if !api_key.is_empty()
        && url
            .query_pairs()
            .any(|(key, value)| key == "apikey" && value == api_key)
    {
        let retained: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(key, value)| !(key == "apikey" && value == api_key))
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        url.set_query(None);
        if !retained.is_empty() {
            url.query_pairs_mut().extend_pairs(retained);
        }
    }
    Some(url.to_string())
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || octets[0] == 0
                || octets[0] >= 240
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 198 && (18..=19).contains(&octets[1])))
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(mapped));
            }
            let segments = ip.segments();
            !ip.is_loopback()
                && !ip.is_unspecified()
                && !ip.is_unique_local()
                && !ip.is_unicast_link_local()
                && !ip.is_multicast()
                && (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

#[async_trait::async_trait]
impl IndexerProvider for TorznabClient {
    fn name(&self) -> &'static str {
        "torznab"
    }

    async fn search_book(&self, book: &ExpectedBook) -> Result<SearchOutcome, IndexerError> {
        TorznabClient::search_book(self, book)
            .await
            .map_err(Into::into)
    }

    async fn fetch_torrent(
        &self,
        release: &ReleaseCandidate,
    ) -> Result<Arc<Vec<u8>>, IndexerError> {
        let url = match &release.method {
            Some(AcquisitionMethod::Torrent { download_url, .. }) => download_url.as_deref(),
            _ => release.download_url.as_deref(),
        }
        .ok_or(IndexerError::NoDownloadLink)?;
        self.torrent_bytes(url).await.map_err(Into::into)
    }

    async fn test_connection(&self) -> Result<String, IndexerError> {
        self.check().await.map_err(Into::into)
    }
}
