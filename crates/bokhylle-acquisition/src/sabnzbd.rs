//! SABnzbd API adapter. Its completed path remains untrusted until the server
//! verifies containment inside the configured downloads directory.

use std::time::Duration;

use async_trait::async_trait;
use bokhylle_core::VERSION;
use reqwest::{Client, Url};
use serde_json::Value;

use crate::response::read_limited;

#[derive(Debug, thiserror::Error)]
pub enum SabnzbdError {
    #[error("invalid SABnzbd configuration")]
    Configuration,
    #[error("SABnzbd could not be reached")]
    Request,
    #[error("SABnzbd returned HTTP {0}")]
    Status(u16),
    #[error("invalid SABnzbd response")]
    InvalidResponse,
    #[error("SABnzbd rejected the request")]
    Rejected,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum NzbStatus {
    Queued { progress: f64 },
    Completed { path: String },
    Failed,
    Missing,
}

#[async_trait]
pub trait NzbDownloadProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn add_url(
        &self,
        url: &str,
        category: &str,
        job_name: &str,
    ) -> Result<String, SabnzbdError>;
    async fn find_by_name(&self, job_name: &str) -> Result<Option<String>, SabnzbdError>;
    async fn status(&self, id: &str) -> Result<NzbStatus, SabnzbdError>;
    async fn cancel(&self, id: &str) -> Result<(), SabnzbdError>;
    async fn test_connection(&self) -> Result<String, SabnzbdError>;
}

pub struct SabnzbdClient {
    endpoint: Url,
    api_key: String,
    client: Client,
}

impl SabnzbdClient {
    pub fn new(endpoint: &str, api_key: &str) -> Result<Self, SabnzbdError> {
        let mut endpoint = Url::parse(endpoint.trim()).map_err(|_| SabnzbdError::Configuration)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint
                .query_pairs()
                .any(|(key, _)| matches!(key.as_ref(), "apikey" | "mode"))
        {
            return Err(SabnzbdError::Configuration);
        }
        endpoint.set_fragment(None);
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .user_agent(format!("Bokhylle/{VERSION}"))
            .build()
            .map_err(|_| SabnzbdError::Configuration)?;
        Ok(Self {
            endpoint,
            api_key: api_key.to_owned(),
            client,
        })
    }

    async fn call(&self, params: &[(&str, &str)]) -> Result<Value, SabnzbdError> {
        // Form parameters keep the SAB key and indexer URL out of request URLs.
        let mut form = vec![("apikey", self.api_key.as_str()), ("output", "json")];
        form.extend_from_slice(params);
        let response = self
            .client
            .post(self.endpoint.clone())
            .form(&form)
            .send()
            .await
            .map_err(|_| SabnzbdError::Request)?;
        if !response.status().is_success() {
            return Err(SabnzbdError::Status(response.status().as_u16()));
        }
        let bytes = read_limited(response, 2 * 1024 * 1024)
            .await
            .map_err(|_| SabnzbdError::InvalidResponse)?;
        let reply: Value =
            serde_json::from_slice(&bytes).map_err(|_| SabnzbdError::InvalidResponse)?;
        if reply.get("status").and_then(Value::as_bool) == Some(false) {
            return Err(SabnzbdError::Rejected);
        }
        Ok(reply)
    }
}

fn slots<'a>(reply: &'a Value, mode: &str) -> Result<&'a [Value], SabnzbdError> {
    reply
        .get(mode)
        .and_then(|value| value.get("slots"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or(SabnzbdError::InvalidResponse)
}

fn job<'a>(reply: &'a Value, mode: &str, id: &str) -> Result<Option<&'a Value>, SabnzbdError> {
    Ok(slots(reply, mode)?
        .iter()
        .find(|slot| slot.get("nzo_id").and_then(Value::as_str) == Some(id)))
}

fn accepted(reply: &Value) -> Result<(), SabnzbdError> {
    match reply.get("status").and_then(Value::as_bool) {
        Some(true) => Ok(()),
        Some(false) => Err(SabnzbdError::Rejected),
        None => Err(SabnzbdError::InvalidResponse),
    }
}

