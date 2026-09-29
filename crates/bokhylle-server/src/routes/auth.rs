use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::{AuthUser, User, clear_cookie_header, session_cookie_token, set_cookie_header};
use crate::error::AppError;
use crate::routes::responses::{CreatedToken, Items, StatusJson, Tokens};
use crate::settings;

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginUser {
    username: String,
    display_name: Option<String>,
    role: crate::auth::Role,
    auth_mode: String,
    profile_type: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LoginUsersResponse {
    users: Vec<LoginUser>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LikedBook {
    book_id: i64,
    title: String,
    authors: Vec<String>,
    readable: bool,
    on_shelf: bool,
    provider: Option<String>,
    provider_key: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStats {
    shelf: i64,
    authors: i64,
    liked: i64,
    books_sent: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct OnboardingState {
    onboarded: bool,
    interests: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct OkResponse {
    ok: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub remember: Option<bool>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MeResponse {
    pub user: UserView,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    #[serde(flatten)]
    user: User,
    notification_email: Option<String>,
    email_notifications: bool,
    preferred_languages: Vec<String>,
    default_language: String,
    can_request: bool,
    can_discover: bool,
    can_acquire: bool,
    profile_type: String,
    shelf_finish: String,
    shelf_decorations: bool,
    spotlight_rotation: bool,
    avatar_version: Option<i64>,
}

pub(crate) async fn user_with_notifications(
    state: &AppState,
    user: &User,
) -> Result<UserView, AppError> {
    type Extras = (
        Option<String>,
        i64,
        String,
        Option<String>,
        i64,
        String,
        i64,
        i64,
        i64,
        Option<i64>,
    );
    let extras: Option<Extras> = sqlx::query_as(
        "SELECT notification_email, email_notifications, profile_type, preferred_languages,
                can_request, shelf_finish, shelf_decorations, spotlight_rotation,
                can_discover,
                CASE WHEN EXISTS (SELECT 1 FROM user_avatars WHERE user_id = users.id)
                     THEN avatar_version ELSE NULL END
         FROM users WHERE id = ?",
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;
    Ok(UserView {
        user: user.clone(),
        notification_email: extras.as_ref().and_then(|(email, ..)| email.clone()),
        email_notifications: extras
            .as_ref()
            .map(|(_, enabled, ..)| *enabled != 0)
            .unwrap_or(false),
        preferred_languages: extras
            .as_ref()
            .and_then(|(_, _, _, languages, ..)| languages.clone())
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default(),
        default_language: state
            .settings
            .get_string(settings::PREFERRED_LANGUAGE, "en")
            .await
            .unwrap_or_else(|_| "en".to_string()),
        can_request: extras
            .as_ref()
            .map(|(_, _, _, _, can_request, ..)| *can_request != 0)
            .unwrap_or(true),
        can_discover: extras
            .as_ref()
            .map(|(_, _, _, _, _, _, _, _, can_discover, _)| *can_discover != 0)
            .unwrap_or(false),
        can_acquire: crate::auth::can_acquire(&state.db, user).await?,
        profile_type: extras
            .as_ref()
            .map(|(_, _, profile, ..)| profile.clone())
            .unwrap_or_else(|| "adult".to_string()),
        shelf_finish: extras
            .as_ref()
            .map(|(_, _, _, _, _, finish, ..)| finish.clone())
            .unwrap_or_else(|| "oak".to_string()),
        shelf_decorations: extras
            .as_ref()
            .map(|(_, _, _, _, _, _, enabled, _, _, _)| *enabled != 0)
            .unwrap_or(true),
        spotlight_rotation: extras
            .as_ref()
            .map(|(_, _, _, _, _, _, _, enabled, _, _)| *enabled != 0)
            .unwrap_or(true),
        avatar_version: extras
            .as_ref()
            .and_then(|(_, _, _, _, _, _, _, _, _, version)| *version),
    })
}

const MAX_AVATAR_BYTES: usize = 1024 * 1024;

fn avatar_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") && bytes.ends_with(b"\xff\xd9") {
        Some("image/jpeg")
    } else if bytes.len() >= 16 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

pub async fn avatar(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let row: Option<(Vec<u8>, String)> =
        sqlx::query_as("SELECT data, mime FROM user_avatars WHERE user_id = ?")
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?;
    let Some((data, mime)) = row else {
        return Err(AppError::NotFound("profile picture not found".to_string()));
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        data,
    )
        .into_response())
}

pub async fn upload_avatar(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<StatusCode, AppError> {
    if bytes.is_empty() || bytes.len() > MAX_AVATAR_BYTES {
        return Err(AppError::Unprocessable(
            "profile picture must be 1 MB or less".to_string(),
        ));
    }
    let mime = avatar_mime(&bytes).ok_or_else(|| {
        AppError::Unprocessable("profile picture must be a PNG, JPEG, or WebP image".to_string())
    })?;
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some(mime)
    {
        return Err(AppError::Unprocessable(
            "profile picture type does not match the image".to_string(),
        ));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO user_avatars (user_id, data, mime) VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET data = excluded.data, mime = excluded.mime",
    )
    .bind(user.id)
    .bind(bytes.as_ref())
    .bind(mime)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE users SET avatar_version = avatar_version + 1 WHERE id = ?")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_avatar(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM user_avatars WHERE user_id = ?")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET avatar_version = avatar_version + 1 WHERE id = ?")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<
        axum::extract::Extension<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    >,
    Json(request): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let account = request.username.trim().to_ascii_lowercase();
    let trusted_proxy = state
        .settings
        .get_bool(settings::TRUSTED_PROXY, false)
        .await
        .unwrap_or(false);
    let peer = connect_info.map(|axum::extract::Extension(info)| info.0.ip());
    let ip = crate::auth::client_key(&headers, peer, trusted_proxy);

    if state.auth.login_blocked(&account, &ip).await? {
        return Err(AppError::RateLimited);
    }

    let Some(user) = state
        .auth
        .verify_login(&request.username, &request.password)
        .await?
    else {
        state.auth.register_login_failure(&account, &ip).await?;
        tracing::warn!(username = %account, "auth.login.failed");
        // Generic failure: do not reveal whether the account exists, is
        // disabled, or which credential type it uses.
        return Err(AppError::Unauthorized);
    };

    state.auth.clear_login_failures(&account).await?;
    let remembered = request.remember.unwrap_or(true);
    tracing::info!(user_id = user.id, username = %user.username, remembered, "auth.login");

    let token = state.auth.create_session(user.id, remembered).await?;
    let secure = state
        .settings
        .get_bool(settings::SECURE_COOKIES, false)
        .await?;

    Ok((
        [(
            header::SET_COOKIE,
            set_cookie_header(&token, secure, remembered),
        )],
        Json(MeResponse {
            user: user_with_notifications(&state, &user).await?,
        }),
    )
        .into_response())
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Some(token) = session_cookie_token(&headers) {
        state.auth.delete_session(&token).await?;
    }

    Ok((
        [(header::SET_COOKIE, clear_cookie_header())],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

pub async fn me(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<MeResponse>, AppError> {
    Ok(Json(MeResponse {
        user: user_with_notifications(&state, &user).await?,
    }))
}

pub async fn login_users(
    State(state): State<AppState>,
) -> Result<Json<LoginUsersResponse>, AppError> {
    let users = state.auth.list_users().await?;
    let mut items = Vec::new();
    for (user, _, disabled) in users {
        if disabled {
            continue;
        }
        let row: Option<(Option<String>, Option<String>)> =
            sqlx::query_as("SELECT credential_type, profile_type FROM users WHERE id = ?")
                .bind(user.id)
                .fetch_optional(&state.db)
                .await?;
        let (auth_mode, profile_type) = row.unwrap_or((None, None));
        items.push(LoginUser {
            username: user.username,
            display_name: user.display_name,
            role: user.role,
            auth_mode: auth_mode.unwrap_or_else(|| "legacy".to_string()),
            profile_type: profile_type.unwrap_or_else(|| "adult".to_string()),
        });
    }

    Ok(Json(LoginUsersResponse { users: items }))
}

/// Taste signals can point at metadata-only books, so this list never
/// requires files: it returns what the user liked and whether a readable
/// household copy exists.
type LikedBookRow = (i64, String, bool, bool, Option<String>, Option<String>);

pub async fn liked_books(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Items<LikedBook>>, AppError> {
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let rows: Vec<LikedBookRow> = sqlx::query_as(
        "SELECT b.id, b.title,
                EXISTS (
                    SELECT 1 FROM book_files f
                    JOIN editions e ON e.id = f.edition_id
                    WHERE e.book_id = b.id
                ) AS readable,
                EXISTS (
                    SELECT 1 FROM user_books mine
                    WHERE mine.user_id = ub.user_id
                      AND mine.book_id = b.id
                      AND mine.on_shelf = 1
                ) AS on_shelf,
                (SELECT e.provider FROM editions e
                 WHERE e.book_id = b.id AND e.provider IS NOT NULL LIMIT 1) AS provider,
                (SELECT e.provider_key FROM editions e
                 WHERE e.book_id = b.id AND e.provider_key IS NOT NULL LIMIT 1) AS provider_key
         FROM user_books ub
         JOIN books b ON b.id = ub.book_id
         WHERE ub.user_id = ? AND ub.preference = 'liked'
           AND (? = 0 OR ub.on_shelf = 1)
         ORDER BY ub.added_at DESC, b.id DESC",
    )
    .bind(user.id)
    .bind(i64::from(child))
    .fetch_all(&state.db)
    .await?;

    let mut items: Vec<LikedBook> = Vec::with_capacity(rows.len());
    for (book_id, title, readable, on_shelf, provider, provider_key) in rows {
        let authors: Vec<String> = sqlx::query_scalar(
            "SELECT a.name FROM book_authors ba
             JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = ?
             ORDER BY ba.position",
        )
        .bind(book_id)
        .fetch_all(&state.db)
        .await?;
        items.push(LikedBook {
            book_id,
            title,
            authors,
            readable,
            on_shelf,
            provider,
            provider_key,
        });
    }

    Ok(Json(Items { items }))
}

/// Personal overview counts. A successful delivery of the same book to
/// several destinations is still one book sent, and historical deliveries
/// without a profile owner cannot be attributed to this profile.
pub async fn profile_stats(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<ProfileStats>, AppError> {
    let child = crate::auth::profile_type(&state.db, user.id).await? == "child";
    let (shelf, authors, liked, books_sent): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT count(*) FROM user_books ub
             WHERE ub.user_id = ? AND ub.on_shelf = 1
               AND EXISTS (
                 SELECT 1 FROM book_files f
                 JOIN editions e ON e.id = f.edition_id
                 WHERE e.book_id = ub.book_id
               )),
            (SELECT count(*) FROM author_follows WHERE user_id = ?),
            (SELECT count(*) FROM user_books ub
             WHERE ub.user_id = ? AND ub.preference = 'liked'
               AND (? = 0 OR ub.on_shelf = 1)),
            (SELECT count(DISTINCT book_id) FROM deliveries
             WHERE user_id = ? AND status = 'SENT')",
    )
    .bind(user.id)
    .bind(user.id)
    .bind(user.id)
    .bind(i64::from(child))
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(ProfileStats {
        shelf,
        authors,
        liked,
        books_sent,
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CredentialChange {
    pub current: String,
    pub credential_type: String,
    pub credential: String,
}

pub async fn change_credential(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CredentialChange>,
) -> Result<Response, AppError> {
    let remembered = match session_cookie_token(&headers) {
        Some(token) => {
            let token_hash = crate::auth::hash_token(&token);
            sqlx::query_scalar::<_, i64>("SELECT remembered FROM sessions WHERE token_hash = ?")
                .bind(token_hash)
                .fetch_optional(&state.db)
                .await?
                .unwrap_or(1)
                != 0
        }
        None => true,
    };
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT password_hash, credential_type, credential_version FROM users WHERE id = ?",
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;

    let Some((hash, credential_type, credential_version)) = row else {
        return Err(AppError::NotFound("user not found".to_string()));
    };

    if !crate::auth::verify_credential(&credential_type, credential_version, &hash, &body.current) {
        return Err(AppError::Unauthorized);
    }

    if matches!(user.role, crate::auth::Role::Admin) && body.credential_type != "password" {
        return Err(AppError::Unprocessable(
            "administrators must use a password".to_string(),
        ));
    }
    crate::auth::validate_credential(&body.credential_type, &body.credential)?;
    let new_hash = crate::auth::hash_credential(&body.credential_type, &body.credential)?;

    sqlx::query(
        "UPDATE users SET password_hash = ?, credential_type = ?, credential_version = 2
         WHERE id = ?",
    )
    .bind(new_hash)
    .bind(&body.credential_type)
    .bind(user.id)
    .execute(&state.db)
    .await?;

    // Rotate every session, including this one: revoke all, issue a fresh one
    // with the same remember-this-device setting as the current session.
    state.auth.delete_all_sessions(user.id).await?;
    let token = state.auth.create_session(user.id, remembered).await?;
    let secure = state
        .settings
        .get_bool(settings::SECURE_COOKIES, false)
        .await?;
    let refreshed = state
        .auth
        .user_by_id(user.id)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    tracing::info!(user_id = user.id, "auth.credential.changed");

    Ok((
        [(
            header::SET_COOKIE,
            set_cookie_header(&token, secure, remembered),
        )],
        Json(MeResponse {
            user: user_with_notifications(&state, &refreshed).await?,
        }),
    )
        .into_response())
}

pub async fn logout_all(AuthUser(user): AuthUser, State(state): State<AppState>) -> Response {
    let _ = state.auth.delete_all_sessions(user.id).await;

    (
        [(header::SET_COOKIE, clear_cookie_header())],
        StatusCode::NO_CONTENT,
    )
        .into_response()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUpdate {
    pub display_name: Option<String>,
    pub preferred_format: Option<String>,
    pub preferred_language: Option<String>,
    pub preferred_languages: Option<Vec<String>>,
    pub acquisition_mode: Option<String>,
    pub notification_email: Option<String>,
    pub email_notifications: Option<bool>,
    pub shelf_finish: Option<String>,
    pub shelf_decorations: Option<bool>,
    pub spotlight_rotation: Option<bool>,
}

fn clean(value: Option<String>) -> Option<Option<String>> {
    value.map(|value| {
        let value = value.trim().to_string();
        if value.is_empty() { None } else { Some(value) }
    })
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InterestsInput {
    #[serde(default)]
    pub subjects: Vec<String>,
}

pub async fn onboarding_state(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<OnboardingState>, AppError> {
    let onboarded: Option<i64> = sqlx::query_scalar("SELECT onboarded_at FROM users WHERE id = ?")
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?
        .flatten();
    let interests: Vec<String> = sqlx::query_scalar(
        "SELECT normalized_name FROM user_subject_interests WHERE user_id = ? ORDER BY created_at",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(OnboardingState {
        onboarded: onboarded.is_some(),
        interests,
    }))
}

/// Onboarding: explicit subject interests, stored separately from hidden
/// subject preferences so either can change without touching the other.
pub async fn update_interests(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<InterestsInput>,
) -> Result<Json<OkResponse>, AppError> {
    sqlx::query("DELETE FROM user_subject_interests WHERE user_id = ?")
        .bind(user.id)
        .execute(&state.db)
        .await?;
    for subject in body.subjects.iter().take(24) {
        let normalized = bokhylle_core::identity::normalize_text(subject.trim());
        if normalized.is_empty() || normalized.len() > 60 {
            continue;
        }
        let _ = sqlx::query(
            "INSERT INTO user_subject_interests (user_id, normalized_name)
             VALUES (?, ?)
             ON CONFLICT(user_id, normalized_name) DO NOTHING",
        )
        .bind(user.id)
        .bind(&normalized)
        .execute(&state.db)
        .await;
    }
    Ok(Json(OkResponse { ok: true }))
}

pub async fn complete_onboarding(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<StatusCode, AppError> {
    sqlx::query("UPDATE users SET onboarded_at = unixepoch() WHERE id = ?")
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_profile(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<ProfileUpdate>,
) -> Result<Json<MeResponse>, AppError> {
    let display_name = match clean(body.display_name) {
        Some(value) => value,
        None => user.display_name.clone(),
    };
    let preferred_format = match clean(body.preferred_format) {
        Some(value) => value,
        None => user.preferred_format.clone(),
    };
    let preferred_language = match clean(body.preferred_language) {
        Some(value) => value,
        None => user.preferred_language.clone(),
    };
    let acquisition_mode = body
        .acquisition_mode
        .or(Some(user.acquisition_mode.clone()));
    let preferred_languages: Option<Vec<String>> = body.preferred_languages.map(|languages| {
        languages
            .into_iter()
            .map(|language| language.trim().to_ascii_lowercase())
            .filter(|language| !language.is_empty() && language.len() <= 8)
            .take(8)
            .collect()
    });

    if let Some(name) = &display_name
        && name.chars().count() > 80
    {
        return Err(AppError::Unprocessable(
            "display name must be 80 characters or fewer".to_string(),
        ));
    }

    // The first language of the set is the default when no explicit primary
    // was sent; clearing the set clears the primary too.
    let preferred_language = match (&preferred_languages, preferred_language) {
        (Some(languages), None) => languages.first().cloned(),
        (_, value) => value,
    };

    // Validate the complete payload before any write so a 422 can never leave
    // a partially updated profile behind.
    if let Some(finish) = &body.shelf_finish
        && !matches!(finish.as_str(), "oak" | "black" | "metal")
    {
        return Err(AppError::Unprocessable(
            "shelf finish must be 'oak', 'black', or 'metal'".to_string(),
        ));
    }
    let mode = acquisition_mode.unwrap_or_else(|| user.acquisition_mode.clone());
    if !matches!(mode.as_str(), "automatic" | "ask") {
        return Err(AppError::Unprocessable(
            "acquisition mode must be 'automatic' or 'ask'".to_string(),
        ));
    }
    let current: (Option<String>, i64) =
        sqlx::query_as("SELECT notification_email, email_notifications FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(&state.db)
            .await?;
    let notification_email = match clean(body.notification_email) {
        Some(value) => value,
        None => current.0,
    };
    if let Some(email) = &notification_email
        && (email.len() > 200 || !email.contains('@') || email.contains(char::is_whitespace))
    {
        return Err(AppError::Unprocessable(
            "notification email must be a valid address".to_string(),
        ));
    }
    let email_notifications = body.email_notifications.unwrap_or(current.1 != 0);

    let mut tx = state.db.begin().await?;
    sqlx::query(
        "UPDATE users SET display_name = ?, preferred_format = ?, preferred_language = ?,
                acquisition_mode = ?, notification_email = ?, email_notifications = ?,
                shelf_finish = COALESCE(?, shelf_finish),
                shelf_decorations = COALESCE(?, shelf_decorations),
                spotlight_rotation = COALESCE(?, spotlight_rotation)
         WHERE id = ?",
    )
    .bind(&display_name)
    .bind(&preferred_format)
    .bind(&preferred_language)
    .bind(&mode)
    .bind(&notification_email)
    .bind(i64::from(email_notifications))
    .bind(&body.shelf_finish)
    .bind(body.shelf_decorations.map(i64::from))
    .bind(body.spotlight_rotation.map(i64::from))
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    if let Some(languages) = &preferred_languages {
        let stored = if languages.is_empty() {
            None
        } else {
            Some(serde_json::to_string(languages).unwrap_or_default())
        };
        sqlx::query("UPDATE users SET preferred_languages = ? WHERE id = ?")
            .bind(stored)
            .bind(user.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    let updated = state
        .auth
        .user_by_id(user.id)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    Ok(Json(MeResponse {
        user: user_with_notifications(&state, &updated).await?,
    }))
}

pub async fn list_reader_tokens(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Tokens<crate::reader_tokens::ReaderToken>>, AppError> {
    let tokens = crate::reader_tokens::list(&state.db, user.id).await?;
    Ok(Json(Tokens { tokens }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TokenInput {
    pub name: Option<String>,
}

pub async fn create_reader_token(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<TokenInput>,
) -> Result<StatusJson<CreatedToken, 201>, AppError> {
    let (id, token) =
        crate::reader_tokens::create(&state.db, user.id, body.name.as_deref().unwrap_or("Reader"))
            .await?;
    Ok(StatusJson(CreatedToken { id, token }))
}

pub async fn revoke_reader_token(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    crate::reader_tokens::revoke(&state.db, user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
