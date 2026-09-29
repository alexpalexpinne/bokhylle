use std::time::{Duration, SystemTime, UNIX_EPOCH};

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use axum::extract::FromRequestParts;
use axum::http::HeaderMap;
use axum::http::header;
use axum::http::request::Parts;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::AppState;
use crate::error::AppError;

pub const SESSION_COOKIE: &str = "bokhylle_session";
/// Absolute lifetime of a normal (not remembered) session.
pub const SESSION_TTL: Duration = Duration::from_secs(60 * 60 * 12);
/// Idle lifetime of a remembered device session, refreshed on use.
pub const REMEMBERED_IDLE_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 90);
/// Absolute lifetime of a remembered device session.
pub const REMEMBERED_ABSOLUTE_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 365);
/// A remembered session refreshes its idle deadline when less than this remains.
pub const ROLLING_WINDOW: Duration = Duration::from_secs(60 * 60 * 24 * 5);
pub const MAX_ACCOUNT_FAILURES: i64 = 10;
pub const MAX_IP_FAILURES: i64 = 30;

const PIN_BLOCKLIST: [&str; 10] = [
    "000000", "111111", "112233", "121212", "123123", "123321", "123456", "654321", "666666",
    "696969",
];

const WEAK_BOOTSTRAP_PASSWORDS: [&str; 6] = [
    "change-me",
    "changeme",
    "password",
    "admin",
    "bokhylle",
    "secret",
];

pub fn bootstrap_password_acceptable(password: &str) -> bool {
    let trimmed = password.trim();
    trimmed.len() >= 8 && !WEAK_BOOTSTRAP_PASSWORDS.contains(&trimmed.to_ascii_lowercase().as_str())
}

#[cfg(test)]
mod bootstrap_password_tests {
    use super::bootstrap_password_acceptable;