#[async_trait]
impl NzbDownloadProvider for SabnzbdClient {
    fn name(&self) -> &'static str {
        "sabnzbd"
    }

    async fn add_url(
        &self,
        url: &str,
        category: &str,
        job_name: &str,
    ) -> Result<String, SabnzbdError> {
        let reply = self
            .call(&[
                ("mode", "addurl"),
                ("name", url),
                ("cat", category),
                ("nzbname", job_name),
            ])
            .await?;
        accepted(&reply)?;
        reply
            .get("nzo_ids")
            .and_then(Value::as_array)
            .and_then(|ids| ids.first())
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or(SabnzbdError::InvalidResponse)
    }

    async fn find_by_name(&self, job_name: &str) -> Result<Option<String>, SabnzbdError> {
        for mode in ["queue", "history"] {
            let reply = self
                .call(&[("mode", mode), ("search", job_name), ("limit", "100")])
                .await?;
            if let Some(slot) = slots(&reply, mode)?.iter().find(|slot| {
                ["name", "nzb_name"].into_iter().any(|field| {
                    slot.get(field).and_then(Value::as_str).is_some_and(|name| {
                        name == job_name || name.strip_suffix(".nzb") == Some(job_name)
                    })
                })
            }) {
                let id = slot
                    .get("nzo_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or(SabnzbdError::InvalidResponse)?;
                return Ok(Some(id.to_owned()));
            }
        }
        Ok(None)
    }

    async fn status(&self, id: &str) -> Result<NzbStatus, SabnzbdError> {
        let history = self
            .call(&[("mode", "history"), ("nzo_ids", id), ("limit", "1")])
            .await?;
        if let Some(slot) = job(&history, "history", id)? {
            return match slot.get("status").and_then(Value::as_str) {
                Some("Completed") => slot
                    .get("storage")
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty())
                    .map(|path| NzbStatus::Completed {
                        path: path.to_owned(),
                    })
                    .ok_or(SabnzbdError::InvalidResponse),
                Some("Failed") => Ok(NzbStatus::Failed),
                Some(
                    "Queued" | "QuickCheck" | "Verifying" | "Repairing" | "Fetching" | "Extracting"
                    | "Moving" | "Running",
                ) => Ok(NzbStatus::Queued { progress: 100.0 }),
                _ => Err(SabnzbdError::InvalidResponse),
            };
        }
        let queue = self
            .call(&[("mode", "queue"), ("nzo_ids", id), ("limit", "1")])
            .await?;
        if let Some(slot) = job(&queue, "queue", id)? {
            let progress = slot
                .get("percentage")
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|value| value.parse::<f64>().ok())
                        .or_else(|| value.as_f64())
                })
                .unwrap_or(0.0);
            return Ok(NzbStatus::Queued {
                progress: progress.clamp(0.0, 100.0),
            });
        }
        Ok(NzbStatus::Missing)
    }

    async fn cancel(&self, id: &str) -> Result<(), SabnzbdError> {
        let history = self
            .call(&[("mode", "history"), ("nzo_ids", id), ("limit", "1")])
            .await?;
        if let Some(slot) = job(&history, "history", id)? {
            if matches!(
                slot.get("status").and_then(Value::as_str),
                Some("Completed" | "Failed")
            ) {
                return Ok(());
            }
            return accepted(&self.call(&[("mode", "cancel_pp"), ("value", id)]).await?);
        }
        let reply = self
            .call(&[
                ("mode", "queue"),
                ("name", "delete"),
                ("value", id),
                ("del_files", "0"),
            ])
            .await?;
        accepted(&reply)?;
        // It can enter postprocessing between the history query and queue
        // deletion. SAB also reports successful deletion for a missing id.
        let history = self
            .call(&[("mode", "history"), ("nzo_ids", id), ("limit", "1")])
            .await?;
        if let Some(slot) = job(&history, "history", id)? {
            if matches!(
                slot.get("status").and_then(Value::as_str),
                Some("Completed" | "Failed")
            ) {
                return Ok(());
            }
            return accepted(&self.call(&[("mode", "cancel_pp"), ("value", id)]).await?);
        }
        let queue = self
            .call(&[("mode", "queue"), ("nzo_ids", id), ("limit", "1")])
            .await?;
        if job(&queue, "queue", id)?.is_some() {
            return Err(SabnzbdError::Rejected);
        }
        Ok(())
    }

    async fn test_connection(&self) -> Result<String, SabnzbdError> {
        let reply = self.call(&[("mode", "version")]).await?;
        reply
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(SabnzbdError::InvalidResponse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };

    #[tokio::test]
    async fn adds_nzb_and_reads_completed_storage_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .and(body_string_contains("mode=addurl"))
            .and(body_string_contains("nzbname=bokhylle-123"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"status": true, "nzo_ids": ["job-1"]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/api")).and(body_string_contains("mode=history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"history": {"slots": [{"nzo_id": "job-1", "status": "Completed", "storage": "/downloads/book"}]}})))
            .mount(&server).await;
        let client = SabnzbdClient::new(&format!("{}/api", server.uri()), "secret").unwrap();
        assert_eq!(
            client
                .add_url(
                    "https://indexer.test/api?t=get&id=abc",
                    "books",
                    "bokhylle-123"
                )
                .await
                .unwrap(),
            "job-1"
        );
        assert_eq!(
            client.status("job-1").await.unwrap(),
            NzbStatus::Completed {
                path: "/downloads/book".into()
            }
        );
    }

    #[tokio::test]
    async fn finds_named_job_after_add_before_id_was_saved() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .and(body_string_contains("mode=queue"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "queue": {"slots": [{"nzo_id": "job-1", "nzb_name": "bokhylle-123.nzb"}]}
            })))
            .mount(&server)
            .await;
        let client = SabnzbdClient::new(&format!("{}/api", server.uri()), "secret").unwrap();
        assert_eq!(
            client.find_by_name("bokhylle-123").await.unwrap(),
            Some("job-1".into())
        );
    }

    #[tokio::test]
    async fn malformed_queue_is_not_treated_as_a_missing_job() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"error": "API key invalid"})),
            )
            .mount(&server)
            .await;
        let client = SabnzbdClient::new(&format!("{}/api", server.uri()), "secret").unwrap();
        assert!(matches!(
            client.find_by_name("bokhylle-123").await,
            Err(SabnzbdError::InvalidResponse)
        ));
        assert!(matches!(
            client.status("job-1").await,
            Err(SabnzbdError::InvalidResponse)
        ));
    }

    #[tokio::test]
    async fn cancellation_aborts_postprocessing_without_deleting_files() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api")).and(body_string_contains("mode=history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"history": {"slots": [{"nzo_id": "job-1", "status": "Extracting"}]}})))
            .expect(1).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .and(body_string_contains("mode=cancel_pp"))
            .and(body_string_contains("value=job-1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"status": true, "nzo_ids": ["job-1"]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let client = SabnzbdClient::new(&format!("{}/api", server.uri()), "secret").unwrap();
        client.cancel("job-1").await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_requires_an_explicit_success_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .and(body_string_contains("mode=history"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"history": {"slots": []}})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api"))
            .and(body_string_contains("name=delete"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let client = SabnzbdClient::new(&format!("{}/api", server.uri()), "secret").unwrap();
        assert!(matches!(
            client.cancel("job-1").await,
            Err(SabnzbdError::InvalidResponse)
        ));
    }
}
