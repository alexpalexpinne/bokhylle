use serde::Deserialize;
use serde::de::DeserializeOwned;

use bokhylle_core::identity::core_title;

use crate::evaluator;
use crate::model::{AcquisitionMethod, ExpectedBook, ReleaseCandidate, SourceIdentity};
use crate::provider::{IndexerError, IndexerProvider, SearchOutcome};
use crate::response::{BodyReadError, read_limited};

#[derive(Debug, thiserror::Error)]
pub enum ProwlarrError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("prowlarr returned status {0}")]
    Status(u16),
    #[error("invalid prowlarr response: {0}")]
    Invalid(String),
}

pub const BOOK_CATEGORY: i32 = 7000;

pub struct ProwlarrClient {
    http: reqwest::Client,
    torrent_http: reqwest::Client,
    base_url: String,
    base_origin: Option<reqwest::Url>,
    api_key: String,
    max_torrent_bytes: u64,
    allow_private_destinations: bool,
}

const MAX_TORRENT_REDIRECTS: usize = 5;

pub const MAX_TORRENT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_API_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct StatusResponse {
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProwlarrRelease {
    guid: Option<String>,
    title: Option<String>,
    indexer: Option<String>,
    #[serde(default)]
    size: i64,
    seeders: Option<i64>,
    leechers: Option<i64>,
    download_url: Option<String>,
    magnet_url: Option<String>,
    info_url: Option<String>,
}

impl ProwlarrClient {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self, ProwlarrError> {
        let user_agent = format!(
            "Bokhylle/{} (self-hosted book server)",
            bokhylle_core::VERSION
        );

        let http = reqwest::Client::builder()
            .user_agent(user_agent.clone())
            .timeout(std::time::Duration::from_secs(20))
            .build()?;

        // Torrent retrieval follows redirects manually so the API key can be
        // dropped as soon as a redirect leaves the Prowlarr origin.
        let torrent_http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let base_url = base_url.trim_end_matches('/').to_string();
        let base_origin = reqwest::Url::parse(&base_url).ok();

        Ok(Self {
            http,
            torrent_http,
            base_url,
            base_origin,
            api_key: api_key.to_string(),
            max_torrent_bytes: MAX_TORRENT_BYTES,
            allow_private_destinations: false,
        })
    }

    pub fn with_max_torrent_bytes(mut self, max_torrent_bytes: u64) -> Self {
        self.max_torrent_bytes = max_torrent_bytes;
        self
    }

    /// Testing only: production keeps the public-destination policy so a
    /// compromised indexer result cannot reach loopback or LAN services.
    pub fn with_allow_private_destinations(mut self, allow: bool) -> Self {
        self.allow_private_destinations = allow;
        self
    }

    pub async fn test_connection(&self) -> Result<String, ProwlarrError> {
        let response = self
            .http
            .get(format!("{}/api/v1/system/status", self.base_url))
            .header("X-Api-Key", &self.api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ProwlarrError::Status(response.status().as_u16()));
        }

        let status: StatusResponse = limited_json(response).await?;

        Ok(status.version.unwrap_or_else(|| "unknown".to_string()))
    }

    pub async fn search(
        &self,
        query: &str,
        categories: &[i32],
    ) -> Result<Vec<ReleaseCandidate>, ProwlarrError> {
        let mut request = self
            .http
            .get(format!("{}/api/v1/search", self.base_url))
            .header("X-Api-Key", &self.api_key)
            .query(&[
                ("query", query.to_string()),
                ("type", "search".to_string()),
                ("limit", "100".to_string()),
            ]);

        for category in categories {
            request = request.query(&[("categories", category.to_string())]);
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(ProwlarrError::Status(response.status().as_u16()));
        }

        let releases: Vec<ProwlarrRelease> = limited_json(response).await?;

        Ok(releases.into_iter().filter_map(map_release).collect())
    }

    pub async fn search_book(&self, book: &ExpectedBook) -> Result<SearchOutcome, ProwlarrError> {
        let mut queries = Vec::new();
        let mut best: Option<(Vec<ReleaseCandidate>, f32)> = None;

        for query in query_candidates(book) {
            let candidates = self.search(&query, &[BOOK_CATEGORY]).await?;
            queries.push(format!("{query} ({} candidates)", candidates.len()));

            if candidates.is_empty() {
                continue;
            }

            let evaluated = evaluator::rank(book, &candidates);
            let suitable = evaluated.iter().any(evaluator::is_confident_match);

            if suitable {
                return Ok(SearchOutcome {
                    queries,
                    candidates,
                });
            }

            // No confident candidate yet: keep the strongest viable set so the
            // evaluator can still offer NeedsSelection instead of "no release".
            let best_confidence = evaluated
                .iter()
                .filter(|release| !release.rejected())
                .map(|release| release.confidence)
                .fold(0.0_f32, f32::max);

            match &best {
                Some((_, existing)) if *existing >= best_confidence => {}
                _ => best = Some((candidates, best_confidence)),
            }
        }

        Ok(SearchOutcome {
            queries,
            candidates: best.map(|(candidates, _)| candidates).unwrap_or_default(),
        })
    }
}