    #[test]
    fn rejects_missing_short_and_default_passwords() {
        assert!(!bootstrap_password_acceptable(""));
        assert!(!bootstrap_password_acceptable("short"));
        assert!(!bootstrap_password_acceptable("change-me"));
        assert!(!bootstrap_password_acceptable("Change-Me"));
        assert!(!bootstrap_password_acceptable("password"));
        assert!(bootstrap_password_acceptable(
            "correct horse battery staple"
        ));
        assert!(bootstrap_password_acceptable("s3cret-passphrase"));
    }
}
type UserRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);
type UserWithHashRow = (
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    String,
    i64,
);
type SessionUserRow = (
    i64,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
    i64,
);
type AdminUserRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    fn from_db(value: &str) -> Self {
        match value {
            "admin" => Self::Admin,
            _ => Self::User,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub role: Role,
    pub preferred_format: Option<String>,
    pub preferred_language: Option<String>,
    pub acquisition_mode: String,
}

/// `adult` or `child`; authorization stays in `role`, this is the profile
/// experience (children see only their shelf and cannot download or acquire).
pub async fn profile_type(pool: &SqlitePool, user_id: i64) -> Result<String, AppError> {
    // No row means the session outlived the user; callers treat this as an
    // error rather than silently assuming the most permissive profile.
    sqlx::query_scalar("SELECT profile_type FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(AppError::from)
}

/// Adding a new file changes the shared collection. Admins always retain
/// this ability; other adults may be limited to books already held.
pub async fn can_acquire(pool: &SqlitePool, user: &User) -> Result<bool, AppError> {
    let (profile, allowed): (String, i64) =
        sqlx::query_as("SELECT profile_type, can_acquire FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(pool)
            .await?;
    Ok(profile == "adult" && (user.role == Role::Admin || allowed != 0))
}

#[derive(Clone)]
pub struct Auth {
    db: SqlitePool,
}

impl Auth {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    pub async fn count_users(&self) -> Result<i64, AppError> {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&self.db)
            .await?;
        Ok(count)
    }

    pub async fn create_user(
        &self,
        username: &str,
        secret: &str,
        role: Role,
    ) -> Result<User, AppError> {
        let credential_type = infer_credential_type(role, secret);
        self.create_user_with_type(username, secret, role, credential_type)
            .await
    }

    pub async fn create_user_with_type(
        &self,
        username: &str,
        secret: &str,
        role: Role,
        credential_type: &str,
    ) -> Result<User, AppError> {
        self.create_user_with_profile(username, secret, role, credential_type, "adult")
            .await
    }

    pub async fn create_user_with_profile(
        &self,
        username: &str,
        secret: &str,
        role: Role,
        credential_type: &str,
        profile_type: &str,
    ) -> Result<User, AppError> {
        let username = username.trim();
        if username.is_empty() {
            return Err(AppError::Unprocessable("username must not be empty".into()));
        }
        if !matches!(profile_type, "adult" | "child") {
            return Err(AppError::Unprocessable(
                "profile type must be 'adult' or 'child'".into(),
            ));
        }
        if matches!(role, Role::Admin) && profile_type == "child" {
            return Err(AppError::Unprocessable(
                "a child profile cannot be an administrator".into(),
            ));
        }
        if matches!(role, Role::Admin) && credential_type != "password" {
            return Err(AppError::Unprocessable(
                "administrators must use a password".into(),
            ));
        }
        validate_credential(credential_type, secret)?;

        let password_hash = hash_credential(credential_type, secret)?;
        let role_value = match role {
            Role::Admin => "admin",
            Role::User => "user",
        };

        let result = sqlx::query(
            "INSERT INTO users
                (username, password_hash, role, credential_type, credential_version, profile_type, onboarded_at)
             VALUES (?, ?, ?, ?, 2, ?, NULL)",
        )
        .bind(username)
        .bind(password_hash)
        .bind(role_value)
        .bind(credential_type)
        .bind(profile_type)
        .execute(&self.db)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(ref database_error) if database_error.is_unique_violation() => {
                AppError::Conflict(format!("user '{username}' already exists"))
            }
            other => AppError::Internal(other),
        })?;

        Ok(User {
            id: result.last_insert_rowid(),
            username: username.to_string(),
            display_name: None,
            role,
            preferred_format: None,
            preferred_language: None,
            acquisition_mode: "automatic".to_string(),
        })
    }

    pub async fn verify_login(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<User>, AppError> {
        let row: Option<UserWithHashRow> = sqlx::query_as(
            "SELECT id, username, password_hash, role, display_name, preferred_format,
                    preferred_language, acquisition_mode, credential_type, credential_version
             FROM users WHERE username = ? AND disabled = 0",
        )
        .bind(username.trim())
        .fetch_optional(&self.db)
        .await?;

        let Some((
            id,
            username,
            password_hash,
            role,
            display_name,
            preferred_format,
            preferred_language,
            acquisition_mode,
            credential_type,
            credential_version,
        )) = row
        else {
            return Ok(None);
        };

        if !verify_credential(
            &credential_type,
            credential_version,
            &password_hash,
            password,
        ) {
            return Ok(None);
        }

        // Upgrade legacy hashes to the typed, prefixed format on first login.
        if credential_version < 2
            && let Ok(hash) = hash_credential(&credential_type, password)
        {
            sqlx::query("UPDATE users SET password_hash = ?, credential_version = 2 WHERE id = ?")
                .bind(hash)
                .bind(id)
                .execute(&self.db)
                .await
                .ok();
        }

        Ok(Some(User {
            id,
            username,
            display_name,
            role: Role::from_db(&role),
            preferred_format,
            preferred_language,
            acquisition_mode,
        }))
    }

    pub async fn create_session(&self, user_id: i64, remembered: bool) -> Result<String, AppError> {
        let token = generate_token()?;
        let token_hash = hash_token(&token);
        let now = now_epoch();
        let (expires_at, absolute_expires_at) = if remembered {
            (
                now + REMEMBERED_IDLE_TTL.as_secs() as i64,
                now + REMEMBERED_ABSOLUTE_TTL.as_secs() as i64,
            )
        } else {
            let deadline = now + SESSION_TTL.as_secs() as i64;
            (deadline, deadline)
        };

        sqlx::query(
            "INSERT INTO sessions (user_id, token_hash, expires_at, remembered, last_seen_at,
                    absolute_expires_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(token_hash)
        .bind(expires_at)
        .bind(remembered)
        .bind(now)
        .bind(absolute_expires_at)
        .execute(&self.db)
        .await?;

        Ok(token)
    }

    pub async fn delete_all_sessions(&self, user_id: i64) -> Result<(), AppError> {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn user_for_token(&self, token: &str) -> Result<Option<User>, AppError> {
        let token_hash = hash_token(token);

        let row: Option<SessionUserRow> = sqlx::query_as(
            "SELECT u.id, u.username, u.role, s.expires_at, u.display_name,
                    u.preferred_format, u.preferred_language, u.acquisition_mode,
                    s.remembered, s.absolute_expires_at, s.last_seen_at
             FROM sessions s
             JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ? AND s.expires_at > unixepoch()
               AND (s.absolute_expires_at = 0 OR s.absolute_expires_at > unixepoch())
               AND u.disabled = 0",
        )
        .bind(&token_hash)
        .fetch_optional(&self.db)
        .await?;

        let Some((
            id,
            username,
            role,
            expires_at,
            display_name,
            preferred_format,
            preferred_language,
            acquisition_mode,
            remembered,
            absolute_expires_at,
            last_seen_at,
        )) = row
        else {
            return Ok(None);
        };

        let now = now_epoch();
        if remembered != 0 && absolute_expires_at > 0 {
            let idle_deadline = now + REMEMBERED_IDLE_TTL.as_secs() as i64;
            let refreshed = idle_deadline.min(absolute_expires_at);
            if refreshed > expires_at && expires_at - now < ROLLING_WINDOW.as_secs() as i64 {
                sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
                    .bind(refreshed)
                    .bind(&token_hash)
                    .execute(&self.db)
                    .await?;
            }
        }
        // Throttle activity writes to avoid a write transaction per request.
        if now - last_seen_at > 300 {
            sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?")
                .bind(now)
                .bind(&token_hash)
                .execute(&self.db)
                .await?;
        }

        Ok(Some(User {
            id,
            username,
            display_name,
            role: Role::from_db(&role),
            preferred_format,
            preferred_language,
            acquisition_mode,
        }))
    }

    pub async fn update_profile(
        &self,
        user_id: i64,
        display_name: Option<String>,
        preferred_format: Option<String>,
        preferred_language: Option<String>,
        acquisition_mode: Option<String>,
    ) -> Result<Option<User>, AppError> {
        if let Some(mode) = &acquisition_mode
            && !matches!(mode.as_str(), "automatic" | "ask")
        {
            return Err(AppError::Unprocessable(
                "acquisition mode must be 'automatic' or 'ask'".to_string(),
            ));
        }

        let mode = acquisition_mode.unwrap_or_else(|| "automatic".to_string());

        sqlx::query(
            "UPDATE users SET display_name = ?, preferred_format = ?, preferred_language = ?,
                    acquisition_mode = ?
             WHERE id = ?",
        )
        .bind(&display_name)
        .bind(&preferred_format)
        .bind(&preferred_language)
        .bind(&mode)
        .bind(user_id)
        .execute(&self.db)
        .await?;

        let row: Option<UserRow> = sqlx::query_as(
            "SELECT id, username, role, display_name, preferred_format, preferred_language,
                    acquisition_mode
             FROM users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(
            |(id, username, role, display_name, preferred_format, preferred_language, mode)| User {
                id,
                username,
                display_name,
                role: Role::from_db(&role),
                preferred_format,
                preferred_language,
                acquisition_mode: mode,
            },
        ))
    }

    pub async fn list_users(&self) -> Result<Vec<(User, i64, bool)>, AppError> {
        let rows: Vec<AdminUserRow> = sqlx::query_as(
            "SELECT u.id, u.username, u.role, u.display_name, u.preferred_format,
                    u.preferred_language, u.acquisition_mode, u.disabled,
                    (SELECT count(*) FROM delivery_targets t WHERE t.user_id = u.id)
             FROM users u
             ORDER BY u.username COLLATE NOCASE",
        )
        .fetch_all(&self.db)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    username,
                    role,
                    display_name,
                    preferred_format,
                    preferred_language,
                    acquisition_mode,
                    disabled,
                    readers,
                )| {
                    (
                        User {
                            id,
                            username,
                            display_name,
                            role: Role::from_db(&role),
                            preferred_format,
                            preferred_language,
                            acquisition_mode,
                        },
                        readers,
                        disabled != 0,
                    )
                },
            )
            .collect())
    }

    pub async fn update_user_admin(
        &self,
        user_id: i64,
        display_name: Option<String>,
        role: Option<Role>,
        credential: Option<(String, String)>,
        disabled: Option<bool>,
    ) -> Result<Option<User>, AppError> {
        let Some(existing) = self.user_by_id(user_id).await? else {
            return Ok(None);
        };
        let current_type: Option<String> =
            sqlx::query_scalar("SELECT credential_type FROM users WHERE id = ?")
                .bind(user_id)
                .fetch_optional(&self.db)
                .await?
                .flatten();

        let effective_role = role.unwrap_or(existing.role);
        if let Some((credential_type, secret)) = &credential {
            validate_credential(credential_type, secret)?;
            if matches!(effective_role, Role::Admin) && credential_type != "password" {
                return Err(AppError::Unprocessable(
                    "administrators must use a password".to_string(),
                ));
            }
        } else if matches!(effective_role, Role::Admin)
            && current_type.as_deref() != Some("password")
        {
            return Err(AppError::Unprocessable(
                "set a password when promoting this account to administrator".to_string(),
            ));
        }

        let role_value = match effective_role {
            Role::Admin => "admin",
            Role::User => "user",
        };
        let existing_disabled: i64 = sqlx::query_scalar("SELECT disabled FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&self.db)
            .await?;
        let (password_hash, credential_type) = match &credential {
            Some((credential_type, secret)) => (
                Some(hash_credential(credential_type, secret)?),
                Some(credential_type.clone()),
            ),
            None => (None, None),
        };
        let disabled = disabled.unwrap_or(existing_disabled != 0);
        let credential_marker = password_hash.as_ref().map(|_| 0i64);

        sqlx::query(
            "UPDATE users SET display_name = COALESCE(?, display_name), role = ?,
                    password_hash = COALESCE(?, password_hash),
                    credential_type = COALESCE(?, credential_type),
                    credential_version = CASE WHEN ? IS NULL THEN credential_version ELSE 2 END,
                    disabled = ?
             WHERE id = ?",
        )
        .bind(&display_name)
        .bind(role_value)
        .bind(&password_hash)
        .bind(&credential_type)
        .bind(credential_marker)
        .bind(disabled)
        .bind(user_id)
        .execute(&self.db)
        .await?;

        if credential.is_some() || disabled {
            self.delete_all_sessions(user_id).await?;
        }

        self.user_by_id(user_id).await
    }

    pub async fn user_by_id(&self, user_id: i64) -> Result<Option<User>, AppError> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT id, username, role, display_name, preferred_format, preferred_language,
                    acquisition_mode
             FROM users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(
            |(id, username, role, display_name, preferred_format, preferred_language, mode)| User {
                id,
                username,
                display_name,
                role: Role::from_db(&role),
                preferred_format,
                preferred_language,
                acquisition_mode: mode,
            },
        ))
    }

    pub async fn login_blocked(&self, account: &str, ip: &str) -> Result<bool, AppError> {
        let account_failures: i64 = sqlx::query_scalar(
            "SELECT failures FROM login_attempts
             WHERE scope = 'account' AND key = ? AND window_start > unixepoch() - 900",
        )
        .bind(account)
        .fetch_optional(&self.db)
        .await?
        .unwrap_or(0);

        if account_failures >= MAX_ACCOUNT_FAILURES {
            return Ok(true);
        }

        let ip_failures: i64 = sqlx::query_scalar(
            "SELECT failures FROM login_attempts
             WHERE scope = 'ip' AND key = ? AND window_start > unixepoch() - 900",
        )
        .bind(ip)
        .fetch_optional(&self.db)
        .await?
        .unwrap_or(0);

        Ok(ip_failures >= MAX_IP_FAILURES)
    }

    pub async fn register_login_failure(&self, account: &str, ip: &str) -> Result<(), AppError> {
        for (scope, key) in [("account", account), ("ip", ip)] {
            sqlx::query(
                "INSERT INTO login_attempts (scope, key, failures, window_start)
                 VALUES (?, ?, 1, unixepoch())
                 ON CONFLICT(scope, key) DO UPDATE SET
                     failures = CASE
                         WHEN login_attempts.window_start <= unixepoch() - 900 THEN 1
                         ELSE login_attempts.failures + 1 END,
                     window_start = CASE
                         WHEN login_attempts.window_start <= unixepoch() - 900 THEN unixepoch()
                         ELSE login_attempts.window_start END",
            )
            .bind(scope)
            .bind(key)
            .execute(&self.db)
            .await?;
        }
        Ok(())
    }

    pub async fn clear_login_failures(&self, account: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM login_attempts WHERE scope = 'account' AND key = ?")
            .bind(account)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn delete_session(&self, token: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(hash_token(token))
            .execute(&self.db)
            .await?;
        Ok(())
    }

    pub async fn delete_expired_sessions(&self) -> Result<u64, AppError> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= unixepoch()")
            .execute(&self.db)
            .await?;
        Ok(result.rows_affected())
    }
}

