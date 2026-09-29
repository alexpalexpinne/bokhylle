use std::sync::Arc;

use crate::provider::{DownloadError, DownloadProvider, DownloadSource};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::response::{BodyReadError, read_limited};

pub const DEFAULT_CATEGORY: &str = "books-app";
pub const TAG_PREFIX: &str = "books-acquisition-";
pub const MAX_API_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TEXT_RESPONSE_BYTES: u64 = 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum QbittorrentError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("qBittorrent returned status {0}")]
    Status(u16),
    #[error("qBittorrent authentication failed")]
    Auth,
    #[error("torrent is not owned by this application")]
    NotOwned,
    #[error("invalid qBittorrent response: {0}")]
    Invalid(String),
}

pub enum QbittorrentAuth {
    ApiKey(String),
    Credentials { username: String, password: String },
}

#[derive(Clone)]
pub struct QbittorrentClient {
    http: reqwest::Client,
    base_url: String,
    auth: Arc<QbittorrentAuth>,
    session: Arc<RwLock<Option<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TorrentInfo {
    pub hash: String,
    pub name: String,
    pub state: String,
    pub progress: f64,
    pub size: i64,
    pub downloaded: i64,
    #[serde(rename = "save_path")]
    pub save_path: String,
    #[serde(rename = "content_path")]
    pub content_path: Option<String>,
    pub category: Option<String>,
    pub tags: Option<String>,
    pub eta: Option<i64>,
    #[serde(rename = "dlspeed")]
    pub dl_speed: Option<i64>,
}

impl TorrentInfo {
    pub fn tag_list(&self) -> Vec<&str> {
        self.tags
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .collect()
    }

