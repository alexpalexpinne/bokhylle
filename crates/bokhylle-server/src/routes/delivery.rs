use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::{AdminUser, AuthUser};
use crate::delivery::{self, Delivery, DeliveryTarget};
use crate::error::AppError;

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevicePreset {
    device_type: String,
    label: String,
    connector: String,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DefaultTarget {
    address: Option<String>,
    source: String,
    sender_address: Option<String>,
    amazon_url: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct ConnectionStatus {
    status: &'static str,
}

pub async fn list_targets(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<DeliveryTarget>>, AppError> {
    let targets = delivery::list_targets(&state.db, user.id).await?;
    Ok(Json(targets))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetInput {
    pub name: Option<String>,
    #[serde(default)]
    pub address: String,
    pub connector: Option<String>,
    pub device_type: Option<String>,
}

pub async fn create_target(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<TargetInput>,
) -> Result<(StatusCode, Json<DeliveryTarget>), AppError> {
    let target = delivery::create_target(
        &state.db,
        user.id,
        body.name.as_deref().unwrap_or(""),
        &body.address,
        body.connector.as_deref().unwrap_or("email"),
        body.device_type.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(target)))
}

pub async fn presets() -> Json<Vec<DevicePreset>> {
    Json(
        delivery::DEVICE_PRESETS
            .iter()
            .map(|(device_type, label, connector)| DevicePreset {
                device_type: (*device_type).to_string(),
                label: (*label).to_string(),
                connector: (*connector).to_string(),
            })
            .collect(),
    )
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TargetUpdate {
    pub name: Option<String>,
    pub address: Option<String>,
    pub enabled: Option<bool>,
}

pub async fn update_target(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<TargetUpdate>,
) -> Result<Json<DeliveryTarget>, AppError> {
    let target = delivery::update_target(
        &state.db,
        user.id,
        id,
        body.name,
        body.address,
        body.enabled,
    )
    .await?
    .ok_or_else(|| AppError::NotFound("delivery target not found".to_string()))?;
    Ok(Json(target))
}

pub async fn set_default(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<DeliveryTarget>>, AppError> {
    if !delivery::set_default(&state.db, user.id, id).await? {
        return Err(AppError::NotFound("delivery target not found".to_string()));
    }
    let targets = delivery::list_targets(&state.db, user.id).await?;
    Ok(Json(targets))
}

pub async fn delete_target(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    if delivery::delete_target(&state.db, user.id, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("delivery target not found".to_string()))
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliverRequest {
    pub target_id: Option<i64>,
}

pub async fn deliver(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((book_id, file_id)): Path<(i64, i64)>,
    Json(body): Json<DeliverRequest>,
) -> Result<(StatusCode, Json<Delivery>), AppError> {
    let record = delivery::deliver(&state, user.id, book_id, file_id, body.target_id).await?;
    Ok((StatusCode::ACCEPTED, Json(record)))
}

pub async fn default_target(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<DefaultTarget>, AppError> {
    let (address, source) = delivery::default_reader(&state, user.id).await?;

    let sender_address = state
        .settings
        .get_string(crate::settings::SMTP_FROM, "")
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    let domain = state
        .settings
        .get_string(crate::settings::AMAZON_DOMAIN, "amazon.com")
        .await
        .unwrap_or_else(|_| "amazon.com".to_string())
        .trim()
        .trim_start_matches("www.")
        .trim_matches('/')
        .to_string();
    let domain = if domain.is_empty() {
        "amazon.com".to_string()
    } else {
        domain
    };
    let amazon_url = format!("https://www.{domain}/hz/mycd/myx#/home/settings/pdoc");

    Ok(Json(DefaultTarget {
        address,
        source: source.to_string(),
        sender_address: (!sender_address.is_empty()).then_some(sender_address),
        amazon_url,
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryQuery {
    pub book_id: Option<i64>,
    pub all: Option<bool>,
}

pub async fn list_deliveries(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<DeliveryQuery>,
) -> Result<Json<Vec<Delivery>>, AppError> {
    let is_admin = user.role == crate::auth::Role::Admin;
    let deliveries = delivery::list_deliveries(
        &state.db,
        user.id,
        is_admin,
        params.all.unwrap_or(false),
        params.book_id,
    )
    .await?;
    Ok(Json(deliveries))
}

pub async fn retry(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<(StatusCode, Json<Delivery>), AppError> {
    let is_admin = user.role == crate::auth::Role::Admin;
    let record = delivery::retry(&state, user.id, is_admin, id).await?;
    Ok((StatusCode::ACCEPTED, Json(record)))
}

pub async fn test_smtp(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<ConnectionStatus>, AppError> {
    delivery::test_connection(&state).await?;
    Ok(Json(ConnectionStatus { status: "ok" }))
}
