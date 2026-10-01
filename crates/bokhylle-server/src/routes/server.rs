use crate::{AppState, auth::AdminUser, error::AppError, server};
use axum::{Json, extract::State};

pub async fn status(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<server::ServerStatus>, AppError> {
    Ok(Json(server::status(&state).await?))
}

pub async fn restart(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<server::RestartChange>>, AppError> {
    Ok(Json(state.server.pending_restart(&state.settings).await?))
}

pub async fn diagnostics(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<server::diagnostics::Diagnostics>, AppError> {
    Ok(Json(server::diagnostics::collect(&state).await?))
}

pub async fn updates(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<server::releases::UpdateStatus>, AppError> {
    Ok(Json(state.server.releases.status(&state).await?))
}

pub async fn check_updates(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<server::releases::UpdateStatus>, AppError> {
    Ok(Json(
        state
            .server
            .releases
            .check_at(&state, true, server::now())
            .await?,
    ))
}
