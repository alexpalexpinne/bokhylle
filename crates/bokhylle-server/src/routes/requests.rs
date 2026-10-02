use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::AuthUser;
use crate::book_requests::BookRequestView;
use crate::discovery::{self, SearchKind};
use crate::error::AppError;
use crate::routes::responses::{CreatedOrOk, Items};
use crate::services;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestSearchParams {
    pub q: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestSearchItem {
    provider: String,
    provider_key: String,
    title: String,
    authors: Vec<String>,
    year: Option<i32>,
    language: Option<String>,
    series: Option<String>,
    series_number: Option<String>,
    cover_id: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestBookDetail {
    provider: String,
    provider_key: String,
    title: String,
    authors: Vec<String>,
    year: Option<i32>,
    language: Option<String>,
    languages: Vec<String>,
    series: Option<String>,
    series_number: Option<String>,
    description: Option<String>,
    cover_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestBookParams {
    provider: String,
    provider_key: String,
}

/// Public provider metadata only, with no local ownership or acquisition data.
pub async fn book(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<RequestBookParams>,
) -> Result<Json<RequestBookDetail>, AppError> {
    if !services::requests::may_discover(&state, user.id).await? {
        return Err(AppError::Forbidden);
    }
    let metadata =
        discovery::resolve_metadata(&state, Some(&params.provider), &params.provider_key)
            .await?
            .ok_or_else(|| {
                AppError::NotFound("the book was not found in the metadata catalogue".to_string())
            })?;
    Ok(Json(RequestBookDetail {
        provider: metadata.provider,
        provider_key: metadata.provider_key,
        title: metadata.title,
        authors: metadata.authors,
        year: metadata.year,
        language: metadata.language,
        languages: metadata.languages,
        series: metadata.series,
        series_number: metadata.series_number,
        description: metadata.description,
        cover_id: metadata.cover_id,
    }))
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct RequestResponse {
    request: BookRequestView,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct CreateRequestResponse {
    request: BookRequestView,
    duplicate: bool,
}

/// Public provider metadata only. A child with either catalogue permission
/// may browse; only can_request permits submitting a request. No household
/// ownership, availability or release information is returned. The catalogue
/// is not age filtered.
pub async fn search(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(params): Query<RequestSearchParams>,
) -> Result<Json<Items<RequestSearchItem>>, AppError> {
    if !services::requests::may_browse_catalogue(&state, user.id).await? {
        return Err(AppError::Forbidden);
    }
    let query = params.q.unwrap_or_default();
    let query = query.trim();
    if query.len() < 2 {
        return Ok(Json(Items { items: Vec::new() }));
    }
    let kind = SearchKind::parse(params.kind.as_deref().unwrap_or("any"))
        .ok_or_else(|| AppError::BadRequest("invalid search type".to_string()))?;

    let results =
        discovery::external_results(&state, kind, query, params.limit.unwrap_or(24)).await?;
    let items: Vec<RequestSearchItem> = results
        .into_iter()
        .map(|result| RequestSearchItem {
            provider: result.provider,
            provider_key: result.provider_key,
            title: result.title,
            authors: result.authors,
            year: result.year,
            language: result.language,
            series: result.series,
            series_number: result.series_number,
            cover_id: result.cover_id,
        })
        .collect();
    Ok(Json(Items { items }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequestInput {
    pub provider: String,
    pub provider_key: String,
    pub sharing: Option<crate::services::sharing::BookSharing>,
}

/// A member asks for a book; administrators are notified and decide.
pub async fn create(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateRequestInput>,
) -> Result<CreatedOrOk<CreateRequestResponse>, AppError> {
    let outcome = services::requests::create_with_sharing(
        &state,
        &user,
        &body.provider,
        &body.provider_key,
        body.sharing,
    )
    .await?;
    Ok(CreatedOrOk {
        created: !outcome.duplicate,
        value: CreateRequestResponse {
            request: outcome.request,
            duplicate: outcome.duplicate,
        },
    })
}

pub async fn list(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Items<BookRequestView>>, AppError> {
    let items = services::requests::list(&state, &user).await?;
    Ok(Json(Items { items }))
}

pub async fn approve(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<RequestResponse>, AppError> {
    let request = services::requests::approve(&state, &user, id).await?;
    Ok(Json(RequestResponse { request }))
}

pub async fn decline(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<RequestResponse>, AppError> {
    let request = services::requests::decline(&state, &user, id).await?;
    Ok(Json(RequestResponse { request }))
}
