use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};

use base64::Engine;
use serde::Deserialize;

use crate::AppState;
use crate::auth::User;
use crate::error::AppError;
use crate::library::queries::{self, BookFilters, BookSummary};

/// An OPDS request authenticated with a reader token as the Basic-auth
/// password (the username is ignored). OPDS readers like KOReader only
/// support Basic auth, so this is their door into the library.
pub struct OpdsUser(pub User);

impl FromRequestParts<AppState> for OpdsUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let Some(header) = parts.headers.get(header::AUTHORIZATION) else {
            return Err(AppError::Unauthorized);
        };
        let value = header.to_str().unwrap_or_default();
        // HTTP auth scheme names are case-insensitive.
        let Some((scheme, encoded)) = value.split_once(' ') else {
            return Err(AppError::Unauthorized);
        };
        if !scheme.eq_ignore_ascii_case("basic") {
            return Err(AppError::Unauthorized);
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|_| AppError::Unauthorized)?;
        let decoded = String::from_utf8(decoded).map_err(|_| AppError::Unauthorized)?;
        let (_, token) = decoded.split_once(':').ok_or(AppError::Unauthorized)?;

        let user = crate::reader_tokens::authenticate(&state.db, token)
            .await?
            .ok_or(AppError::Unauthorized)?;
        Ok(OpdsUser(user))
    }
}

fn rfc3339(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let (hour, minute, second) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let mut month = if mp < 10 { mp + 3 } else { mp - 9 };
    if month <= 2 {
        year += 1;
    }
    if month <= 0 {
        month += 12;
    }
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn now_rfc3339() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default();
    rfc3339(now)
}

fn content_type_for(format: &str) -> &'static str {
    match format {
        "pdf" => "application/pdf",
        "cbz" => "application/vnd.comicbook+zip",
        _ => "application/epub+zip",
    }
}

/// OPDS readers expect a Basic challenge on 401; the SPA API must not send
/// one, or browsers hold unauthenticated fetches waiting for credentials.
pub async fn challenge(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let mut response = next.run(request).await;
    if response.status() == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"Bokhylle OPDS\", charset=\"UTF-8\""),
        );
    }
    response
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn feed(title: &str, id: &str, entries: String, navigation: bool) -> Response {
    let updated = now_rfc3339();
    let kind = if navigation {
        "navigation"
    } else {
        "acquisition"
    };
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog">
  <id>{id}</id>
  <title>{title}</title>
  <updated>{updated}</updated>
  <author><name>Bokhylle</name></author>
  {entries}
</feed>"#,
        id = escape(id),
        title = escape(title),
        updated = updated,
    );
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static(match kind {
                "navigation" => "application/atom+xml;profile=opds-catalog;kind=navigation",
                _ => "application/atom+xml;profile=opds-catalog;kind=acquisition",
            }),
        )],
        body,
    )
        .into_response()
}

fn navigation_entry(title: &str, href: &str) -> String {
    format!(
        r#"<entry>
    <title>{title}</title>
    <id>{href}</id>
    <updated>{updated}</updated>
    <link rel="subsection" type="application/atom+xml;profile=opds-catalog;kind=acquisition" href="{href}"/>
  </entry>"#,
        title = escape(title),
        href = escape(href),
        updated = now_rfc3339(),
    )
}

fn acquisition_entry(book: &BookSummary, author: &str, format: &str) -> String {
    format!(
        r#"<entry>
    <title>{title}</title>
    <id>urn:bokhylle:book:{id}</id>
    <updated>{updated}</updated>
    <author><name>{author}</name></author>
    <link rel="http://opds-spec.org/acquisition/open-access" type="{content_type}" href="/opds/books/{id}/download"/>
  </entry>"#,
        title = escape(&book.title),
        id = book.id,
        author = escape(author),
        updated = rfc3339(book.added_at),
        content_type = content_type_for(format),
    )
}

pub async fn root(
    OpdsUser(user): OpdsUser,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let entries = if child {
        // Children only ever see their shelf; do not advertise /opds/all.
        format!(
            "{}{}{}",
            navigation_entry("My shelf", "/opds/shelf"),
            navigation_entry("Recently Added", "/opds/recent"),
            navigation_entry("Authors", "/opds/authors"),
        )
    } else {
        format!(
            "{}{}{}{}",
            navigation_entry("All books", "/opds/all"),
            navigation_entry("My shelf", "/opds/shelf"),
            navigation_entry("Recently Added", "/opds/recent"),
            navigation_entry("Authors", "/opds/authors"),
        )
    };
    Ok(feed("Bokhylle", "urn:bokhylle:catalog", entries, true))
}

#[derive(Debug, Deserialize)]
pub struct FeedPage {
    #[serde(default = "first_page")]
    page: u32,
}

fn first_page() -> u32 {
    1
}

pub async fn all(
    OpdsUser(user): OpdsUser,
    State(state): State<AppState>,
    Query(page): Query<FeedPage>,
) -> Result<Response, AppError> {
    if crate::auth::profile_type(&state.db, user.id).await? == "child" {
        return Err(AppError::Forbidden);
    }
    books_feed(
        &state,
        None,
        "/opds/all",
        page.page.max(1),
        "title",
        "All books",
    )
    .await
}

