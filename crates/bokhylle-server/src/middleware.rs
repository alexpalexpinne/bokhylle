use axum::extract::Request;
use axum::http::{HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::Response;
use tracing::Instrument;
use uuid::Uuid;

use crate::error::AppError;

pub const REQUEST_ID_HEADER: &str = "x-request-id";

pub async fn request_id(mut request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let header = HeaderValue::from_str(&request_id).expect("uuid string is a valid header value");

    request
        .headers_mut()
        .insert(REQUEST_ID_HEADER, header.clone());

    let span = tracing::info_span!("request", request_id = %request_id);
    let mut response = next.run(request).instrument(span).await;

    response.headers_mut().insert(REQUEST_ID_HEADER, header);
    response
}

pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data: blob:; font-src 'self' data: blob:; \
             style-src 'self' 'unsafe-inline' blob:; script-src 'self' 'wasm-unsafe-eval'; \
             frame-src 'self' blob:; object-src 'none'; frame-ancestors 'none'; \
             base-uri 'self'; form-action 'self'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );

    response
}

pub async fn cache_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let mut response = next.run(request).await;

    if response.status().is_success() && !path.starts_with("/api/") {
        let value = if path.starts_with("/assets/") {
            HeaderValue::from_static("public, max-age=31536000, immutable")
        } else {
            HeaderValue::from_static("no-cache")
        };
        response.headers_mut().insert(header::CACHE_CONTROL, value);
    }

    response
}

pub async fn origin_guard(request: Request, next: Next) -> Result<Response, AppError> {
    let is_mutation = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );

    if is_mutation
        && let Some(origin) = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
    {
        let host = request
            .headers()
            .get(header::HOST)
            .and_then(|value| value.to_str().ok());

        let origin_host = origin.split_once("://").map_or(origin, |(_, rest)| rest);
        let origin_host = origin_host.split('/').next().unwrap_or(origin_host);
        let same_origin = host.is_some_and(|host| host.eq_ignore_ascii_case(origin_host));

        if !same_origin {
            tracing::warn!(%origin, "rejected cross-origin state-changing request");
            return Err(AppError::Forbidden);
        }
    }

    Ok(next.run(request).await)
}

/// Children may browse their shelf and, when allowed, public catalogue
/// metadata. Adult discovery, acquisition, delivery and household browsing
/// remain refused.
pub async fn child_guard(
    state: &crate::AppState,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let path = request.uri().path().to_string();
    let reader_route = match path.split('/').collect::<Vec<_>>().as_slice() {
        ["", "api", "books", book_id, "files", file_id, "content"] => {
            book_id.parse::<i64>().is_ok()
                && file_id.parse::<i64>().is_ok()
                && request.method() == Method::GET
        }
        ["", "api", "books", book_id, "files", file_id, "position"] => {
            book_id.parse::<i64>().is_ok()
                && file_id.parse::<i64>().is_ok()
                && matches!(*request.method(), Method::GET | Method::PUT)
        }
        ["", "api", "books", book_id, "files", file_id, "direction"] => {
            book_id.parse::<i64>().is_ok()
                && file_id.parse::<i64>().is_ok()
                && request.method() == Method::PUT
        }
        ["", "api", "books", book_id, "files", file_id, "pages"] => {
            book_id.parse::<i64>().is_ok()
                && file_id.parse::<i64>().is_ok()
                && request.method() == Method::GET
        }
        ["", "api", "books", book_id, "files", file_id, "pages", page] => {
            book_id.parse::<i64>().is_ok()
                && file_id.parse::<i64>().is_ok()
                && page.parse::<usize>().is_ok()
                && request.method() == Method::GET
        }
        _ => false,
    };
    let blocked = path.starts_with("/api/discover")
        || path.starts_with("/api/acquisitions")
        || path.starts_with("/api/delivery-targets")
        || path.starts_with("/api/deliveries")
        || path.starts_with("/api/collections")
        || path.starts_with("/api/authors")
        || path.starts_with("/api/home/updates")
        || path.starts_with("/api/household")
        || path == "/api/activity/direct"
        || path == "/api/profile"
        || path == "/api/books/facets"
        || path == "/api/books/sharing"
        || (path.starts_with("/api/books/")
            && ((path.contains("/files/") && !reader_route)
                || path.ends_with("/related")
                || path.ends_with("/deliver")
                // Children are curated by a parent: no self-shelf mutations.
                || path.ends_with("/shelf")
                || path.ends_with("/sharing")
                || path.ends_with("/shelf/claim-all")));
    if !blocked {
        return Ok(next.run(request).await);
    }

    // Read the token before awaiting so no borrow of the request body is held
    // across an await, which would make this future non-Send.
    let token = crate::auth::session_cookie_token(request.headers()).map(|token| token.to_string());
    let Some(token) = token else {
        return Ok(next.run(request).await);
    };
    let user = match state.auth.user_for_token(&token).await? {
        Some(user) => user,
        None => return Ok(next.run(request).await),
    };
    // Authorization boundaries fail closed: a lookup error is not "adult".
    if crate::auth::profile_type(&state.db, user.id).await? == "child" {
        if path.starts_with("/api/discover/cover/")
            && crate::services::requests::may_browse_catalogue(state, user.id).await?
        {
            return Ok(next.run(request).await);
        }
        return Err(AppError::Forbidden);
    }

    Ok(next.run(request).await)
}
