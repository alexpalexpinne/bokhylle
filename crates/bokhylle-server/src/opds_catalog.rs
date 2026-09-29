//! OPDS 1.x (Atom) and OPDS 2 (JSON) normalized into one browsing model.
//! Only direct EPUB/PDF/CBZ acquisition links are offered for import.

use reqwest::Url;
use roxmltree::Node;
use serde::Serialize;
use serde_json::Value;
use sqlx::FromRow;

use crate::{error::AppError, remote_http};

const MAX_FEED_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 100;

#[derive(Debug, Clone, FromRow, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSource {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogLink {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileOption {
    pub index: usize,
    pub format: String,
    pub label: String,
    #[serde(skip)]
    #[schemars(skip)]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub files: Vec<FileOption>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFeed {
    pub title: String,
    pub page_url: String,
    pub navigation: Vec<CatalogLink>,
    pub entries: Vec<CatalogEntry>,
    pub next: Option<String>,
    pub search_available: bool,
    #[serde(skip)]
    #[schemars(skip)]
    search: Option<SearchTarget>,
}

#[derive(Debug, Clone)]
enum SearchTarget {
    Template(String),
    OpenSearch(String),
}

pub async fn fetch(
    source: &CatalogSource,
    page_url: Option<&str>,
    query: Option<&str>,
) -> Result<CatalogFeed, AppError> {
    let trusted = remote_http::origin(&source.url)?;
    let url = page_url.unwrap_or(&source.url);
    let mut feed = fetch_page(url, Some(&trusted)).await?;
    if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
        // Search is advertised by the catalogue; a feed without it stays
        // browsable and is never guessed into a vendor-specific URL.
        let target = feed.search.clone().ok_or_else(|| {
            AppError::Unprocessable("this catalogue does not offer search".to_string())
        })?;
        let template = match target {
            SearchTarget::Template(template) => template,
            SearchTarget::OpenSearch(descriptor) => {
                let (bytes, descriptor_url) = remote_http::bytes_with_url(
                    &descriptor,
                    Some(&trusted),
                    Some("application/opensearchdescription+xml"),
                    256 * 1024,
                )
                .await?;
                let template = open_search_template(&bytes)?;
                absolute_template(&descriptor_url, &template).ok_or_else(|| {
                    AppError::Unprocessable("invalid catalogue search URL".to_string())
                })?
            }
        };
        let search_url = expand_search(&template, query)?;
        feed = fetch_page(search_url.as_str(), Some(&trusted)).await?;
        feed.search_available = true;
    }
    Ok(feed)
}

async fn fetch_page(url: &str, trusted_origin: Option<&str>) -> Result<CatalogFeed, AppError> {
    let (bytes, final_url) = remote_http::bytes_with_url(
        url,
        trusted_origin,
        Some("application/opds+json, application/atom+xml;q=0.9"),
        MAX_FEED_BYTES,
    )
    .await?;
    let mut feed = if bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(b'{')
    {
        parse_json(&bytes, &final_url)?
    } else {
        parse_atom(&bytes, &final_url)?
    };
    feed.page_url = final_url.to_string();
    Ok(feed)
}