async fn limited_json<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ProwlarrError> {
    let bytes = read_limited(response, MAX_API_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BodyReadError::Request(error) => ProwlarrError::Request(error),
            BodyReadError::TooLarge => {
                ProwlarrError::Invalid("the response exceeds the size limit".to_string())
            }
        })?;
    serde_json::from_slice(&bytes).map_err(|error| ProwlarrError::Invalid(error.to_string()))
}

pub(crate) fn query_candidates(book: &ExpectedBook) -> Vec<String> {
    let mut queries: Vec<String> = Vec::new();

    let full_title = book.title.trim();
    let core = core_title(full_title);
    let author = book
        .authors
        .first()
        .map(|author| author.trim())
        .filter(|author| !author.is_empty());

    if !core.is_empty() {
        if let Some(author) = author {
            queries.push(format!("{core} {author}"));
        }
        queries.push(core.clone());
    }
    if !full_title.is_empty() && full_title != core {
        queries.push(full_title.to_string());
    }
    if let Some(isbn) = book
        .isbn
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        queries.push(isbn.to_string());
    }

    queries.retain(|query| !query.trim().is_empty());
    queries.dedup();
    queries
}

fn map_release(release: ProwlarrRelease) -> Option<ReleaseCandidate> {
    let title = release.title?.trim().to_string();
    if title.is_empty() {
        return None;
    }

    let id = release.guid.unwrap_or_else(|| title.clone());
    Some(ReleaseCandidate {
        source: Some(SourceIdentity {
            kind: "prowlarr".to_string(),
            name: release
                .indexer
                .clone()
                .unwrap_or_else(|| "Prowlarr".to_string()),
            key: id.clone(),
        }),
        method: Some(AcquisitionMethod::Torrent {
            magnet_url: release.magnet_url.clone(),
            download_url: release.download_url.clone(),
        }),
        id,
        title,
        indexer: release.indexer,
        size_bytes: release.size,
        seeders: release.seeders,
        leechers: release.leechers,
        download_url: release.download_url,
        magnet_url: release.magnet_url,
        info_url: release.info_url,
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

/// Foreign torrent destinations must be public HTTP(S): a compromised
/// indexer result must not point Bokhylle's server at loopback or LAN services.
/// Named hosts are resolved here and every resolved address must be public.
async fn foreign_destination_allowed(url: &reqwest::Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return false;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => is_public_ip(ip),
        Err(_) => {
            let port = url.port_or_known_default().unwrap_or(80);
            match tokio::net::lookup_host((host.as_str(), port)).await {
                Ok(addresses) => {
                    let addresses: Vec<_> = addresses.collect();
                    !addresses.is_empty()
                        && addresses.iter().all(|address| is_public_ip(address.ip()))
                }
                Err(_) => false,
            }
        }
    }
}

fn is_public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(address) => {
            !(address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_unspecified()
                || address.is_documentation()
                || address.is_multicast())
        }
        std::net::IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_public_ip(std::net::IpAddr::V4(mapped));
            }
            !(address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address.is_multicast())
        }
    }
}

impl ProwlarrClient {
    fn is_prowlarr_origin(&self, url: &reqwest::Url) -> bool {
        self.base_origin
            .as_ref()
            .is_some_and(|base| base.origin() == url.origin())
    }

    async fn follow_redirects(
        &self,
        url: &mut reqwest::Url,
        same_origin: &mut bool,
    ) -> Result<reqwest::Response, IndexerError> {
        for hop in 0..=MAX_TORRENT_REDIRECTS {
            if !*same_origin
                && !self.allow_private_destinations
                && !foreign_destination_allowed(url).await
            {
                return Err(IndexerError::Prowlarr(ProwlarrError::Invalid(
                    "foreign torrent destination is not a public address".to_string(),
                )));
            }
            let mut request = self.torrent_http.get(url.clone());
            if *same_origin {
                request = request.header("X-Api-Key", &self.api_key);
            }

            let response = request.send().await?;
            if !response.status().is_redirection() {
                return Ok(response);
            }

            if hop == MAX_TORRENT_REDIRECTS {
                return Err(IndexerError::Prowlarr(ProwlarrError::Invalid(
                    "too many redirects while fetching the torrent".to_string(),
                )));
            }

            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    IndexerError::Prowlarr(ProwlarrError::Invalid(
                        "redirect without a location header".to_string(),
                    ))
                })?;

