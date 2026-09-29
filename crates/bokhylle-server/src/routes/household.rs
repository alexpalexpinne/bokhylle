use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::{AuthUser, Role, User};
use crate::error::AppError;

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HouseholdMember {
    id: i64,
    username: String,
    display_name: String,
    profile_type: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct HouseholdMembers {
    members: Vec<HouseholdMember>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShelfUser {
    user_id: i64,
    username: String,
    display_name: String,
    profile_type: String,
    on_shelf: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct ShelfUsers {
    users: Vec<ShelfUser>,
}

/// Only administrators may browse child shelves. Adult shelves remain private.
pub async fn members(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<HouseholdMembers>, AppError> {
    if (user.role != Role::Admin && state.demo.is_none())
        || crate::auth::profile_type(&state.db, user.id).await? == "child"
    {
        return Err(AppError::Forbidden);
    }
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = if state.demo.is_some() {
        let (_, child) = crate::demo::pair_names(&user.username).ok_or(AppError::Forbidden)?;
        sqlx::query_as(
            "SELECT id, username, display_name, profile_type FROM users
             WHERE disabled = 0 AND profile_type = 'child' AND username = ? ORDER BY username",
        )
        .bind(child)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, username, display_name, profile_type
             FROM users WHERE disabled = 0 AND profile_type = 'child' ORDER BY username",
        )
        .fetch_all(&state.db)
        .await?
    };
    let members: Vec<HouseholdMember> = rows
        .into_iter()
        .map(
            |(id, username, display_name, profile_type)| HouseholdMember {
                id,
                display_name: display_name.unwrap_or_else(|| username.clone()),
                username,
                profile_type: profile_type.unwrap_or_else(|| "adult".to_string()),
            },
        )
        .collect();
    Ok(Json(HouseholdMembers { members }))
}

pub(crate) async fn can_manage_child(
    state: &AppState,
    actor: &User,
    target_id: i64,
) -> Result<bool, AppError> {
    if (actor.role != Role::Admin && state.demo.is_none())
        || crate::auth::profile_type(&state.db, actor.id).await? == "child"
    {
        return Ok(false);
    }
    let target: Option<(String, String)> =
        sqlx::query_as("SELECT profile_type, username FROM users WHERE id = ? AND disabled = 0")
            .bind(target_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((profile, username)) = target else {
        return Ok(false);
    };
    if profile != "child" {
        return Ok(false);
    }
    if state.demo.is_some() {
        let (_, child) = crate::demo::pair_names(&actor.username).ok_or(AppError::Forbidden)?;
        return Ok(username == child);
    }
    Ok(true)
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShelfAssignInput {
    pub on_shelf: bool,
}

/// Only an administrator may assign a child's shelf. Adults curate their own
/// shelf through /api/books/{id}/shelf.
pub async fn assign_shelf(
    AuthUser(actor): AuthUser,
    State(state): State<AppState>,
    Path((user_id, book_id)): Path<(i64, i64)>,
    Json(body): Json<ShelfAssignInput>,
) -> Result<StatusCode, AppError> {
    if !can_manage_child(&state, &actor, user_id).await? {
        return Err(AppError::Forbidden);
    }
    if body.on_shelf {
        crate::user_books::add(&state.db, user_id, book_id, "parent_assigned").await?;
    } else {
        crate::user_books::remove(&state.db, user_id, book_id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Only child assignments are visible here. Other adults' shelves are private.
pub async fn book_shelf_users(
    AuthUser(actor): AuthUser,
    State(state): State<AppState>,
    Path(book_id): Path<i64>,
) -> Result<Json<ShelfUsers>, AppError> {
    if (actor.role != Role::Admin && state.demo.is_none())
        || crate::auth::profile_type(&state.db, actor.id).await? == "child"
    {
        return Err(AppError::Forbidden);
    }
    let sql = "SELECT u.id, u.username, u.display_name, COALESCE(u.profile_type, 'adult'),
                COALESCE(ub.on_shelf, 0)
         FROM users u
         LEFT JOIN user_books ub ON ub.user_id = u.id AND ub.book_id = ?
         WHERE u.disabled = 0 AND COALESCE(u.profile_type, 'adult') = 'child'
         ORDER BY u.username";
    let rows: Vec<(i64, String, Option<String>, String, i64)> = if state.demo.is_some() {
        let (_, child) = crate::demo::pair_names(&actor.username).ok_or(AppError::Forbidden)?;
        sqlx::query_as(
            "SELECT u.id, u.username, u.display_name, COALESCE(u.profile_type, 'adult'),
                    COALESCE(ub.on_shelf, 0)
             FROM users u LEFT JOIN user_books ub ON ub.user_id = u.id AND ub.book_id = ?
             WHERE u.disabled = 0 AND u.username = ?",
        )
        .bind(book_id)
        .bind(child)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as(sql)
            .bind(book_id)
            .fetch_all(&state.db)
            .await?
    };
    let entries: Vec<ShelfUser> = rows
        .into_iter()
        .map(
            |(user_id, username, display_name, profile_type, on_shelf)| ShelfUser {
                user_id,
                display_name: display_name.unwrap_or_else(|| username.clone()),
                username,
                profile_type,
                on_shelf: on_shelf != 0,
            },
        )
        .collect();
    Ok(Json(ShelfUsers { users: entries }))
}
