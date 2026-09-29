use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

use bokhylle_core::identity::{isbn10_to_isbn13, parse_isbn};

use crate::{
    AuthorCandidate, AuthorProfile, MetadataError, MetadataProvider, MetadataQuery, MetadataResult,
    SearchPage,
};

const DEFAULT_BASE_URL: &str = "https://openlibrary.org";
const DEFAULT_COVERS_URL: &str = "https://covers.openlibrary.org";
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(350);
const MAX_ATTEMPTS: usize = 3;

pub struct OpenLibraryClient {
    http: reqwest::Client,
    base_url: String,
    covers_url: String,
    last_request: Mutex<Instant>,
}

impl OpenLibraryClient {
    pub fn new() -> Result<Self, MetadataError> {
        Self::with_base_url(DEFAULT_BASE_URL, DEFAULT_COVERS_URL)
    }

    pub fn with_base_url(base_url: &str, covers_url: &str) -> Result<Self, MetadataError> {
        let http = reqwest::Client::builder()
            .user_agent(format!(
                "Bokhylle/{} (self-hosted book server)",
                bokhylle_core::VERSION
            ))
            .timeout(Duration::from_secs(15))
            .build()?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            covers_url: covers_url.trim_end_matches('/').to_string(),
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

    /// Provider responses are untrusted input: never buffer past this cap,
    /// with or without a Content-Length.
    const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, MetadataError> {
        let mut attempt = 0;

        loop {
            attempt += 1;
            self.throttle().await;

            match self.http.get(url).send().await {
                Ok(mut response) if response.status().is_success() => {
                    if response
                        .content_length()
                        .is_some_and(|length| length > Self::MAX_RESPONSE_BYTES as u64)
                    {
                        return Err(MetadataError::Status(413));
                    }
                    let mut bytes: Vec<u8> = Vec::new();
                    while let Some(chunk) = response.chunk().await? {
                        if chunk.len() > Self::MAX_RESPONSE_BYTES - bytes.len() {
                            return Err(MetadataError::Status(413));
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    return Ok(bytes);
                }
                Ok(response) if response.status().as_u16() == 404 => {
                    return Err(MetadataError::Status(404));
                }
                Ok(response) if response.status().is_server_error() && attempt < MAX_ATTEMPTS => {
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                }
                Ok(response) => {
                    return Err(MetadataError::Status(response.status().as_u16()));
                }
                Err(error)
                    if (error.is_timeout() || error.is_connect()) && attempt < MAX_ATTEMPTS =>
                {
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, MetadataError> {
        let bytes = self.get_bytes(url).await?;
        serde_json::from_slice(&bytes).map_err(|error| MetadataError::Invalid(error.to_string()))
    }

    async fn search_response(
        &self,
        query: &MetadataQuery,
    ) -> Result<SearchResponse, MetadataError> {
        let limit = query.limit.clamp(1, 50);
        let fields = "key,title,author_name,first_publish_year,isbn,edition_key,cover_i,language,subject,\
                      edition_count,ratings_count,ratings_average,want_to_read_count,\
                      currently_reading_count,already_read_count";
        let mut url = format!(
            "{}/search.json?limit={limit}&fields={fields}",
            self.base_url
        );

        if let Some(offset) = query
            .continuation
            .as_deref()
            .and_then(|value| value.parse::<u64>().ok())
        {
            url.push_str(&format!("&offset={offset}"));
        }

        if let Some(isbn) = trimmed(&query.isbn) {
            url.push_str(&format!("&isbn={}", encode(isbn)));
        } else {
            if let Some(title) = trimmed(&query.title) {
                url.push_str(&format!("&title={}", encode(title)));
            }
            if let Some(author) = trimmed(&query.author) {
                url.push_str(&format!("&author={}", encode(author)));
            }
            if let Some(text) = trimmed(&query.free_text) {
                url.push_str(&format!("&q={}", encode(text)));
            }
        }

        self.get_json(&url).await
    }
}

/// "Last, First" and "First Last" variants; the author search API is strict
/// about the query shape, and library files use both conventions.
fn author_name_variants(name: &str) -> Vec<String> {
    let mut variants = vec![name.trim().to_string()];

    if let Some((last, first)) = name.split_once(',') {
        let inverted = format!("{} {}", first.trim(), last.trim());
        let inverted = inverted.trim().to_string();
        if !inverted.is_empty() && !variants.contains(&inverted) {
            variants.push(inverted);
        }
    }

    variants
}

fn names_match(expected: &str, found: &str) -> bool {
    let expected = bokhylle_core::identity::normalize_text(expected);
    let found = bokhylle_core::identity::normalize_text(found);
    if expected == found {
        return true;
    }

    let mut expected_tokens: Vec<&str> = expected.split_whitespace().collect();
    let mut found_tokens: Vec<&str> = found.split_whitespace().collect();
    expected_tokens.sort_unstable();
    found_tokens.sort_unstable();
    !expected_tokens.is_empty() && expected_tokens == found_tokens
}

#[derive(Deserialize)]
struct AuthorSearchResponse {
    #[serde(default)]
    docs: Vec<AuthorSearchDoc>,
}

#[derive(Deserialize)]
struct AuthorSearchDoc {
    #[serde(default)]
    key: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    docs: Vec<SearchDoc>,
}

#[derive(Deserialize)]
struct SearchDoc {
    key: Option<String>,
    title: Option<String>,
    #[serde(default)]
    author_name: Vec<String>,
    first_publish_year: Option<i32>,
    #[serde(default)]
    isbn: Vec<String>,
    #[serde(default)]
    edition_key: Vec<String>,
    cover_i: Option<i64>,
    #[serde(default)]
    language: Vec<String>,
    #[serde(default)]
    subject: Vec<String>,
    edition_count: Option<i64>,
    ratings_count: Option<i64>,
    ratings_average: Option<f64>,
    want_to_read_count: Option<i64>,
    currently_reading_count: Option<i64>,
    already_read_count: Option<i64>,
}

#[derive(Deserialize)]
struct WorkDoc {
    title: Option<String>,
    first_publish_date: Option<String>,
    #[serde(default)]
    description: Option<Value>,
    #[serde(default)]
    covers: Vec<i64>,
    #[serde(default)]
    authors: Vec<WorkAuthor>,
    #[serde(default)]
    subjects: Vec<SubjectDoc>,
}

/// Open Library work subjects come in two shapes: plain strings on older
/// records and `{ "name": ... }` objects on newer ones. Both must parse or
/// the whole work detail fails.
#[derive(Deserialize)]
#[serde(untagged)]
enum SubjectDoc {
    Plain(String),
    Named { name: Option<String> },
}

impl SubjectDoc {
    fn into_name(self) -> Option<String> {
        match self {
            Self::Plain(name) => Some(name),
            Self::Named { name } => name,
        }
    }
}

#[derive(Deserialize)]
struct WorkAuthor {
    author: Option<KeyRef>,
}

#[derive(Deserialize)]
struct KeyRef {
    key: String,
}

#[derive(Deserialize)]
struct AuthorDoc {
    name: Option<String>,
}

#[derive(Deserialize)]
struct AuthorProfileDoc {
    bio: Option<Value>,
    birth_date: Option<String>,
    death_date: Option<String>,
}

/// Reject paths and arbitrary provider input before it enters an author URL.
pub fn valid_author_olid(key: &str) -> Option<&str> {
    let key = key.trim().strip_prefix("/authors/").unwrap_or(key.trim());
    let digits = key.strip_prefix("OL")?.strip_suffix('A')?;
    (digits.len() <= 16 && !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(key)
}

#[derive(Deserialize)]
struct EditionsResponse {
    #[serde(default)]
    entries: Vec<EditionDoc>,
}

#[derive(Deserialize)]
struct EditionDoc {
    #[serde(default)]
    isbn_13: Vec<String>,
    #[serde(default)]
    isbn_10: Vec<String>,
    #[serde(default)]
    languages: Vec<KeyRef>,
}

#[async_trait]
impl MetadataProvider for OpenLibraryClient {
    async fn get_author_profile(
        &self,
        provider_key: &str,
    ) -> Result<Option<AuthorProfile>, MetadataError> {
        let olid = valid_author_olid(provider_key)
            .ok_or_else(|| MetadataError::Invalid("invalid author identifier".to_string()))?;
        let url = format!("{}/authors/{olid}.json", self.base_url);
        let doc: AuthorProfileDoc = match self.get_json(&url).await {
            Ok(doc) => doc,
            Err(MetadataError::Status(404)) => return Ok(None),
            Err(error) => return Err(error),
        };
        Ok(Some(AuthorProfile {
            bio: doc
                .bio
                .and_then(description_text)
                .and_then(|value| bounded_author_text(&value, 800)),
            birth_date: doc
                .birth_date
                .and_then(|value| bounded_author_text(&value, 80)),
            death_date: doc
                .death_date
                .and_then(|value| bounded_author_text(&value, 80)),
        }))
    }

    async fn search_authors(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<AuthorCandidate>, MetadataError> {
        #[derive(Deserialize)]
        struct AuthorsResponse {
            docs: Option<Vec<AuthorDoc>>,
        }
        #[derive(Deserialize)]
        struct AuthorDoc {
            name: Option<String>,
            key: Option<String>,
        }

        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!(
            "{}/search/authors.json?q={}&limit={}",
            self.base_url,
            encode(trimmed),
            limit.clamp(1, 20)
        );
        let response: AuthorsResponse = self.get_json(&url).await?;
        Ok(response
            .docs
            .unwrap_or_default()
            .into_iter()
            .filter_map(|doc| {
                Some(AuthorCandidate {
                    name: doc.name?,
                    provider: "openlibrary".to_string(),
                    provider_key: doc.key?,
                })
            })
            .collect())
    }

    async fn fetch_ratings(&self, provider_key: &str) -> Result<Option<(f64, i64)>, MetadataError> {
        #[derive(Deserialize)]
        struct Summary {
            average: Option<f64>,
            count: Option<i64>,
        }
        #[derive(Deserialize)]
        struct Ratings {
            summary: Option<Summary>,
        }

        // ISBN lookups can hand back an edition key (OL...M); ratings live
        // on the work, so resolve it through the edition record first.
        let mut key = work_path(provider_key);
        if key.ends_with('M') {
            #[derive(Deserialize)]
            struct EditionWorks {
                works: Option<Vec<WorkRef>>,
            }
            #[derive(Deserialize)]
            struct WorkRef {
                key: String,
            }

            let edition_key = provider_key
                .trim_start_matches('/')
                .trim_start_matches("books/");
            let edition_url = format!("{}/books/{edition_key}.json", self.base_url);
            match self.get_json::<EditionWorks>(&edition_url).await {
                Ok(edition) => match edition.works.and_then(|works| works.into_iter().next()) {
                    Some(work) => key = work.key,
                    None => return Ok(None),
                },
                Err(_) => return Ok(None),
            }
        }

        let url = format!("{}{}/ratings.json", self.base_url, key);
        let ratings: Ratings = self.get_json(&url).await?;
        Ok(ratings
            .summary
            .and_then(|summary| match (summary.average, summary.count) {
                (Some(average), Some(count)) if count > 0 => Some((average, count)),
                _ => None,
            }))
    }

    fn name(&self) -> &'static str {
        "openlibrary"
    }

    async fn search(&self, query: &MetadataQuery) -> Result<Vec<MetadataResult>, MetadataError> {
        Ok(self
            .search_response(query)
            .await?
            .docs
            .into_iter()
            .filter_map(doc_to_result)
            .collect())
    }

    async fn search_page(&self, query: &MetadataQuery) -> Result<SearchPage, MetadataError> {
        let limit = query.limit.clamp(1, 50);
        let response = self.search_response(query).await?;
        let returned = response.docs.len();
        let items = response
            .docs
            .into_iter()
            .filter_map(doc_to_result)
            .collect();
        let next = if returned >= limit {
            let offset = query
                .continuation
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            Some((offset + limit as u64).to_string())
        } else {
            None
        };
        Ok(SearchPage { items, next })
    }

    async fn get_book(&self, provider_key: &str) -> Result<Option<MetadataResult>, MetadataError> {
        let path = work_path(provider_key);
        let work: WorkDoc = match self
            .get_json(&format!("{}{path}.json", self.base_url))
            .await
        {
            Ok(work) => work,
            Err(MetadataError::Status(404)) => return Ok(None),
            Err(error) => return Err(error),
        };

        let author_keys: Vec<String> = work
            .authors
            .iter()
            .filter_map(|entry| entry.author.as_ref().map(|reference| reference.key.clone()))
            .take(3)
            .collect();

        let authors = futures::future::join_all(author_keys.iter().map(|key| async move {
            self.get_json::<AuthorDoc>(&format!("{}{key}.json", self.base_url))
                .await
                .ok()
                .and_then(|author| author.name)
        }))
        .await
        .into_iter()
        .flatten()
        .collect();

        let editions: EditionsResponse = self
            .get_json(&format!("{}{path}/editions.json?limit=10", self.base_url))
            .await
            .unwrap_or(EditionsResponse { entries: vec![] });

        let mut isbn13 = None;
        let mut isbn10 = None;
        let mut languages: Vec<String> = Vec::new();
        let mut edition_key = None;

        for (index, edition) in editions.entries.iter().enumerate() {
            for value in &edition.isbn_13 {
                if let Some(isbn) = parse_isbn(value)
                    && isbn13.is_none()
                    && isbn.len() == 13
                {
                    isbn13 = Some(isbn);
                    edition_key = Some(format!("{path}/editions/{index}"));
                }
            }
            for value in &edition.isbn_10 {
                if let Some(isbn) = parse_isbn(value)
                    && isbn10.is_none()
                    && isbn.len() == 10
                {
                    isbn10 = Some(isbn);
                }
            }
            for language in &edition.languages {
                if let Some(code) = language_from_key(&language.key)
                    && !languages.contains(&code)
                {
                    languages.push(code);
                }
            }
        }

        let cover_id = work.covers.first().map(|cover| cover.to_string());

        Ok(Some(MetadataResult {
            provider: "openlibrary".to_string(),
            provider_key: path,
            edition_key,
            title: work.title.unwrap_or_default(),
            authors,
            // Edition ordering is arbitrary. A work's year and language must
            // not be borrowed from the first returned edition.
            year: work.first_publish_date.as_deref().and_then(year_from_date),
            language: (languages.len() == 1).then(|| languages[0].clone()),
            languages,
            isbn10,
            isbn13,
            series: None,
            series_number: None,
            description: work.description.and_then(description_text),
            publisher: None,
            cover_id,
            subjects: clean_subjects(work.subjects.into_iter().filter_map(SubjectDoc::into_name)),
            ..Default::default()
        }))
    }

    async fn fetch_cover(&self, cover_id: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        let url = format!("{}/b/id/{cover_id}-L.jpg?default=false", self.covers_url);
        match self.get_bytes(&url).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(MetadataError::Status(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn fetch_cover_thumbnail(
        &self,
        cover_id: &str,
    ) -> Result<Option<Vec<u8>>, MetadataError> {
        let url = format!("{}/b/id/{cover_id}-M.jpg?default=false", self.covers_url);
        match self.get_bytes(&url).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(MetadataError::Status(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn fetch_cover_by_isbn(&self, isbn: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        let url = format!("{}/b/isbn/{isbn}-L.jpg?default=false", self.covers_url);
        match self.get_bytes(&url).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(MetadataError::Status(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn resolve_author_olid(&self, name: &str) -> Result<Option<String>, MetadataError> {
        for candidate in author_name_variants(name) {
            let url = format!(
                "{}/search/authors.json?q={}&limit=5",
                self.base_url,
                encode(&candidate)
            );
            let bytes = match self.get_bytes(&url).await {
                Ok(bytes) => bytes,
                Err(MetadataError::Status(404)) => continue,
                Err(error) => return Err(error),
            };

            let parsed: AuthorSearchResponse = match serde_json::from_slice(&bytes) {
                Ok(parsed) => parsed,
                Err(_) => continue,
            };

            let matched = parsed.docs.into_iter().find(|doc| {
                !doc.key.is_empty()
                    && doc
                        .name
                        .as_deref()
                        .is_some_and(|found| names_match(&candidate, found))
            });

            if let Some(doc) = matched {
                return Ok(Some(doc.key));
            }
        }

        Ok(None)
    }

    async fn fetch_author_photo(&self, olid: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        let url = format!("{}/a/olid/{olid}-M.jpg?default=false", self.covers_url);
        match self.get_bytes(&url).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(MetadataError::Status(404)) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

fn bounded_author_text(raw: &str, limit: usize) -> Option<String> {
    let mut result = String::new();
    let mut chars = 0;
    let mut in_tag = false;
    let mut whitespace = false;
    for character in raw.chars() {
        if character == '<' {
            in_tag = true;
            continue;
        }
        if in_tag {
            if character == '>' {
                in_tag = false;
            }
            continue;
        }
        if character.is_whitespace() {
            if !result.is_empty() && !whitespace && chars < limit {
                result.push(' ');
                chars += 1;
            }
            whitespace = true;
            continue;
        }
        if chars == limit {
            break;
        }
        result.push(character);
        chars += 1;
        whitespace = false;
    }
    let value = result.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn doc_to_result(doc: SearchDoc) -> Option<MetadataResult> {
    let provider_key = doc.key?;
    let title = doc.title?.trim().to_string();
    if provider_key.is_empty() || title.is_empty() {
        return None;
    }

    let mut isbn13 = None;
    let mut isbn10 = None;
    for value in &doc.isbn {
        if let Some(isbn) = parse_isbn(value) {
            match isbn.len() {
                13 if isbn13.is_none() => isbn13 = Some(isbn),
                10 if isbn10.is_none() => isbn10 = Some(isbn),
                _ => {}
            }
        }
    }
    if isbn13.is_none()
        && let Some(isbn10) = &isbn10
    {
        isbn13 = isbn10_to_isbn13(isbn10);
    }

    // The complete language set matters for ranking; taking only the first
    // turned a [pl, en, sv] work into "Polish only".
    let mut languages: Vec<String> = Vec::new();
    for code in &doc.language {
        if let Some(language) = language_from_code(code)
            && !languages.contains(&language)
        {
            languages.push(language);
        }
    }
    let popularity = match (
        doc.want_to_read_count,
        doc.currently_reading_count,
        doc.already_read_count,
    ) {
        (None, None, None) => None,
        (want, reading, read) => Some(want.unwrap_or(0) + reading.unwrap_or(0) + read.unwrap_or(0)),
    };

    Some(MetadataResult {
        provider: "openlibrary".to_string(),
        provider_key,
        edition_key: doc.edition_key.first().cloned(),
        title,
        authors: doc
            .author_name
            .into_iter()
            .map(|author| author.trim().to_string())
            .filter(|author| !author.is_empty())
            .collect(),
        subjects: clean_subjects(doc.subject.into_iter()),
        year: doc.first_publish_year,
        language: languages.first().cloned(),
        languages,
        isbn10,
        isbn13,
        series: None,
        series_number: None,
        description: None,
        publisher: None,
        cover_id: doc.cover_i.map(|cover| cover.to_string()),
        rating_average: doc.ratings_average,
        rating_count: doc.ratings_count,
        edition_count: doc.edition_count,
        popularity,
    })
}

fn clean_subjects(subjects: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut cleaned = Vec::new();
    for subject in subjects {
        let subject = subject.trim().to_string();
        if subject.is_empty() {
            continue;
        }
        let key = subject.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        cleaned.push(subject);
        if cleaned.len() >= 40 {
            break;
        }
    }
    cleaned
}

fn trimmed(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn work_path(provider_key: &str) -> String {
    let key = provider_key.trim();
    if key.contains('/') {
        if key.starts_with('/') {
            key.to_string()
        } else {
            format!("/{key}")
        }
    } else {
        format!("/works/{key}")
    }
}

fn description_text(value: Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text),
        Value::Object(object) => object
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn year_from_date(value: &str) -> Option<i32> {
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() != 4 {
        return None;
    }
    digits.parse().ok()
}

fn language_from_key(key: &str) -> Option<String> {
    key.rsplit('/').next().and_then(language_from_code)
}

fn language_from_code(code: &str) -> Option<String> {
    if !code.is_ascii() {
        return None;
    }
    let lowered = code.to_ascii_lowercase();
    let mapped = match lowered.as_str() {
        "eng" => "en",
        "swe" => "sv",
        "ger" | "deu" => "de",
        "fre" | "fra" => "fr",
        "spa" => "es",
        "ita" => "it",
        "por" => "pt",
        "dut" | "nld" => "nl",
        "dan" => "da",
        "nor" => "no",
        "fin" => "fi",
        "pol" => "pl",
        "rus" => "ru",
        "jpn" => "ja",
        "chi" | "zho" => "zh",
        other if other.len() == 2 => other,
        other if other.len() > 2 => &other[..2],
        _ => return None,
    };
    Some(mapped.to_string())
}

fn encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        let character = *byte as char;
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '~') {
            encoded.push(character);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_works_path() {
        assert_eq!(work_path("OL123W"), "/works/OL123W");
        assert_eq!(work_path("/works/OL123W"), "/works/OL123W");
        assert_eq!(work_path("works/OL123W"), "/works/OL123W");
    }

    #[test]
    fn maps_language_codes() {
        assert_eq!(language_from_code("eng"), Some("en".to_string()));
        assert_eq!(language_from_key("/languages/swe"), Some("sv".to_string()));
        assert_eq!(language_from_code("xx"), Some("xx".to_string()));
        assert_eq!(language_from_code("aé"), None);
    }

    #[test]
    fn extracts_year() {
        assert_eq!(year_from_date("2021-05-04"), Some(2021));
        assert_eq!(year_from_date("May 2021"), None);
    }
}

#[cfg(test)]
mod subject_shape_tests {
    use super::*;

    #[test]
    fn parses_both_subject_shapes() {
        let work: WorkDoc = serde_json::from_str(
            r#"{"title":"Mixed","subjects":["Mystery and detective stories",{"name":"Dune"}]}"#,
        )
        .unwrap();
        let names: Vec<String> = work
            .subjects
            .into_iter()
            .filter_map(SubjectDoc::into_name)
            .collect();
        assert_eq!(names, vec!["Mystery and detective stories", "Dune"]);
    }
}
