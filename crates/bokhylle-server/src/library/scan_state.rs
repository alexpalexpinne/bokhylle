use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::{ScanSummary, index_library};
use crate::AppState;
use crate::error::AppError;

#[derive(Default)]
struct Inner {
    running: bool,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    summary: Option<ScanSummary>,
    error: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub running: bool,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub summary: Option<ScanSummary>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct ScanState {
    inner: Mutex<Inner>,
}

impl ScanState {
    pub fn status(&self) -> ScanStatus {
        let inner = self.inner.lock().expect("scan state lock");
        ScanStatus {
            running: inner.running,
            started_at: inner.started_at,
            finished_at: inner.finished_at,
            summary: inner.summary.clone(),
            error: inner.error.clone(),
        }
    }

    fn try_begin(&self) -> bool {
        let mut inner = self.inner.lock().expect("scan state lock");
        if inner.running {
            return false;
        }

        inner.running = true;
        inner.started_at = Some(now_epoch());
        inner.finished_at = None;
        inner.error = None;
        true
    }

    fn finish(&self, result: Result<ScanSummary, AppError>) {
        let mut inner = self.inner.lock().expect("scan state lock");
        inner.running = false;
        inner.finished_at = Some(now_epoch());

        match result {
            Ok(summary) => inner.summary = Some(summary),
            Err(error) => {
                tracing::error!(%error, "library.scan.failed");
                inner.error = Some(error.to_string());
            }
        }
    }
}

pub fn start_scan(state: &AppState) -> bool {
    if !state.scan_state.try_begin() {
        return false;
    }

    let state = state.clone();
    tokio::spawn(async move {
        let result = index_library(&state).await;
        state.scan_state.finish(result);
    });

    true
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
