use md5::Digest;
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::auth::{User, hash_token};
use crate::error::AppError;

#[derive(Debug, Serialize, sqlx::FromRow, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReaderToken {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// Creates an OPDS token. The plain token is returned once and never stored.
pub async fn create(
    pool: &SqlitePool,
    user_id: i64,
    name: &str,
) -> Result<(i64, String), AppError> {
    let name = name.trim();
    let name = if name.is_empty() { "Reader" } else { name };
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

    // hash_token(md5(token)): the kosync wire key is compared hashed, so the
    // stored value is not itself a usable credential.
    let sync_hash = hash_token(&hex::encode(md5::Md5::digest(token.as_bytes())));
    let id = sqlx::query(
        "INSERT INTO reader_tokens (user_id, name, token_hash, sync_hash) VALUES (?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(name)
    .bind(hash_token(&token))
    .bind(sync_hash)
    .execute(pool)
    .await?
    .last_insert_rowid();

    Ok((id, token))
}

/// Resolves the `x-auth-key` of a kosync request (md5 of the reader token) to
/// its user. Tokens created before sync support have no stored md5 and simply
/// never match. The username is bound on first use, so one token is one kosync
/// account, like the reference server's `user:<name>:key` entries.
pub async fn authenticate_sync(
    pool: &SqlitePool,
    key: &str,
    username: &str,
) -> Result<Option<User>, AppError> {
    if key.is_empty() || username.is_empty() || username.contains(':') {
        return Ok(None);
    }
    let row: Option<(i64, i64, Option<String>)> =
        sqlx::query_as("SELECT id, user_id, sync_user FROM reader_tokens WHERE sync_hash = ?")
            .bind(hash_token(key))
            .fetch_optional(pool)
            .await?;
    let Some((id, user_id, bound)) = row else {
        return Ok(None);
    };
    match bound {
        Some(name) if name != username => return Ok(None),
        None => {
            let updated = sqlx::query(
                "UPDATE reader_tokens SET sync_user = ? WHERE id = ? AND sync_user IS NULL",
            )
            .bind(username)
            .bind(id)
            .execute(pool)
            .await?;
            if updated.rows_affected() == 0 {
                // Lost the first-use race; only the winning username may auth.
                let bound: Option<String> =
                    sqlx::query_scalar("SELECT sync_user FROM reader_tokens WHERE id = ?")
                        .bind(id)
                        .fetch_optional(pool)
                        .await?
                        .flatten();
                if bound.as_deref() != Some(username) {
                    return Ok(None);
                }
            }
        }
        Some(_) => {}
    }

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
    Ok(Some(user))
}

/// Registers a kosync username against a reader token. A token registers once;
/// afterwards only that username authenticates. `Ok(false)` means the key is
/// not a reader token.
pub async fn register_sync(pool: &SqlitePool, key: &str, username: &str) -> Result<bool, AppError> {
    if key.is_empty() || username.is_empty() || username.contains(':') {
        return Ok(false);
    }
    let row: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT id, sync_user FROM reader_tokens WHERE sync_hash = ?")
            .bind(hash_token(key))
            .fetch_optional(pool)
            .await?;
    let Some((id, bound)) = row else {
        return Ok(false);
    };
    if bound.is_some() {
        return Err(AppError::Conflict("username already registered".into()));
    }
    let updated =
        sqlx::query("UPDATE reader_tokens SET sync_user = ? WHERE id = ? AND sync_user IS NULL")
            .bind(username)
            .bind(id)
            .execute(pool)
            .await?;
    if updated.rows_affected() == 0 {
        // Another device registered first; that username owns the account.
        return Err(AppError::Conflict("username already registered".into()));
    }
    Ok(true)
}

pub async fn list(pool: &SqlitePool, user_id: i64) -> Result<Vec<ReaderToken>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, name, created_at, last_used_at
         FROM reader_tokens WHERE user_id = ? ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

pub async fn revoke(pool: &SqlitePool, user_id: i64, id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM reader_tokens WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resolves an OPDS token to its user (disabled accounts are refused).
pub async fn authenticate(pool: &SqlitePool, token: &str) -> Result<Option<User>, AppError> {
    let row: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM reader_tokens WHERE token_hash = ?")
            .bind(hash_token(token))
            .fetch_optional(pool)
            .await?;
    let Some(user_id) = row else {
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
        "UPDATE reader_tokens SET last_used_at = unixepoch()
         WHERE token_hash = ? AND (last_used_at IS NULL OR last_used_at < unixepoch() - 300)",
    )
    .bind(hash_token(token))
    .execute(pool)
    .await
    .ok();

    Ok(Some(user))
}
