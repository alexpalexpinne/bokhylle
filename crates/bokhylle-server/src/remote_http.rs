//! Bounded requests for catalogue feeds and publication files. Every hop is
//! resolved and pinned before connecting, including redirects.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use reqwest::{Client, Url, header};

use crate::error::AppError;

const MAX_REDIRECTS: usize = 5;

pub fn parse_url(raw: &str) -> Result<Url, AppError> {
    let mut url =
        Url::parse(raw.trim()).map_err(|_| AppError::BadRequest("invalid HTTP URL".to_string()))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::BadRequest(
            "only HTTP(S) URLs without embedded credentials are supported".to_string(),
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

pub fn origin(raw: &str) -> Result<String, AppError> {
    Ok(parse_url(raw)?.origin().ascii_serialization())
}

pub async fn response(
    raw: &str,
    trusted_origin: Option<&str>,
    accept: Option<&'static str>,
    max_bytes: u64,
) -> Result<reqwest::Response, AppError> {
    let mut url = parse_url(raw)?;
    for hop in 0..=MAX_REDIRECTS {
        let trusted =
            trusted_origin.is_some_and(|origin| origin == url.origin().ascii_serialization());
        let client = client_for(&url, trusted).await?;
        let mut request = client.get(url.clone());
        if let Some(accept) = accept {
            request = request.header(header::ACCEPT, accept);
        }
        let reply = request.send().await.map_err(|_| {
            AppError::Unavailable("the remote source could not be reached".to_string())
        })?;
        if reply.status().is_redirection() {
            if hop == MAX_REDIRECTS {
                return Err(AppError::Unprocessable("too many redirects".to_string()));
            }
            let location = reply
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| AppError::Unprocessable("invalid redirect".to_string()))?;
            url = url
                .join(location)
                .map_err(|_| AppError::Unprocessable("invalid redirect".to_string()))?;
            url = parse_url(url.as_str())?;
            // A new connection is made only after the next hop is validated.
            continue;
        }
        if !reply.status().is_success() {
            return Err(AppError::Unprocessable(format!(
                "remote source returned HTTP {}",
                reply.status().as_u16()
            )));
        }
        if reply.content_length().is_some_and(|size| size > max_bytes) {
            return Err(AppError::Unprocessable(
                "remote content exceeds the size limit".to_string(),
            ));
        }
        return Ok(reply);
    }
    Err(AppError::Unprocessable("too many redirects".to_string()))
}

pub async fn bytes(
    raw: &str,
    trusted_origin: Option<&str>,
    accept: Option<&'static str>,
    max_bytes: u64,
) -> Result<Vec<u8>, AppError> {
    Ok(bytes_with_url(raw, trusted_origin, accept, max_bytes)
        .await?
        .0)
}

pub async fn bytes_with_url(
    raw: &str,
    trusted_origin: Option<&str>,
    accept: Option<&'static str>,
    max_bytes: u64,
) -> Result<(Vec<u8>, Url), AppError> {
    let mut response = response(raw, trusted_origin, accept, max_bytes).await?;
    let final_url = response.url().clone();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AppError::Unavailable("remote content could not be read".to_string()))?
    {
        if bytes.len() as u64 + chunk.len() as u64 > max_bytes {
            return Err(AppError::Unprocessable(
                "remote content exceeds the size limit".to_string(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, final_url))
}

async fn client_for(url: &Url, trusted: bool) -> Result<Client, AppError> {
    let host = url
        .host_str()
        .ok_or_else(|| AppError::BadRequest("HTTP URL has no host".to_string()))?;
    let host = host.trim_matches(['[', ']']);
    let port = url.port_or_known_default().unwrap_or(80);
    let addresses: Vec<SocketAddr> = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| AppError::Unavailable("remote host resolution timed out".to_string()))?
    .map_err(|_| AppError::Unavailable("remote host could not be resolved".to_string()))?
    .collect();
    if addresses.is_empty()
        || (!trusted && addresses.iter().any(|address| !public_ip(address.ip())))
    {
        return Err(AppError::Unprocessable(
            "remote address is not allowed".to_string(),
        ));
    }
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15 * 60))
        .resolve(host, addresses[0])
        .build()
        .map_err(|_| AppError::Unavailable("HTTP client could not start".to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_internal_and_embedded_credentials() {
        assert!(!public_ip("127.0.0.1".parse().unwrap()));
        assert!(!public_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!public_ip("100.64.0.1".parse().unwrap()));
        assert!(!public_ip("192.0.0.8".parse().unwrap()));
        assert!(parse_url("https://user:pass@example.org/book.epub").is_err());
        assert!(parse_url("file:///tmp/book.epub").is_err());
    }
}