pub struct AuthUser(pub User);

impl aide::operation::OperationInput for AuthUser {}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = session_cookie_token(&parts.headers).ok_or(AppError::Unauthorized)?;
        let user = state
            .auth
            .user_for_token(&token)
            .await?
            .ok_or(AppError::Unauthorized)?;
        Ok(AuthUser(user))
    }
}

pub struct AdminUser(pub User);

impl aide::operation::OperationInput for AdminUser {}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let AuthUser(user) = AuthUser::from_request_parts(parts, state).await?;
        if user.role != Role::Admin {
            return Err(AppError::Forbidden);
        }
        Ok(AdminUser(user))
    }
}

pub fn session_cookie_token(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;

    cookies
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.to_string())
}

pub fn set_cookie_header(token: &str, secure: bool, remembered: bool) -> String {
    let secure_suffix = if secure { "; Secure" } else { "" };
    if remembered {
        format!(
            "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure_suffix}",
            REMEMBERED_ABSOLUTE_TTL.as_secs()
        )
    } else {
        // A browser-session cookie: closing the browser ends the session.
        format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax{secure_suffix}")
    }
}

pub fn clear_cookie_header() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub fn infer_credential_type(role: Role, secret: &str) -> &'static str {
    match role {
        Role::Admin => "password",
        Role::User => {
            if secret.len() == 6 && secret.chars().all(|character| character.is_ascii_digit()) {
                "pin"
            } else {
                "password"
            }
        }
    }
}