fn parse_atom(bytes: &[u8], base: &Url) -> Result<CatalogFeed, AppError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AppError::Unprocessable("invalid OPDS XML encoding".to_string()))?;
    let document = roxmltree::Document::parse(text)
        .map_err(|_| AppError::Unprocessable("invalid OPDS XML feed".to_string()))?;
    let root = document.root_element();
    if root.tag_name().name() != "feed" {
        return Err(AppError::Unprocessable("not an OPDS feed".to_string()));
    }
    let mut feed = empty_feed(child_text(root, "title").unwrap_or("Untitled catalogue"));
    for link in root.children().filter(|node| node.has_tag_name("link")) {
        let rel = link.attribute("rel").unwrap_or_default();
        let href = link.attribute("href").unwrap_or_default();
        if rel == "next" {
            feed.next = absolute(base, href);
        } else if rel == "search" {
            feed.search = search_target(base, href, link.attribute("type"));
        } else if rel == "subsection"
            && let Some(url) = absolute(base, href)
        {
            feed.navigation.push(CatalogLink {
                title: link.attribute("title").unwrap_or("Browse").to_string(),
                url,
            });
        }
    }
    for entry in root
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .take(MAX_ENTRIES)
    {
        let title = child_text(entry, "title").unwrap_or("Untitled").to_string();
        let id = child_text(entry, "id").unwrap_or(&title).to_string();
        let authors: Vec<String> = entry
            .children()
            .filter(|node| node.has_tag_name("author"))
            .filter_map(|node| child_text(node, "name"))
            .map(str::to_string)
            .collect();
        let authors = if authors.is_empty() {
            child_text(entry, "content")
                .map(|author| vec![author.to_string()])
                .unwrap_or_default()
        } else {
            authors
        };
        let language = child_text(entry, "language").map(str::to_string);
        let mut files = Vec::new();
        let mut navigation = None;
        for link in entry.children().filter(|node| node.has_tag_name("link")) {
            let rel = link.attribute("rel").unwrap_or_default();
            let href = link.attribute("href").unwrap_or_default();
            if rel == "subsection" {
                navigation = absolute(base, href);
            }
            if !direct_relation(rel) {
                continue;
            }
            if let Some(format) = supported_format(link.attribute("type").unwrap_or_default())
                && let Some(url) = absolute(base, href)
            {
                files.push(FileOption {
                    index: files.len(),
                    format: format.to_string(),
                    label: link.attribute("title").unwrap_or(format).to_string(),
                    url,
                });
            }
        }
        if files.is_empty() {
            if let Some(url) = navigation {
                feed.navigation.push(CatalogLink { title, url });
            }
        } else {
            feed.entries.push(CatalogEntry {
                id,
                title,
                authors,
                language,
                files,
            });
        }
    }
    feed.search_available = feed.search.is_some();
    Ok(feed)
}

fn parse_json(bytes: &[u8], base: &Url) -> Result<CatalogFeed, AppError> {
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|_| AppError::Unprocessable("invalid OPDS JSON feed".to_string()))?;
    let title = root
        .pointer("/metadata/title")
        .and_then(Value::as_str)
        .unwrap_or("Untitled catalogue");
    let mut feed = empty_feed(title);
    for link in values(&root, "links") {
        let rel = link_rel(link);
        if rel.contains(&"next") {
            feed.next = link
                .get("href")
                .and_then(Value::as_str)
                .and_then(|href| absolute(base, href));
        }
        if rel.contains(&"search") {
            feed.search = link
                .get("href")
                .and_then(Value::as_str)
                .and_then(|href| absolute_template(base, href))
                .map(SearchTarget::Template);
        }
    }
    for link in values(&root, "navigation") {
        if let (Some(title), Some(url)) = (
            link.get("title").and_then(Value::as_str),
            link.get("href")
                .and_then(Value::as_str)
                .and_then(|href| absolute(base, href)),
        ) {
            feed.navigation.push(CatalogLink {
                title: title.to_string(),
                url,
            });
        }
    }
    for publication in values(&root, "publications").take(MAX_ENTRIES) {
        let metadata = &publication["metadata"];
        let title = metadata
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Untitled");
        let id = metadata
            .get("identifier")
            .and_then(Value::as_str)
            .or_else(|| {
                publication
                    .get("links")
                    .and_then(Value::as_array)
                    .and_then(|links| links.first())
                    .and_then(|link| link.get("href"))
                    .and_then(Value::as_str)
            })
            .unwrap_or(title);
        let authors = match metadata.get("author") {
            Some(Value::String(author)) => vec![author.clone()],
            Some(Value::Array(authors)) => authors.iter().filter_map(author_name).collect(),
            Some(author) => author_name(author).into_iter().collect(),
            None => Vec::new(),
        };
        let language = match metadata.get("language") {
            Some(Value::String(language)) => Some(language.clone()),
            Some(Value::Array(languages)) => languages
                .first()
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        };
        let mut files = Vec::new();
        for link in values(publication, "links") {
            if !link_rel(link).iter().any(|rel| direct_relation(rel)) {
                continue;
            }
            if let Some(format) = link
                .get("type")
                .and_then(Value::as_str)
                .and_then(supported_format)
                && let Some(url) = link
                    .get("href")
                    .and_then(Value::as_str)
                    .and_then(|href| absolute(base, href))
            {
                files.push(FileOption {
                    index: files.len(),
                    format: format.to_string(),
                    label: link
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or(format)
                        .to_string(),
                    url,
                });
            }
        }
        if !files.is_empty() {
            feed.entries.push(CatalogEntry {
                id: id.to_string(),
                title: title.to_string(),
                authors,
                language,
                files,
            });
        }
    }
    feed.search_available = feed.search.is_some();
    Ok(feed)
}

