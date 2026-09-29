use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::AppState;
use crate::auth::{AuthUser, Role};
use crate::book_requests;
use crate::error::AppError;
use crate::notifications;
use crate::routes::responses::UpdatedCount;

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsResponse {
    items: Vec<notifications::Notification>,
    unread: i64,
    pending_requests: i64,
    pending_request_items: Vec<book_requests::BookRequestView>,
}

pub async fn list_notifications(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<NotificationsResponse>, AppError> {
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let items = notifications::list(&state.db, user.id, 50, child).await?;
    let unread = notifications::unread_count(&state.db, user.id, child).await?;
    // Only administrators decide others' requests. Other readers see their
    // own pending requests, without learning what a child asked for.
    let pending_request_items: Vec<book_requests::BookRequestView> = if state.demo.is_some()
        && !child
    {
        let (_, child_name) = crate::demo::pair_names(&user.username).ok_or(AppError::Forbidden)?;
        let requester_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
            .bind(child_name)
            .fetch_one(&state.db)
            .await?;
        book_requests::list(&state.db, requester_id, false).await?
    } else {
        book_requests::list(&state.db, user.id, user.role == Role::Admin && !child).await?
    }
    .into_iter()
    .filter(|item| item.status == "requested")
    .collect();
    let pending_requests = pending_request_items.len() as i64;
    Ok(Json(NotificationsResponse {
        items,
        unread,
        pending_requests,
        pending_request_items,
    }))
}

pub async fn mark_read(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<UpdatedCount>, AppError> {
    let updated = notifications::mark_all_read(&state.db, user.id).await?;
    Ok(Json(UpdatedCount { updated }))
}