    pub fn is_owned(&self, category: &str) -> bool {
        self.category.as_deref() == Some(category)
            || self
                .tag_list()
                .iter()
                .any(|tag| tag.starts_with(TAG_PREFIX))
    }
}

impl QbittorrentClient {
    pub fn new(base_url: &str, auth: QbittorrentAuth) -> Result<Self, QbittorrentError> {
        let http = reqwest::Client::builder()
            .user_agent(format!(
                "Bokhylle/{} (self-hosted book server)",
                bokhylle_core::VERSION
            ))
            .timeout(std::time::Duration::from_secs(20))
            .build()?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth: Arc::new(auth),
            session: Arc::new(RwLock::new(None)),
        })
    }

    async fn login(&self) -> Result<String, QbittorrentError> {
        let QbittorrentAuth::Credentials { username, password } = self.auth.as_ref() else {
            return Err(QbittorrentError::Auth);
        };

        let response = self
            .http
            .post(format!("{}/api/v2/auth/login", self.base_url))
            .form(&[("username", username), ("password", password)])
            .send()
            .await?;

        if response.status().as_u16() == 403 {
            return Err(QbittorrentError::Auth);
        }
        if !response.status().is_success() {
            return Err(QbittorrentError::Status(response.status().as_u16()));
        }

        let sid = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(extract_sid);

        let body = limited_text(response).await?;
        if body.trim() != "Ok." {
            return Err(QbittorrentError::Auth);
        }

        let sid = sid.ok_or(QbittorrentError::Auth)?;
        *self.session.write().await = Some(sid.clone());
        Ok(sid)
    }

    async fn send<F>(
        &self,
        method: reqwest::Method,
        path: &str,
        configure: F,
    ) -> Result<reqwest::Response, QbittorrentError>
    where
        F: Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    {
        let url = format!("{}{path}", self.base_url);

        for attempt in 0..2 {
            let mut builder = self.http.request(method.clone(), &url);

            match self.auth.as_ref() {
                QbittorrentAuth::ApiKey(key) => {
                    builder = builder.header("Authorization", format!("Bearer {key}"));
                }
                QbittorrentAuth::Credentials { .. } => {
                    let cached = self.session.read().await.clone();
                    let sid = match cached {
                        Some(sid) => sid,
                        None => self.login().await?,
                    };
                    builder = builder.header("Cookie", format!("SID={sid}"));
                }
            }

            let response = configure(builder).send().await?;

            if response.status().as_u16() == 403
                && attempt == 0
                && matches!(self.auth.as_ref(), QbittorrentAuth::Credentials { .. })
            {
                *self.session.write().await = None;
                continue;
            }

            return Ok(response);
        }

        Err(QbittorrentError::Auth)
    }

    pub async fn test_connection(&self) -> Result<String, QbittorrentError> {
        let response = self
            .send(reqwest::Method::GET, "/api/v2/app/version", |builder| {
                builder
            })
            .await?;

        if !response.status().is_success() {
            return Err(QbittorrentError::Status(response.status().as_u16()));
        }

        Ok(limited_text(response).await?.trim().to_string())
    }

    pub async fn add_magnet(
        &self,
        magnet: &str,
        category: &str,
        tag: &str,
    ) -> Result<(), QbittorrentError> {
        let fields = [
            ("urls", magnet.to_string()),
            ("category", category.to_string()),
            ("tags", tag.to_string()),
        ];

        let response = self
            .send(reqwest::Method::POST, "/api/v2/torrents/add", |builder| {
                builder.form(&fields)
            })
            .await?;

        ensure_success(&response, "add magnet")?;
        ensure_accepted(response).await
    }

    pub async fn add_torrent(
        &self,
        bytes: Arc<Vec<u8>>,
        filename: &str,
        category: &str,
        tag: &str,
    ) -> Result<(), QbittorrentError> {
        let filename = filename.to_string();
        let category = category.to_string();
        let tag = tag.to_string();

        let response = self
            .send(
                reqwest::Method::POST,
                "/api/v2/torrents/add",
                move |builder| {
                    let part = reqwest::multipart::Part::bytes((*bytes).clone())
                        .file_name(filename.clone());
                    let form = reqwest::multipart::Form::new()
                        .part("torrents", part)
                        .text("category", category.clone())
                        .text("tags", tag.clone());
                    builder.multipart(form)
                },
            )
            .await?;

        ensure_success(&response, "add torrent")?;
        ensure_accepted(response).await
    }

    pub async fn torrent_info(&self, hash: &str) -> Result<Option<TorrentInfo>, QbittorrentError> {
        let path = format!("/api/v2/torrents/info?hashes={hash}");
        let response = self
            .send(reqwest::Method::GET, &path, |builder| builder)
            .await?;

        if !response.status().is_success() {
            return Err(QbittorrentError::Status(response.status().as_u16()));
        }

        let torrents: Vec<TorrentInfo> = limited_json(response).await?;

        Ok(torrents.into_iter().next())
    }

    pub async fn torrents_with_tag(&self, tag: &str) -> Result<Vec<TorrentInfo>, QbittorrentError> {
        let path = format!("/api/v2/torrents/info?tag={tag}");
        let response = self
            .send(reqwest::Method::GET, &path, |builder| builder)
            .await?;

        if !response.status().is_success() {
            return Err(QbittorrentError::Status(response.status().as_u16()));
        }

        limited_json(response).await
    }

    pub async fn torrents_in_category(
        &self,
        category: &str,
    ) -> Result<Vec<TorrentInfo>, QbittorrentError> {
        let path = format!("/api/v2/torrents/info?category={category}");
        let response = self
            .send(reqwest::Method::GET, &path, |builder| builder)
            .await?;

        if !response.status().is_success() {
            return Err(QbittorrentError::Status(response.status().as_u16()));
        }

        limited_json(response).await
    }

    pub async fn cancel_owned(
        &self,
        hash: &str,
        expected_category: &str,
        delete_files: bool,
    ) -> Result<bool, QbittorrentError> {
        let Some(info) = self.torrent_info(hash).await? else {
            return Ok(false);
        };

        if !info.is_owned(expected_category) {
            return Err(QbittorrentError::NotOwned);
        }

        self.delete(hash, delete_files).await?;
        Ok(true)
    }

    pub async fn delete(&self, hash: &str, delete_files: bool) -> Result<(), QbittorrentError> {
        let fields = [
            ("hashes", hash.to_string()),
            (
                "deleteFiles",
                if delete_files { "true" } else { "false" }.to_string(),
            ),
        ];

        let response = self
            .send(
                reqwest::Method::POST,
                "/api/v2/torrents/delete",
                |builder| builder.form(&fields),
            )
            .await?;

        ensure_success(&response, "delete torrent")
    }
}