fn empty_feed(title: &str) -> CatalogFeed {
    CatalogFeed {
        title: title.to_string(),
        page_url: String::new(),
        navigation: Vec::new(),
        entries: Vec::new(),
        next: None,
        search_available: false,
        search: None,
    }
}

fn values<'a>(object: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    object
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn link_rel(link: &Value) -> Vec<&str> {
    match link.get("rel") {
        Some(Value::String(rel)) => vec![rel],
        Some(Value::Array(relations)) => relations.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn author_name(author: &Value) -> Option<String> {
    author
        .as_str()
        .or_else(|| author.get("name").and_then(Value::as_str))
        .map(str::to_string)
}

fn child_text<'a>(node: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn absolute(base: &Url, href: &str) -> Option<String> {
    let url = base.join(href).ok()?;
    remote_http::parse_url(url.as_str())
        .ok()
        .map(|url| url.to_string())
}

fn absolute_template(base: &Url, href: &str) -> Option<String> {
    let (marked, token) = if let Some(start) = href.find('{') {
        let end = href[start..].find('}').map(|offset| start + offset)?;
        let token = &href[start..=end];
        (href.replace(token, "OPDS_TEMPLATE_SLOT"), Some(token))
    } else {
        (href.to_string(), None)
    };
    let resolved = absolute(base, &marked)?;
    Some(match token {
        Some(token) => resolved.replace("OPDS_TEMPLATE_SLOT", token),
        None => resolved,
    })
}

fn supported_format(mime: &str) -> Option<&'static str> {
    match mime.split(';').next()?.trim().to_ascii_lowercase().as_str() {
        "application/epub+zip" => Some("epub"),
        "application/pdf" => Some("pdf"),
        "application/vnd.comicbook+zip" | "application/x-cbz" => Some("cbz"),
        _ => None,
    }
}

fn direct_relation(rel: &str) -> bool {
    matches!(
        rel,
        "download"
            | "acquisition"
            | "http://opds-spec.org/acquisition"
            | "http://opds-spec.org/acquisition/open-access"
    )
}

fn search_target(base: &Url, href: &str, mime: Option<&str>) -> Option<SearchTarget> {
    let url = absolute(base, href)?;
    if mime.unwrap_or_default().contains("opensearchdescription") {
        Some(SearchTarget::OpenSearch(url))
    } else {
        Some(SearchTarget::Template(
            absolute_template(base, href).unwrap_or(url),
        ))
    }
}