pub async fn shelf(
    OpdsUser(user): OpdsUser,
    State(state): State<AppState>,
    Query(page): Query<FeedPage>,
) -> Result<Response, AppError> {
    books_feed(
        &state,
        Some(user.id),
        "/opds/shelf",
        page.page.max(1),
        "title",
        "My shelf",
    )
    .await
}

pub async fn recent(
    OpdsUser(user): OpdsUser,
    State(state): State<AppState>,
    Query(page): Query<FeedPage>,
) -> Result<Response, AppError> {
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let mine = if child { Some(user.id) } else { None };
    books_feed(
        &state,
        mine,
        "/opds/recent",
        page.page.max(1),
        "recent",
        "Recently Added",
    )
    .await
}

async fn books_feed(
    state: &AppState,
    mine: Option<i64>,
    base: &str,
    page_number: u32,
    sort: &str,
    title: &str,
) -> Result<Response, AppError> {
    let listing = queries::list_books(
        &state.db,
        sort,
        page_number as i64,
        100,
        &BookFilters {
            mine,
            kind: None,
            format: None,
            language: None,
            series: None,
            subject: None,
            collection: None,
            letter: None,
            missing: None,
        },
    )
    .await?;
    let formats = primary_formats(state).await?;
    let mut entries = book_entries(&listing.items, &formats);
    if listing.items.len() == 100 {
        entries.push_str(&navigation_entry(
            "Next page",
            &format!("{base}?page={}", page_number + 1),
        ));
    }
    Ok(feed(title, "urn:bokhylle:catalog:books", entries, false))
}

async fn primary_formats(
    state: &AppState,
) -> Result<std::collections::HashMap<i64, String>, AppError> {
    let formats: Vec<(i64, String)> = sqlx::query_as(
        "SELECT e.book_id, f.format
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         ORDER BY e.book_id, CASE f.format WHEN 'epub' THEN 0 ELSE 1 END, f.id",
    )
    .fetch_all(&state.db)
    .await?;
    let mut primary: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    for (book_id, format) in formats {
        primary.entry(book_id).or_insert(format);
    }
    Ok(primary)
}

fn book_entries(books: &[BookSummary], formats: &std::collections::HashMap<i64, String>) -> String {
    books
        .iter()
        .map(|book| {
            let format = formats.get(&book.id).map(String::as_str).unwrap_or("epub");
            acquisition_entry(book, &book.authors.join(", "), format)
        })
        .collect()
}

pub async fn authors(
    OpdsUser(user): OpdsUser,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let mine = if child { Some(user.id) } else { None };
    let authors = queries::list_authors(&state.db, mine, false, user.id).await?;
    let entries: String = authors
        .iter()
        .filter(|author| author.book_count > 0)
        .map(|author| {
            navigation_entry(
                &format!("{} ({})", author.name, author.book_count),
                &format!("/opds/authors/{}", author.id),
            )
        })
        .collect();
    Ok(feed(
        "Authors",
        "urn:bokhylle:catalog:authors",
        entries,
        true,
    ))
}

pub async fn author_feed(
    Path(id): Path<i64>,
    OpdsUser(user): OpdsUser,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let mine = if child { Some(user.id) } else { None };
    let Some(author) = queries::get_author(&state.db, id, mine).await? else {
        return Err(AppError::NotFound("author not found".to_string()));
    };
    if author.books.is_empty() {
        return Err(AppError::NotFound("author not found".to_string()));
    }
    let formats = primary_formats(&state).await?;
    let entries = book_entries(&author.books, &formats);
    Ok(feed(
        &author.name,
        &format!("urn:bokhylle:catalog:author:{id}"),
        entries,
        false,
    ))
}

/// Filename the OPDS download serves. KOReader's filename checksum hashes
/// this name, so `routes::kosync` shares it to map those documents back.
pub fn download_filename(title: &str, format: &str) -> String {
    let safe: String = title
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}.{format}")
}

pub async fn download(
    Path(id): Path<i64>,
    OpdsUser(user): OpdsUser,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    if crate::auth::profile_type(&state.db, user.id).await? == "child"
        && !crate::user_books::contains(&state.db, user.id, id).await?
    {
        return Err(AppError::NotFound("book not found".to_string()));
    }

    let file: Option<(String, String, String)> = sqlx::query_as(
        "SELECT f.path, f.format, b.title
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         JOIN books b ON b.id = e.book_id
         WHERE e.book_id = ?
         ORDER BY CASE f.format WHEN 'epub' THEN 0 ELSE 1 END, f.id
         LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let Some((path, format, title)) = file else {
        return Err(AppError::NotFound("no file for this book".to_string()));
    };

    if !tokio::fs::try_exists(&path).await? {
        return Err(AppError::NotFound("the book file is missing".to_string()));
    }
    // Books can be tens of megabytes; never read them on a runtime worker.
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&path))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))??;
    let content_type = match format.as_str() {
        "pdf" => "application/pdf",
        "cbz" => "application/vnd.comicbook+zip",
        _ => "application/epub+zip",
    };
    let disposition = format!(
        "attachment; filename=\"{}\"",
        download_filename(&title, &format)
    );

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition)
                    .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
            ),
        ],
        bytes,
    )
        .into_response())
}