fn ensure_success(response: &reqwest::Response, _action: &str) -> Result<(), QbittorrentError> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(QbittorrentError::Status(response.status().as_u16()))
    }
}

async fn ensure_accepted(response: reqwest::Response) -> Result<(), QbittorrentError> {
    let body = limited_text(response).await?;

    if body.trim().eq_ignore_ascii_case("fails.") {
        return Err(QbittorrentError::Invalid(
            "qBittorrent rejected the torrent".to_string(),
        ));
    }

    Ok(())
}

async fn limited_text(response: reqwest::Response) -> Result<String, QbittorrentError> {
    let bytes = read_limited(response, MAX_TEXT_RESPONSE_BYTES)
        .await
        .map_err(map_body_error)?;
    String::from_utf8(bytes).map_err(|error| QbittorrentError::Invalid(error.to_string()))
}

async fn limited_json<T: DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, QbittorrentError> {
    let bytes = read_limited(response, MAX_API_RESPONSE_BYTES)
        .await
        .map_err(map_body_error)?;
    serde_json::from_slice(&bytes).map_err(|error| QbittorrentError::Invalid(error.to_string()))
}

fn map_body_error(error: BodyReadError) -> QbittorrentError {
    match error {
        BodyReadError::Request(error) => QbittorrentError::Request(error),
        BodyReadError::TooLarge => {
            QbittorrentError::Invalid("the response exceeds the size limit".to_string())
        }
    }
}

fn extract_sid(set_cookie: &str) -> Option<String> {
    let (name, rest) = set_cookie.split_once('=')?;
    if name.trim() != "SID" {
        return None;
    }
    Some(rest.split(';').next()?.trim().to_string())
}

#[async_trait::async_trait]
impl DownloadProvider for QbittorrentClient {
    fn name(&self) -> &'static str {
        "qbittorrent"
    }

    async fn add(
        &self,
        source: DownloadSource,
        category: &str,
        tag: &str,
    ) -> Result<(), DownloadError> {
        match source {
            DownloadSource::Magnet(magnet) => {
                self.add_magnet(&magnet, category, tag).await?;
            }
            DownloadSource::TorrentFile { bytes, filename } => {
                self.add_torrent(bytes, &filename, category, tag).await?;
            }
        }
        Ok(())
    }

    async fn status(&self, id: &str) -> Result<Option<TorrentInfo>, DownloadError> {
        Ok(self.torrent_info(id).await?)
    }

    async fn find_by_tag(&self, tag: &str) -> Result<Vec<TorrentInfo>, DownloadError> {
        Ok(self.torrents_with_tag(tag).await?)
    }

    async fn list_category(&self, category: &str) -> Result<Vec<TorrentInfo>, DownloadError> {
        Ok(self.torrents_in_category(category).await?)
    }

    async fn cancel_owned(
        &self,
        id: &str,
        category: &str,
        delete_files: bool,
    ) -> Result<bool, DownloadError> {
        Ok(QbittorrentClient::cancel_owned(self, id, category, delete_files).await?)
    }

    async fn test_connection(&self) -> Result<String, DownloadError> {
        Ok(QbittorrentClient::test_connection(self).await?)
    }
}
