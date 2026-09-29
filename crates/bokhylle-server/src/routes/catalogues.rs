use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use bokhylle_metadata::MetadataResult;

use crate::AppState;
use crate::auth::{AdminUser, AuthUser};
use crate::error::AppError;
use crate::opds_catalog::{self, CatalogFeed, CatalogSource};
use crate::routes::responses::StatusJson;

async fn adult(state: &AppState, user_id: i64) -> Result<(), AppError> {
    if crate::auth::profile_type(&state.db, user_id).await? == "child" {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

pub async fn list(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<CatalogSource>>, AppError> {
    adult(&state, user.id).await?;
    let sources = sqlx::query_as("SELECT id, name, url FROM opds_sources ORDER BY name")
        .fetch_all(&state.db)
        .await?;
    Ok(Json(sources))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateSource {
    pub name: String,
    pub url: String,
}

pub async fn create_source(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateSource>,
) -> Result<StatusJson<CatalogSource, 201>, AppError> {
    let name = body.name.trim();
    if name.is_empty() || name.len() > 80 {
        return Err(AppError::BadRequest(
            "catalogue name must be 1 to 80 characters".to_string(),
        ));
    }
    if body.url.len() > 2048 {
        return Err(AppError::BadRequest(
            "catalogue URL is too long".to_string(),
        ));
    }
    let url = crate::remote_http::parse_url(&body.url)?.to_string();
    let source = CatalogSource {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_string(),
        url,
    };
    sqlx::query("INSERT INTO opds_sources (id, name, url) VALUES (?, ?, ?)")
        .bind(&source.id)
        .bind(&source.name)
        .bind(&source.url)
        .execute(&state.db)
        .await?;
    Ok(StatusJson(source))
}

pub async fn delete_source(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let removed = sqlx::query("DELETE FROM opds_sources WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await?;
    if removed.rows_affected() == 0 {
        return Err(AppError::NotFound("catalogue not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FeedParams {
    pub url: Option<String>,
    pub q: Option<String>,
}

async fn source(state: &AppState, id: &str) -> Result<CatalogSource, AppError> {
    sqlx::query_as("SELECT id, name, url FROM opds_sources WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("catalogue not found".to_string()))
}

pub async fn feed(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<FeedParams>,
) -> Result<Json<CatalogFeed>, AppError> {
    adult(&state, user.id).await?;
    let source = source(&state, &id).await?;
    Ok(Json(
        opds_catalog::fetch(&source, params.url.as_deref(), params.q.as_deref()).await?,
    ))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcquireFromCatalog {
    pub page_url: String,
    pub entry_id: String,
    pub file_index: usize,
    pub send_to_reader: Option<bool>,
}

pub async fn acquire(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AcquireFromCatalog>,
) -> Result<StatusJson<crate::services::books::CatalogueAcquisitionOutcome, 202>, AppError> {
    if !crate::auth::can_acquire(&state.db, &user).await? {
        return Err(AppError::Forbidden);
    }
    let source = source(&state, &id).await?;
    let feed = opds_catalog::fetch(&source, Some(&body.page_url), None).await?;
    let entry = feed
        .entries
        .iter()
        .find(|entry| entry.id == body.entry_id)
        .ok_or_else(|| {
            AppError::NotFound("publication not found in this catalogue page".to_string())
        })?;
    let file = entry
        .files
        .get(body.file_index)
        .ok_or_else(|| AppError::BadRequest("file choice is unavailable".to_string()))?;

    let key = hex::encode(Sha256::digest(entry.id.as_bytes()));
    let metadata = MetadataResult {
        provider: format!("opds:{}", source.id),
        provider_key: key.clone(),
        title: entry.title.clone(),
        authors: entry.authors.clone(),
        language: entry.language.clone(),
        languages: entry.language.iter().cloned().collect(),
        ..Default::default()
    };
    let book_id =
        crate::library::import_metadata::upsert_book_from_metadata(&state.db, &metadata).await?;
    let trusted_origin = crate::remote_http::origin(&source.url)?;
    let (acquisition, duplicate) = crate::routes::acquisitions::start_http(
        &state,
        &user,
        book_id,
        &file.url,
        &file.format,
        "opds",
        &source.name,
        &key,
        Some(&trusted_origin),
        body.send_to_reader.unwrap_or(false),
    )
    .await?;
    Ok(StatusJson(
        crate::services::books::CatalogueAcquisitionOutcome {
            id: acquisition.id.clone(),
            status: acquisition.status()?,
            duplicate,
            book_id,
        },
    ))
}