            *url = url.join(location).map_err(|error| {
                IndexerError::Prowlarr(ProwlarrError::Invalid(format!(
                    "invalid redirect location: {error}"
                )))
            })?;
            *same_origin = self.is_prowlarr_origin(url);
        }

        Err(IndexerError::Prowlarr(ProwlarrError::Invalid(
            "too many redirects while fetching the torrent".to_string(),
        )))
    }
}

#[async_trait::async_trait]
impl IndexerProvider for ProwlarrClient {
    fn name(&self) -> &'static str {
        "prowlarr"
    }

    async fn search_book(&self, book: &ExpectedBook) -> Result<SearchOutcome, IndexerError> {
        ProwlarrClient::search_book(self, book)
            .await
            .map_err(IndexerError::from)
    }

    async fn fetch_torrent(
        &self,
        release: &ReleaseCandidate,
    ) -> Result<std::sync::Arc<Vec<u8>>, IndexerError> {
        let download_url = match &release.method {
            Some(AcquisitionMethod::Torrent { download_url, .. }) => download_url.as_deref(),
            _ => release.download_url.as_deref(),
        };
        let Some(download_url) = download_url else {
            return Err(IndexerError::NoDownloadLink);
        };

        let mut url = reqwest::Url::parse(download_url).map_err(|error| {
            IndexerError::Prowlarr(ProwlarrError::Invalid(format!(
                "invalid download url: {error}"
            )))
        })?;

        let mut same_origin = self.is_prowlarr_origin(&url);
        let response = self.follow_redirects(&mut url, &mut same_origin).await?;
        let mut response = response;

        if !response.status().is_success() {
            return Err(IndexerError::Prowlarr(ProwlarrError::Status(
                response.status().as_u16(),
            )));
        }

        if let Some(length) = response.content_length()
            && length > self.max_torrent_bytes
        {
            return Err(IndexerError::Prowlarr(ProwlarrError::Invalid(
                "the torrent file exceeds the size limit".to_string(),
            )));
        }

        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() as u64 + chunk.len() as u64 > self.max_torrent_bytes {
                return Err(IndexerError::Prowlarr(ProwlarrError::Invalid(
                    "the torrent file exceeds the size limit".to_string(),
                )));
            }
            bytes.extend_from_slice(&chunk);
        }

        Ok(std::sync::Arc::new(bytes))
    }

    async fn test_connection(&self) -> Result<String, IndexerError> {
        ProwlarrClient::test_connection(self)
            .await
            .map_err(IndexerError::from)
    }
}

#[cfg(test)]
mod query_tests {
    use super::*;

    #[test]
    fn prefers_core_title_over_subtitles() {
        let book = ExpectedBook {
            title: "Strange Dogs: An Expanse Novella (The Expanse)".to_string(),
            authors: vec!["James S. A. Corey".to_string()],
            isbn: Some("9780316217576".to_string()),
            ..Default::default()
        };

        let queries = query_candidates(&book);
        assert_eq!(queries[0], "Strange Dogs James S. A. Corey");
        assert_eq!(queries[1], "Strange Dogs");
        assert_eq!(queries[2], "Strange Dogs: An Expanse Novella (The Expanse)");
        assert_eq!(queries[3], "9780316217576");
    }

    #[test]
    fn keeps_plain_titles_unchanged() {
        let book = ExpectedBook {
            title: "Project Hail Mary".to_string(),
            authors: vec!["Andy Weir".to_string()],
            ..Default::default()
        };

        assert_eq!(
            query_candidates(&book),
            vec!["Project Hail Mary Andy Weir", "Project Hail Mary"]
        );
    }

    #[test]
    fn only_public_addresses_pass_the_destination_policy() {
        for private in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.10",
            "169.254.1.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
        ] {
            assert!(
                !is_public_ip(private.parse().unwrap()),
                "{private} must not be a public destination"
            );
        }
        for public in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(
                is_public_ip(public.parse().unwrap()),
                "{public} must be a public destination"
            );
        }
    }
}
