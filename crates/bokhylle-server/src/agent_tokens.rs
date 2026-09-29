//! Per-profile credentials for the MCP endpoint. One token represents one
//! Bokhylle profile with a read/write scope; the profile's own capabilities
//! (child rules, roles) stay authoritative at call time.

use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::auth::{User, hash_token};
use crate::error::AppError;

#[derive(Debug, Serialize, sqlx::FromRow, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgentToken {
    pub id: i64,
    pub name: String,
    pub scope: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// The authenticated caller behind an MCP request.
#[derive(Debug, Clone)]
pub struct AgentPrincipal {
    pub user: User,
    pub scope: String,
}

impl AgentPrincipal {
    pub fn can_write(&self) -> bool {
        self.scope == "write"
    }
}

fn clean_scope(scope: &str) -> Result<String, AppError> {
    match scope {
        "read" | "write" => Ok(scope.to_string()),
        _ => Err(AppError::Unprocessable(
            "scope must be 'read' or 'write'".to_string(),
        )),
    }
}

/// Creates an agent token. The plain token is returned once and never stored.
pub async fn create(
    pool: &SqlitePool,
    user_id: i64,
    name: &str,
    scope: &str,
) -> Result<(i64, String), AppError> {
    let name = name.trim();
    let name = if name.is_empty() { "Agent" } else { name };
    let scope = clean_scope(scope)?;
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

    let id = sqlx::query(
        "INSERT INTO agent_tokens (user_id, name, token_hash, scope) VALUES (?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(name)
    .bind(hash_token(&token))
    .bind(&scope)
    .execute(pool)
    .await?
    .last_insert_rowid();

    Ok((id, token))
}

pub async fn list(pool: &SqlitePool, user_id: i64) -> Result<Vec<AgentToken>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, name, scope, created_at, last_used_at
         FROM agent_tokens
         WHERE user_id = ? AND revoked_at IS NULL
         ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

pub async fn revoke(pool: &SqlitePool, user_id: i64, id: i64) -> Result<(), AppError> {
    sqlx::query("UPDATE agent_tokens SET revoked_at = unixepoch() WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resolves a bearer token to its profile (revoked and disabled accounts are
/// refused). `last_used_at` is written at most every five minutes per token.
pub async fn authenticate(
    pool: &SqlitePool,
    token: &str,
) -> Result<Option<AgentPrincipal>, AppError> {
    if token.is_empty() {
        return Ok(None);
    }
    let hash = hash_token(token);
    let row: Option<(i64, i64, String)> = sqlx::query_as(
        "SELECT id, user_id, scope FROM agent_tokens
         WHERE token_hash = ? AND revoked_at IS NULL",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await?;
    let Some((id, user_id, scope)) = row else {
        return Ok(None);
    };

    let user = crate::auth::Auth::new(pool.clone())
        .user_by_id(user_id)
        .await?;
    let Some(user) = user else {
        return Ok(None);
    };
    let disabled: i64 = sqlx::query_scalar("SELECT disabled FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    if disabled != 0 {
        return Ok(None);
    }

    sqlx::query(
        "UPDATE agent_tokens SET last_used_at = unixepoch()
         WHERE id = ? AND (last_used_at IS NULL OR last_used_at < unixepoch() - 300)",
    )
    .bind(id)
    .execute(pool)
    .await
    .ok();

    Ok(Some(AgentPrincipal { user, scope }))
}