pub fn validate_credential(credential_type: &str, secret: &str) -> Result<(), AppError> {
    match credential_type {
        "pin" => {
            if secret.len() != 6 || !secret.chars().all(|character| character.is_ascii_digit()) {
                return Err(AppError::Unprocessable(
                    "PIN must be exactly 6 digits".to_string(),
                ));
            }
            if PIN_BLOCKLIST.contains(&secret) {
                return Err(AppError::Unprocessable(
                    "that PIN is too easy to guess".to_string(),
                ));
            }
            Ok(())
        }
        "password" => {
            let length = secret.chars().count();
            if !(8..=128).contains(&length) {
                return Err(AppError::Unprocessable(
                    "password must be at least 8 characters".to_string(),
                ));
            }
            Ok(())
        }
        _ => Err(AppError::Unprocessable(
            "credential type must be 'pin' or 'password'".to_string(),
        )),
    }
}

pub fn hash_credential(credential_type: &str, secret: &str) -> Result<String, AppError> {
    hash_password(&format!("{credential_type}:{secret}"))
}

pub fn verify_credential(
    credential_type: &str,
    credential_version: i64,
    stored_hash: &str,
    secret: &str,
) -> bool {
    let material = if credential_version >= 2 {
        format!("{credential_type}:{secret}")
    } else {
        secret.to_string()
    };
    verify_password(&material, stored_hash)
}

/// Raw argon2 hash (no credential prefix); used for legacy rows in tests.
pub fn hash_password(password: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|hash| hash.to_string())
        .map_err(|_| AppError::Crypto)
}

fn verify_password(password: &str, stored_hash: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), stored_hash)
        .is_ok()
}

/// The rate-limit key. Forwarding headers are trusted only when the operator
/// has declared a trusted proxy; otherwise a directly connected client could
/// spoof them, so the peer address is authoritative.
pub fn client_key(
    headers: &HeaderMap,
    peer: Option<std::net::IpAddr>,
    trusted_proxy: bool,
) -> String {
    if trusted_proxy {
        for name in ["x-forwarded-for", "x-real-ip"] {
            if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok())
                && let Some(first) = value.split(',').next()
                && !first.trim().is_empty()
            {
                return first.trim().to_ascii_lowercase();
            }
        }
    }
    match peer {
        Some(peer) => peer.to_string(),
        None => "local".to_string(),
    }
}

fn generate_token() -> Result<String, AppError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| AppError::Crypto)?;
    Ok(hex::encode(bytes))
}

pub(crate) fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