fn open_search_template(bytes: &[u8]) -> Result<String, AppError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AppError::Unprocessable("invalid OpenSearch document".to_string()))?;
    let document = roxmltree::Document::parse(text)
        .map_err(|_| AppError::Unprocessable("invalid OpenSearch document".to_string()))?;
    document
        .descendants()
        .filter(|node| node.has_tag_name("Url"))
        .find(|node| {
            node.attribute("type")
                .unwrap_or_default()
                .contains("atom+xml")
        })
        .and_then(|node| node.attribute("template"))
        .map(str::to_string)
        .ok_or_else(|| AppError::Unprocessable("catalogue search has no OPDS feed".to_string()))
}

fn expand_search(template: &str, query: &str) -> Result<Url, AppError> {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let expanded = if template.contains("{searchTerms}") {
        template.replace("{searchTerms}", &encoded)
    } else if let Some(start) = template.find("{?") {
        let end = template[start..]
            .find('}')
            .map(|offset| start + offset)
            .ok_or_else(|| AppError::Unprocessable("unsupported search template".to_string()))?;
        let parameter = template[start + 2..end]
            .split(',')
            .next()
            .unwrap_or("query");
        format!(
            "{}?{}={}{}",
            &template[..start],
            parameter,
            encoded,
            &template[end + 1..]
        )
    } else {
        return Err(AppError::Unprocessable(
            "unsupported search template".to_string(),
        ));
    };
    remote_http::parse_url(&expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atom_navigation_and_downloads_are_distinct() {
        let xml = br#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Books</title>
        <entry><id>one</id><title>One</title><link rel="subsection" href="/one.opds"/></entry>
        <entry><id>two</id><title>Two</title><author><name>A Writer</name></author>
        <link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/two.epub"/>
        <link rel="http://opds-spec.org/acquisition/borrow" type="application/pdf" href="/borrow"/></entry></feed>"#;
        let feed = parse_atom(xml, &Url::parse("https://books.example/feed").unwrap()).unwrap();
        assert_eq!(feed.navigation.len(), 1);
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].files.len(), 1);
        assert_eq!(feed.entries[0].files[0].format, "epub");
        assert!(!serde_json::to_string(&feed).unwrap().contains("two.epub"));
    }

    #[test]
    fn json_feed_ignores_purchase_and_preview_links() {
        let json = br#"{"metadata":{"title":"Books"},"publications":[{"metadata":{"title":"One","identifier":"urn:one","author":[{"name":"A Writer"}]},"links":[{"rel":"download","type":"application/epub+zip","href":"/one.epub"},{"rel":"buy","type":"application/pdf","href":"/buy"},{"rel":"preview","type":"application/pdf","href":"/sample"}]}]}"#;
        let feed = parse_json(json, &Url::parse("https://books.example/feed").unwrap()).unwrap();
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].files.len(), 1);
        assert_eq!(feed.entries[0].authors, vec!["A Writer"]);
    }

    #[test]
    fn opds_two_search_template_expands_query() {
        let json = br#"{"metadata":{"title":"Books"},"links":[{"rel":"search","href":"/search{?query}","type":"application/opds+json","templated":true}]}"#;
        let feed = parse_json(json, &Url::parse("https://books.example/feed").unwrap()).unwrap();
        let SearchTarget::Template(template) = feed.search.unwrap() else {
            panic!("search template expected")
        };
        let url = expand_search(&template, "A & B").unwrap();
        assert_eq!(url.as_str(), "https://books.example/search?query=A+%26+B");
    }

    #[test]
    fn opensearch_relative_template_resolves_from_descriptor() {
        let descriptor = br#"<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
            <Url type="application/atom+xml;profile=opds-catalog" template="../search?q={searchTerms}"/>
        </OpenSearchDescription>"#;
        let template = open_search_template(descriptor).unwrap();
        let descriptor_url = Url::parse("https://books.example/catalogue/search.xml").unwrap();
        let resolved = absolute_template(&descriptor_url, &template).unwrap();
        assert_eq!(
            expand_search(&resolved, "A & B").unwrap().as_str(),
            "https://books.example/search?q=A+%26+B"
        );
    }
}
