//! MCP endpoint authentication. The bearer token resolves to one Bokhylle
//! profile with a read/write scope; the profile's own capabilities stay
//! authoritative in the services.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

use crate::AppState;
use crate::agent_tokens;

/// Validates Origin when a browser sends one (DNS-rebinding guard), then
/// resolves the bearer token to its profile.
pub async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !origin_allowed(&headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let token = bearer_token(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let principal = agent_tokens::authenticate(&state.db, token)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}

/// HTTP auth scheme names are case-insensitive.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

/// A browser Origin must match the Host it is talking to; non-browser MCP
/// clients send no Origin at all.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let origin_host = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin);
    origin_host.eq_ignore_ascii_case(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_must_match_the_request_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "bokhylle.test".parse().unwrap());
        assert!(origin_allowed(&headers), "no Origin is allowed");

        headers.insert(header::ORIGIN, "https://bokhylle.test".parse().unwrap());
        assert!(origin_allowed(&headers), "same host passes");

        headers.insert(header::ORIGIN, "https://evil.test".parse().unwrap());
        assert!(!origin_allowed(&headers), "another host is refused");
    }

    #[test]
    fn bearer_scheme_is_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("abc"));
        headers.insert(header::AUTHORIZATION, "BeArEr xyz".parse().unwrap());
        assert_eq!(bearer_token(&headers), Some("xyz"));
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
        headers.insert(header::AUTHORIZATION, "Bearer ".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
    }
}
