use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::AppState;
use crate::auth::AdminUser;
use crate::delivery::{self, DeliveryTarget};
use crate::error::AppError;
use crate::routes::delivery::{TargetInput, TargetUpdate};
use crate::routes::responses::{CreatedToken, StatusJson, Tokens};

async fn require_child(state: &AppState, user_id: i64) -> Result<(), AppError> {
    let profile: Option<String> = sqlx::query_scalar("SELECT profile_type FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    if profile.as_deref() != Some("child") {
        return Err(AppError::NotFound("child profile not found".into()));
    }
    Ok(())
}

pub async fn list_targets(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<Vec<DeliveryTarget>>, AppError> {
    require_child(&state, user_id).await?;
    Ok(Json(delivery::list_targets(&state.db, user_id).await?))
}

pub async fn create_target(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Json(body): Json<TargetInput>,
) -> Result<(StatusCode, Json<DeliveryTarget>), AppError> {
    require_child(&state, user_id).await?;
    let target = delivery::create_target(
        &state.db,
        user_id,
        body.name.as_deref().unwrap_or(""),
        &body.address,
        body.connector.as_deref().unwrap_or("email"),
        body.device_type.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(target)))
}

pub async fn update_target(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path((user_id, target_id)): Path<(i64, i64)>,
    Json(body): Json<TargetUpdate>,
) -> Result<Json<DeliveryTarget>, AppError> {
    require_child(&state, user_id).await?;
    let target = delivery::update_target(
        &state.db,
        user_id,
        target_id,
        body.name,
        body.address,
        body.enabled,
    )
    .await?
    .ok_or_else(|| AppError::NotFound("delivery target not found".into()))?;
    Ok(Json(target))
}

pub async fn set_default(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path((user_id, target_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<DeliveryTarget>>, AppError> {
    require_child(&state, user_id).await?;
    if !delivery::set_default(&state.db, user_id, target_id).await? {
        return Err(AppError::NotFound("delivery target not found".into()));
    }
    Ok(Json(delivery::list_targets(&state.db, user_id).await?))
}

pub async fn delete_target(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path((user_id, target_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    require_child(&state, user_id).await?;
    if delivery::delete_target(&state.db, user_id, target_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("delivery target not found".into()))
    }
}

pub async fn list_tokens(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<Tokens<crate::reader_tokens::ReaderToken>>, AppError> {
    require_child(&state, user_id).await?;
    Ok(Json(Tokens {
        tokens: crate::reader_tokens::list(&state.db, user_id).await?,
    }))
}

pub async fn create_token(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Json(body): Json<crate::routes::auth::TokenInput>,
) -> Result<StatusJson<CreatedToken, 201>, AppError> {
    require_child(&state, user_id).await?;
    let (id, token) =
        crate::reader_tokens::create(&state.db, user_id, body.name.as_deref().unwrap_or("Reader"))
            .await?;
    Ok(StatusJson(CreatedToken { id, token }))
}

pub async fn revoke_token(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path((user_id, token_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    require_child(&state, user_id).await?;
    crate::reader_tokens::revoke(&state.db, user_id, token_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
