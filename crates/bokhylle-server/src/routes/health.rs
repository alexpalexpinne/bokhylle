use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::AppState;
use crate::error::AppError;

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
}

pub async fn liveness() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: bokhylle_core::VERSION,
    })
}

pub async fn readiness(State(state): State<AppState>) -> Result<Json<Health>, AppError> {
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.db)
        .await?;

    Ok(Json(Health {
        status: "ok",
        version: bokhylle_core::VERSION,
    }))
}
