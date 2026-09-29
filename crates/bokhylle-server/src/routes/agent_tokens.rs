use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::AppState;
use crate::agent_tokens;
use crate::auth::AuthUser;
use crate::error::AppError;
use crate::routes::responses::{CreatedToken, StatusJson, Tokens};

pub async fn list(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Tokens<agent_tokens::AgentToken>>, AppError> {
    let tokens = agent_tokens::list(&state.db, user.id).await?;
    Ok(Json(Tokens { tokens }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateAgentToken {
    pub name: Option<String>,
    pub scope: Option<String>,
}

/// Creates an MCP access token for the signed-in profile. The plain token is
/// shown once.
pub async fn create(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateAgentToken>,
) -> Result<StatusJson<CreatedToken, 201>, AppError> {
    let (id, token) = agent_tokens::create(
        &state.db,
        user.id,
        body.name.as_deref().unwrap_or("Agent"),
        body.scope.as_deref().unwrap_or("read"),
    )
    .await?;
    tracing::info!(user_id = user.id, token_id = id, "agent_token.created");
    Ok(StatusJson(CreatedToken { id, token }))
}

pub async fn revoke(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    agent_tokens::revoke(&state.db, user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
