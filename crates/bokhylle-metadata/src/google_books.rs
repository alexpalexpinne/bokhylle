//! Google Books as an optional provider: an alternative catalogue for
//! households that prefer it or that hit Open Library's duplicate works.
//! Kept behind the `MetadataProvider` trait so nothing depends on it.

use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use async_trait::async_trait;
use serde::Deserialize;

use crate::{
    MetadataCapabilities, MetadataError, MetadataProvider, MetadataQuery, MetadataResult,
    SearchPage,
};

const DEFAULT_BASE_URL: &str = "https://www.googleapis.com/books/v1";
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(250);
const MAX_ATTEMPTS: u32 = 3;
const MAX_JSON_BYTES: usize = 10 * 1024 * 1024;

pub struct GoogleBooksClient {
    http: reqwest::Client,
    cover_http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    last_request: Mutex<Instant>,
}

impl GoogleBooksClient {
    pub fn new(api_key: Option<String>) -> Result<Self, MetadataError> {
        Self::with_base_url(DEFAULT_BASE_URL, api_key)
    }

    pub fn with_base_url(base_url: &str, api_key: Option<String>) -> Result<Self, MetadataError> {
        let http = reqwest::Client::builder()
            .user_agent(format!(
                "Bokhylle/{} (self-hosted book server)",
                bokhylle_core::VERSION
            ))
            .timeout(Duration::from_secs(15))
            .build()?;

        // Cover fetches never follow redirects: an allowed host must not be
        // able to bounce us to an arbitrary address.
        let cover_http = reqwest::Client::builder()
            .user_agent(format!(
                "Bokhylle/{} (self-hosted book server)",
                bokhylle_core::VERSION
            ))
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        Ok(Self {
            http,
            cover_http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.filter(|key| !key.trim().is_empty()),
            last_request: Mutex::new(Instant::now() - MIN_REQUEST_INTERVAL),
        })
    }

    async fn throttle(&self) {
        let mut last_request = self.last_request.lock().await;
        let elapsed = last_request.elapsed();
        if elapsed < MIN_REQUEST_INTERVAL {
            tokio::time::sleep(MIN_REQUEST_INTERVAL - elapsed).await;
        }
        *last_request = Instant::now();
    }

