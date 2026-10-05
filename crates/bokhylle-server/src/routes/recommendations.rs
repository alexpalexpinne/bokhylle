use super::spotlight::{self, SpotlightItem};
use crate::{
    AppState,
    auth::{AuthUser, Role},
    error::AppError,
    services::recommendations as engine,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationsQuery {
    #[serde(default)]
    pub cached_only: bool,
    pub subject: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationsPage {
    pub items: Vec<SpotlightItem>,
    pub subjects: Vec<String>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

pub async fn list(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Query(query): Query<RecommendationsQuery>,
) -> Result<Json<RecommendationsPage>, AppError> {
    if !crate::services::requests::may_discover(&state, user.id).await? {
        return Err(AppError::Forbidden);
    }
    let selection = spotlight::selection(user, state, query.cached_only, 72, true).await?;
    let mut subjects = std::collections::BTreeSet::new();
    for item in &selection.recommendations {
        for name in &item.subjects {
            let normalized = crate::library::subjects::normalized(name);
            if crate::library::subjects::is_displayable(&normalized) {
                subjects.insert(crate::library::subjects::concept(&normalized).to_string());
            }
        }
    }
    let subject = query.subject.map(|s| {
        let n = crate::library::subjects::normalized(&s);
        crate::library::subjects::concept(&n).to_string()
    });
    let matches: Vec<_> = selection
        .recommendations
        .into_iter()
        .filter(|item| {
            subject.as_ref().is_none_or(|s| {
                item.subjects.iter().any(|name| {
                    crate::library::subjects::concept(&crate::library::subjects::normalized(name))
                        == s
                })
            })
        })
        .collect();
    let total = matches.len();
    let offset = query.offset.unwrap_or(0).min(72);
    let limit = query.limit.unwrap_or(24).clamp(1, 72);
    let next_offset = (offset + limit < total).then_some(offset + limit);
    Ok(Json(RecommendationsPage {
        items: matches.into_iter().skip(offset).take(limit).collect(),
        subjects: subjects.into_iter().collect(),
        total,
        next_offset,
    }))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct FeedbackInput {
    pub action: engine::FeedbackAction,
}
pub async fn feedback(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<FeedbackInput>,
) -> Result<Json<engine::FeedbackReceipt>, AppError> {
    engine::feedback(&state, &user, &key, body.action)
        .await
        .map(Json)
}

pub async fn undo_feedback(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path((key, token)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    engine::undo_feedback(&state, &user, &key, &token).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ImpressionsInput {
    pub keys: Vec<String>,
}
pub async fn impressions(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<ImpressionsInput>,
) -> Result<StatusCode, AppError> {
    engine::impressions(&state, &user, &body.keys).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn diagnostics(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<engine::RecommendationDiagnostics>, AppError> {
    if user.role != Role::Admin {
        return Err(AppError::Forbidden);
    }
    Ok(Json(engine::diagnostics(&state, user.id).await?))
}

pub async fn series(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::library::queries::SeriesContinuation>>, AppError> {
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    crate::library::queries::home_series(&state.db, user.id, child)
        .await
        .map(Json)
}

pub async fn restore_rejection(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    engine::restore_rejection(&state, user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