    fn url(&self, path: &str, params: &[(&str, String)]) -> String {
        let mut url = format!("{}{}", self.base_url, path);
        let mut separator = '?';
        for (key, value) in params {
            url.push(separator);
            url.push_str(key);
            url.push('=');
            url.push_str(&urlencode(value));
            separator = '&';
        }
        if let Some(api_key) = &self.api_key {
            url.push(separator);
            url.push_str("key=");
            url.push_str(api_key);
        }
        url
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, MetadataError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.throttle().await;
            match self.http.get(url).send().await {
                Ok(mut response) if response.status().is_success() => {
                    if response
                        .content_length()
                        .is_some_and(|length| length > MAX_JSON_BYTES as u64)
                    {
                        return Err(MetadataError::Status(413));
                    }
                    let mut bytes = Vec::new();
                    while let Some(chunk) = response.chunk().await? {
                        if chunk.len() > MAX_JSON_BYTES - bytes.len() {
                            return Err(MetadataError::Status(413));
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    return serde_json::from_slice(&bytes)
                        .map_err(|error| MetadataError::Invalid(error.to_string()));
                }
                Ok(response) if response.status().as_u16() == 404 => {
                    return Err(MetadataError::Status(404));
                }
                Ok(response) if response.status().is_server_error() && attempt < MAX_ATTEMPTS => {
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                }
                Ok(response) => return Err(MetadataError::Status(response.status().as_u16())),
                Err(error)
                    if (error.is_timeout() || error.is_connect()) && attempt < MAX_ATTEMPTS =>
                {
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn volume(&self, id: &str) -> Result<Option<Volume>, MetadataError> {
        let url = self.url(&format!("/volumes/{}", urlencode(id)), &[]);
        match self.get_json::<Volume>(&url).await {
            Ok(volume) => Ok(Some(volume)),
            Err(MetadataError::Status(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn search_results(
        &self,
        query: &MetadataQuery,
    ) -> Result<(Vec<MetadataResult>, usize), MetadataError> {
        let limit = query.limit.clamp(1, 40);
        let q = if let Some(isbn) = query.isbn.as_deref().filter(|isbn| !isbn.trim().is_empty()) {
            format!("isbn:{}", isbn.trim())
        } else if let Some(title) = query
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            match query
                .author
                .as_deref()
                .filter(|author| !author.trim().is_empty())
            {
                Some(author) => format!(
                    "intitle:\"{}\" inauthor:\"{}\"",
                    title.trim(),
                    author.trim()
                ),
                None => format!("intitle:\"{}\"", title.trim()),
            }
        } else if let Some(text) = query
            .free_text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            text.trim().to_string()
        } else {
            return Ok((Vec::new(), 0));
        };

        let mut params = vec![
            ("q", q),
            ("maxResults", limit.to_string()),
            ("printType", "books".to_string()),
        ];
        if let Some(start) = query
            .continuation
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
        {
            params.push(("startIndex", start.to_string()));
        }
        let url = self.url("/volumes", &params);
        let response: VolumesResponse = self.get_json(&url).await?;
        let entries = response.items.unwrap_or_default();
        let count = entries.len();
        Ok((
            entries.into_iter().filter_map(result_from_volume).collect(),
            count,
        ))
    }
}

const ALLOWED_COVER_HOSTS: [&str; 3] = [
    "books.google.com",
    "books.googleusercontent.com",
    "googleusercontent.com",
];

/// Resolves an opaque Google cover reference to a safe image URL:
/// - a volume id (the value we expose) builds the Books content URL directly,
///   so no extra metadata request is needed;
/// - a legacy allowed Google URL is still accepted for old cached rows.
fn cover_url_for(cover_id: &str) -> Option<String> {
    let looks_like_volume_id = !cover_id.is_empty()
        && !cover_id.contains("://")
        && cover_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        });
    if looks_like_volume_id {
        return Some(format!(
            "https://books.google.com/books/content?id={}&printsec=frontcover&img=1&zoom=1",
            urlencode(cover_id)
        ));
    }

    let parsed = reqwest::Url::parse(cover_id).ok()?;
    let host = parsed.host_str()?;
    let host_allowed = ALLOWED_COVER_HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")));
    if !host_allowed {
        return None;
    }
    match parsed.scheme() {
        "https" => Some(parsed.to_string()),
        // Google's API still hands out http image URLs for its own host;
        // upgrade rather than allowing plain http.
        "http" if host == "books.google.com" => {
            Some(parsed.to_string().replacen("http://", "https://", 1))
        }
        _ => None,
    }
}

fn urlencode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

#[derive(Debug, Deserialize)]
struct VolumesResponse {
    items: Option<Vec<Volume>>,
}

#[derive(Debug, Deserialize)]
struct Volume {
    id: String,
    #[serde(rename = "volumeInfo", default)]
    info: VolumeInfo,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VolumeInfo {
    title: Option<String>,
    subtitle: Option<String>,
    authors: Option<Vec<String>>,
    published_date: Option<String>,
    description: Option<String>,
    publisher: Option<String>,
    language: Option<String>,
    categories: Option<Vec<String>>,
    average_rating: Option<f64>,
    ratings_count: Option<i64>,
    industry_identifiers: Option<Vec<IndustryIdentifier>>,
    image_links: Option<ImageLinks>,
}

#[derive(Debug, Deserialize)]
struct IndustryIdentifier {
    #[serde(rename = "type")]
    kind: String,
    identifier: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageLinks {
    thumbnail: Option<String>,
    small_thumbnail: Option<String>,
    small: Option<String>,
    medium: Option<String>,
    large: Option<String>,
}

/// Prefer the documented medium/small sizes for UI covers.
fn best_cover_url(info: &VolumeInfo) -> Option<String> {
    let links = info.image_links.as_ref()?;
    [
        links.medium.as_ref(),
        links.small.as_ref(),
        links.large.as_ref(),
        links.thumbnail.as_ref(),
        links.small_thumbnail.as_ref(),
    ]
    .into_iter()
    .flatten()
    .next()
    .cloned()
}

const MAX_COVER_BYTES: usize = 5 * 1024 * 1024;

fn looks_like_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        || bytes.starts_with(b"GIF8")
        || (bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
        || bytes.starts_with(b"BM")
}

fn year_from_date(date: &str) -> Option<i32> {
    date.get(0..4)?.parse::<i32>().ok()
}

fn result_from_volume(volume: Volume) -> Option<MetadataResult> {
    let info = volume.info;
    let title = info.title?;
    let full_title = match &info.subtitle {
        Some(subtitle) if !subtitle.trim().is_empty() => format!("{title}: {subtitle}"),
        _ => title,
    };

    let mut isbn10 = None;
    let mut isbn13 = None;
    for identifier in info.industry_identifiers.unwrap_or_default() {
        match identifier.kind.as_str() {
            "ISBN_10" if isbn10.is_none() => isbn10 = Some(identifier.identifier),
            "ISBN_13" if isbn13.is_none() => isbn13 = Some(identifier.identifier),
            _ => {}
        }
    }

    // Only advertise a cover when the volume has one, and expose the opaque
    // volume id instead of the transport URL.
    let cover_id = info
        .image_links
        .and_then(|links| links.thumbnail.or(links.small_thumbnail))
        .map(|_| volume.id.clone());

    let languages: Vec<String> = info.language.clone().into_iter().collect();
    Some(MetadataResult {
        provider: "google_books".to_string(),
        provider_key: volume.id,
        title: full_title,
        authors: info.authors.unwrap_or_default(),
        year: info.published_date.as_deref().and_then(year_from_date),
        language: info.language,
        languages,
        isbn10,
        isbn13,
        description: info.description,
        publisher: info.publisher,
        cover_id,
        subjects: info.categories.unwrap_or_default(),
        rating_average: info.average_rating,
        rating_count: info.ratings_count,
        ..Default::default()
    })
}

#[async_trait]
impl MetadataProvider for GoogleBooksClient {
    fn name(&self) -> &'static str {
        "google_books"
    }

    /// Google fills gaps and enriches; Open Library stays the durable
    /// catalogue identity, so Google records get the short cache TTL and are
    /// never preferred as a book's primary link.
    fn capabilities(&self) -> MetadataCapabilities {
        MetadataCapabilities {
            durable_identity: false,
            persistent_metadata: false,
            covers: true,
            ratings: false,
            // No stable author search is implemented for Google Books.
            author_search: false,
        }
    }

    async fn search(&self, query: &MetadataQuery) -> Result<Vec<MetadataResult>, MetadataError> {
        Ok(self.search_results(query).await?.0)
    }

    async fn search_page(&self, query: &MetadataQuery) -> Result<SearchPage, MetadataError> {
        let limit = query.limit.clamp(1, 40);
        let (items, returned) = self.search_results(query).await?;
        let next = if returned >= limit {
            let start = query
                .continuation
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            Some((start + limit as u64).to_string())
        } else {
            None
        };
        Ok(SearchPage { items, next })
    }

    async fn get_book(&self, provider_key: &str) -> Result<Option<MetadataResult>, MetadataError> {
        Ok(self
            .volume(provider_key)
            .await?
            .and_then(result_from_volume))
    }

    async fn fetch_ratings(&self, provider_key: &str) -> Result<Option<(f64, i64)>, MetadataError> {
        let Some(volume) = self.volume(provider_key).await? else {
            return Ok(None);
        };
        Ok(
            match (volume.info.average_rating, volume.info.ratings_count) {
                (Some(average), Some(count)) if count > 0 => Some((average, count)),
                _ => None,
            },
        )
    }

    async fn fetch_cover(&self, cover_id: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        // The public identifier is an opaque volume id; the documented
        // imageLinks are the source of truth for where its cover lives. A
        // legacy allowed URL is still resolved for old cached rows.
        let url = if cover_id.contains("://") {
            match cover_url_for(cover_id) {
                Some(url) => url,
                None => return Ok(None),
            }
        } else {
            let Some(volume) = self.volume(cover_id).await? else {
                return Ok(None);
            };
            let Some(raw) = best_cover_url(&volume.info) else {
                return Ok(None);
            };
            match cover_url_for(&raw) {
                Some(url) => url,
                None => return Ok(None),
            }
        };

        // The URL is client-influenced, so it must never become an arbitrary
        // fetch: https (or Google's http URL upgraded), a known Google image
        // host, and no redirects.
        match self.cover_http.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                if response
                    .content_length()
                    .is_some_and(|length| length > MAX_COVER_BYTES as u64)
                {
                    return Ok(None);
                }
                // Hard streaming limit: never buffer more than the cap,
                // even without a Content-Length or with chunked encoding.
                let mut response = response;
                let mut bytes: Vec<u8> = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    if chunk.len() > MAX_COVER_BYTES - bytes.len() {
                        return Ok(None);
                    }
                    bytes.extend_from_slice(&chunk);
                }
                if !looks_like_image(&bytes) {
                    return Ok(None);
                }
                Ok(Some(bytes))
            }
            Ok(_) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_volume_with_identifiers_rating_and_categories() {
        let volume: Volume = serde_json::from_str(
            r#"{
                "id": "abc123",
                "volumeInfo": {
                    "title": "House Atreides",
                    "subtitle": "Prelude to Dune",
                    "authors": ["Brian Herbert", "Kevin J. Anderson"],
                    "publishedDate": "1999-10-01",
                    "description": "A prequel.",
                    "publisher": "Bantam",
                    "language": "en",
                    "categories": ["Fiction", "Science Fiction"],
                    "averageRating": 4.1,
                    "ratingsCount": 312,
                    "industryIdentifiers": [
                        {"type": "ISBN_10", "identifier": "0553108444"},
                        {"type": "ISBN_13", "identifier": "9780553108446"}
                    ],
                    "imageLinks": {"thumbnail": "https://books.google.com/thumb.jpg"}
                }
            }"#,
        )
        .unwrap();

        let result = result_from_volume(volume).unwrap();
        assert_eq!(result.provider, "google_books");
        assert_eq!(result.provider_key, "abc123");
        assert_eq!(result.title, "House Atreides: Prelude to Dune");
        assert_eq!(result.year, Some(1999));
        assert_eq!(result.isbn13.as_deref(), Some("9780553108446"));
        assert_eq!(result.subjects, vec!["Fiction", "Science Fiction"]);
        assert_eq!(result.cover_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn cover_refs_are_opaque_or_known_google_urls() {
        // The exposed value is a volume id; it resolves without extra lookups.
        assert_eq!(
            cover_url_for("abc123-_X").as_deref(),
            Some(
                "https://books.google.com/books/content?id=abc123-_X&printsec=frontcover&img=1&zoom=1"
            )
        );
        // Legacy cached rows may still hold allowed URLs.
        assert!(cover_url_for("https://books.google.com/books/content?id=x").is_some());
        assert!(
            cover_url_for("http://books.google.com/books/content?id=x")
                .is_some_and(|url| url.starts_with("https://"))
        );
        assert!(cover_url_for("http://127.0.0.1:8090/healthz").is_none());
        assert!(cover_url_for("https://192.168.1.20/admin").is_none());
        assert!(cover_url_for("https://books.google.com.evil.test/x").is_none());
        assert!(cover_url_for("file:///etc/passwd").is_none());
    }

    #[test]
    fn picks_the_best_documented_cover_size() {
        let volume: Volume = serde_json::from_str(
            r#"{
                "id": "zyx987",
                "volumeInfo": {
                    "title": "Covered",
                    "imageLinks": {
                        "smallThumbnail": "https://books.google.com/small-thumb.jpg",
                        "thumbnail": "https://books.google.com/thumb.jpg",
                        "small": "https://books.google.com/small.jpg",
                        "medium": "https://books.google.com/medium.jpg"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            best_cover_url(&volume.info).as_deref(),
            Some("https://books.google.com/medium.jpg")
        );

        // A large cover must never lose to an 80 px thumbnail.
        let large_and_thumb: Volume = serde_json::from_str(
            r#"{"id": "b", "volumeInfo": {"title": "T", "imageLinks": {"large": "https://books.google.com/l.jpg", "smallThumbnail": "https://books.google.com/t.jpg"}}}"#,
        )
        .unwrap();
        assert_eq!(
            best_cover_url(&large_and_thumb.info).as_deref(),
            Some("https://books.google.com/l.jpg")
        );
    }

    #[test]
    fn only_real_images_are_cached() {
        assert!(looks_like_image(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(looks_like_image(&[0x89, b'P', b'N', b'G', 0x0D]));
        assert!(!looks_like_image(b"<html><body>error</body></html>"));
        assert!(!looks_like_image(b""));
    }

    #[test]
    fn discovered_covers_expose_the_volume_id_not_a_url() {
        let volume: Volume = serde_json::from_str(
            r#"{
                "id": "zyx987",
                "volumeInfo": {
                    "title": "Covered",
                    "imageLinks": {"thumbnail": "http://books.google.com/books/content?id=zyx987"}
                }
            }"#,
        )
        .unwrap();
        let result = result_from_volume(volume).unwrap();
        assert_eq!(result.cover_id.as_deref(), Some("zyx987"));
    }

    #[test]
    fn builds_isbn_and_title_queries() {
        let client = GoogleBooksClient::with_base_url("https://example.test", None).unwrap();
        let isbn = client.url(
            "/volumes",
            &[
                ("q", "isbn:9780553108446".to_string()),
                ("maxResults", "1".to_string()),
            ],
        );
        assert!(isbn.starts_with("https://example.test/volumes?q=isbn%3A9780553108446"));

        let keyed = GoogleBooksClient::with_base_url("https://example.test", Some("k".into()))
            .unwrap()
            .url("/volumes", &[("q", "dune".to_string())]);
        assert!(keyed.ends_with("&key=k"));
    }
}
