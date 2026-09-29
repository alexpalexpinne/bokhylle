use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path as RoutePath, Request, State};
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::AppState;
use crate::auth::{AuthUser, Role, client_key, set_cookie_header};
use crate::book_requests;
use crate::error::AppError;
use crate::notifications;
use crate::settings;
use crate::user_books;

const MARKER: &str = "bokhylle-demo-v1\n";
const MAX_VISITORS: i64 = 500;
const MAX_ENTRIES_PER_IP: usize = 5;
const MAX_MUTATIONS_PER_IP: usize = 120;
const MAX_POSITION_SAVES_PER_IP: usize = 1800;
const ENTRY_WINDOW: Duration = Duration::from_secs(60 * 60);

#[derive(Serialize, schemars::JsonSchema)]
pub struct DemoStatus {
    enabled: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DemoGetStarted {
    id: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DemoSendResult {
    id: i64,
    status: &'static str,
    message: &'static str,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DemoGetActivity {
    id: String,
    book_id: i64,
    title: String,
    status: &'static str,
    started_at: i64,
    ready_at: i64,
    completed_at: Option<i64>,
    send_when_ready: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DemoSendActivity {
    id: i64,
    book_id: i64,
    title: String,
    created_at: i64,
    status: &'static str,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DemoActivity {
    gets: Vec<DemoGetActivity>,
    sends: Vec<DemoSendActivity>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct DemoRequestResult {
    request: Option<book_requests::BookRequestView>,
}

#[derive(Default)]
pub struct DemoState {
    entries: Mutex<HashMap<String, Vec<Instant>>>,
    mutations: Mutex<HashMap<String, Vec<Instant>>>,
    position_saves: Mutex<HashMap<String, Vec<Instant>>>,
    entry_lock: tokio::sync::Mutex<()>,
}

pub fn pair_names(username: &str) -> Option<(String, String)> {
    let suffix = username
        .strip_prefix("demo_adult_")
        .or_else(|| username.strip_prefix("demo_child_"))?;
    if suffix.len() != 32 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some((
        format!("demo_adult_{suffix}"),
        format!("demo_child_{suffix}"),
    ))
}

impl DemoState {
    fn admit(&self, key: &str) -> bool {
        admit(&self.entries, key, MAX_ENTRIES_PER_IP)
    }

    fn admit_mutation(&self, key: &str) -> bool {
        admit(&self.mutations, key, MAX_MUTATIONS_PER_IP)
    }

    fn admit_position_save(&self, key: &str) -> bool {
        admit(&self.position_saves, key, MAX_POSITION_SAVES_PER_IP)
    }
}

fn admit(entries: &Mutex<HashMap<String, Vec<Instant>>>, key: &str, limit: usize) -> bool {
    let now = Instant::now();
    let mut entries = entries.lock().expect("demo rate limit lock");
    entries.retain(|_, times| {
        times.retain(|time| now.duration_since(*time) < ENTRY_WINDOW);
        !times.is_empty()
    });
    let times = entries.entry(key.to_string()).or_default();
    if times.len() >= limit {
        return false;
    }
    times.push(now);
    true
}

/// Demo mode must be pointed at deliberately prepared storage. It refuses a
/// real installation even if the environment flag was set by mistake.
pub async fn validate_installation(pool: &SqlitePool, config_dir: &Path) -> Result<(), AppError> {
    let marker = tokio::fs::read_to_string(config_dir.join(".bokhylle-demo"))
        .await
        .map_err(|_| AppError::Unprocessable("demo marker is missing".to_string()))?;
    if marker != MARKER {
        return Err(AppError::Unprocessable("invalid demo marker".to_string()));
    }
    if std::env::var_os("BOKHYLLE_ADMIN_PASSWORD").is_some() {
        return Err(AppError::Unprocessable(
            "demo cannot use an administrator bootstrap password".to_string(),
        ));
    }
    let admins: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE role = 'admin'")
        .fetch_one(pool)
        .await?;
    if admins != 0 {
        return Err(AppError::Unprocessable(
            "demo storage contains administrator accounts".to_string(),
        ));
    }
    let unknown_users: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE username NOT GLOB 'demo_adult_*' AND username NOT GLOB 'demo_child_*'",
    )
    .fetch_one(pool)
    .await?;
    if unknown_users != 0 {
        return Err(AppError::Unprocessable(
            "demo storage contains regular accounts".to_string(),
        ));
    }
    let secret_keys = settings::SECRET_KEYS
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let query = format!("SELECT count(*) FROM settings WHERE key IN ({secret_keys})");
    let mut check = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(query));
    for key in settings::SECRET_KEYS {
        check = check.bind(key);
    }
    if check.fetch_one(pool).await? != 0 {
        return Err(AppError::Unprocessable(
            "demo storage contains connector credentials".to_string(),
        ));
    }
    Ok(())
}

pub async fn status(State(state): State<AppState>) -> Json<DemoStatus> {
    Json(DemoStatus {
        enabled: state.demo.is_some(),
    })
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DemoProfile {
    Adult,
    Child,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct EnterRequest {
    profile: DemoProfile,
}

pub async fn enter(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<
        axum::extract::Extension<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    >,
    Json(body): Json<EnterRequest>,
) -> Result<Response, AppError> {
    let demo = state
        .demo
        .as_ref()
        .ok_or_else(|| AppError::NotFound("demo is unavailable".to_string()))?;
    let trusted_proxy = state
        .settings
        .get_bool(settings::TRUSTED_PROXY, false)
        .await?;
    let peer = connect_info.map(|axum::extract::Extension(info)| info.0.ip());
    let key = client_key(&headers, peer, trusted_proxy);
    if !demo.admit(&key) {
        return Err(AppError::RateLimited);
    }
    let _entry = demo.entry_lock.lock().await;
    let visitors: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE username GLOB 'demo_adult_*'")
            .fetch_one(&state.db)
            .await?;
    if visitors >= MAX_VISITORS {
        return Err(AppError::Unavailable(
            "the demo is full; please try after its next reset".to_string(),
        ));
    }

    let suffix = Uuid::new_v4().simple().to_string();
    let credential = Uuid::new_v4().to_string();
    let adult = state
        .auth
        .create_user_with_profile(
            &format!("demo_adult_{suffix}"),
            &credential,
            Role::User,
            "password",
            "adult",
        )
        .await?;
    let child = match state
        .auth
        .create_user_with_profile(
            &format!("demo_child_{suffix}"),
            &credential,
            Role::User,
            "password",
            "child",
        )
        .await
    {
        Ok(child) => child,
        Err(error) => {
            let _ = sqlx::query("DELETE FROM users WHERE id = ?")
                .bind(adult.id)
                .execute(&state.db)
                .await;
            return Err(error);
        }
    };
    for (user, display_name) in [(&adult, "Adult reader"), (&child, "Child reader")] {
        sqlx::query("UPDATE users SET display_name = ?, onboarded_at = unixepoch() WHERE id = ?")
            .bind(display_name)
            .bind(user.id)
            .execute(&state.db)
            .await?;
    }
    sqlx::query(
        "INSERT OR IGNORE INTO user_books (user_id, book_id, source, on_shelf)
         SELECT ?, b.id, 'demo', 1 FROM books b
         WHERE EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                       WHERE e.book_id = b.id)
         AND b.title IN ('A Christmas Carol', 'Dracula', 'Jane Eyre',
                         'The Picture of Dorian Gray', 'Wuthering Heights')",
    )
    .bind(adult.id)
    .execute(&state.db)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO user_books (user_id, book_id, source, on_shelf)
         SELECT ?, b.id, 'demo', 1 FROM books b
         WHERE b.title IN ('A Christmas Carol', 'Treasure Island', 'The Wonderful Wizard of Oz',
                           'Alice’s Adventures in Wonderland', 'The Secret Garden',
                           'Five Children and It')
           AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                       WHERE e.book_id = b.id)",
    )
    .bind(child.id)
    .execute(&state.db)
    .await?;

    // Each visitor pair gets one real, local-only child request. This makes
    // the notification and its decision useful without contacting a provider.
    let sample_book: Option<i64> = sqlx::query_scalar(
        "SELECT b.id FROM books b
         WHERE b.title = 'Black Beauty'
           AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                       WHERE e.book_id = b.id AND f.format = 'epub')
         LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    if let Some(book_id) = sample_book {
        book_requests::create(&state.db, book_id, child.id).await?;
        notifications::create_for_book(
            &state.db,
            adult.id,
            Some(book_id),
            "request",
            "Child reader requested a book",
            Some("Open notifications to approve or decline the sample request."),
            None,
        )
        .await?;
    }

    let user = match body.profile {
        DemoProfile::Adult => adult,
        DemoProfile::Child => child,
    };
    session_response(&state, user.id).await
}

pub async fn switch(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    if state.demo.is_none() {
        return Err(AppError::NotFound("demo is unavailable".to_string()));
    }
    let (adult, child) = pair_names(&user.username).ok_or(AppError::Forbidden)?;
    let peer_name = if user.username == adult { child } else { adult };
    let peer_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(peer_name)
        .fetch_one(&state.db)
        .await?;
    session_response(&state, peer_id).await
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetRequest {
    book_id: i64,
    #[serde(default)]
    send_when_ready: bool,
}

fn ensure_demo_adult(state: &AppState, user: &crate::auth::User) -> Result<(), AppError> {
    if state.demo.is_none() || pair_names(&user.username).is_none() {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// Start a real, durable demo activity against a sample EPUB already on disk.
/// No indexer, download client, or mail service is contacted.
pub async fn start_get(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<GetRequest>,
) -> Result<Json<DemoGetStarted>, AppError> {
    ensure_demo_adult(&state, &user)?;
    if crate::auth::profile_type(&state.db, user.id).await? != "adult" {
        return Err(AppError::Forbidden);
    }
    if user_books::contains(&state.db, user.id, body.book_id).await? {
        return Err(AppError::Conflict(
            "this book is already on your shelf".to_string(),
        ));
    }
    let has_file: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM book_files f JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ? AND f.format = 'epub'",
    )
    .bind(body.book_id)
    .fetch_one(&state.db)
    .await?;
    if has_file == 0 {
        return Err(AppError::Unprocessable(
            "this title is outside the prepared demo catalogue".to_string(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO demo_gets
         (id, user_id, book_id, started_at, ready_at, send_when_ready)
         VALUES (?, ?, ?, unixepoch(), unixepoch() + 12, ?)
         ON CONFLICT(user_id, book_id) DO UPDATE SET
             started_at = unixepoch(), ready_at = unixepoch() + 12,
             completed_at = NULL, send_when_ready = excluded.send_when_ready
         WHERE demo_gets.completed_at IS NOT NULL",
    )
    .bind(&id)
    .bind(user.id)
    .bind(body.book_id)
    .bind(body.send_when_ready)
    .execute(&state.db)
    .await?;
    let actual_id: String =
        sqlx::query_scalar("SELECT id FROM demo_gets WHERE user_id = ? AND book_id = ?")
            .bind(user.id)
            .bind(body.book_id)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(DemoGetStarted { id: actual_id }))
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    book_id: i64,
}

/// Simulates reader delivery using a persisted timestamp, without sending mail.
pub async fn send(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Json(body): Json<SendRequest>,
) -> Result<Json<DemoSendResult>, AppError> {
    ensure_demo_adult(&state, &user)?;
    if crate::auth::profile_type(&state.db, user.id).await? != "adult" {
        return Err(AppError::Forbidden);
    }
    if !user_books::contains(&state.db, user.id, body.book_id).await? {
        return Err(AppError::Forbidden);
    }
    let file_id: i64 = sqlx::query_scalar(
        "SELECT f.id FROM book_files f JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ? AND f.format = 'epub' ORDER BY f.id LIMIT 1",
    )
    .bind(body.book_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("sample EPUB is unavailable".to_string()))?;
    // One pending send per profile/book, including simultaneous clicks.
    sqlx::query(
        "INSERT INTO demo_sends (user_id, book_id, file_id)
         SELECT ?, ?, ? WHERE NOT EXISTS (
             SELECT 1 FROM demo_sends WHERE user_id = ? AND book_id = ?
             AND created_at > unixepoch() - 8
         )",
    )
    .bind(user.id)
    .bind(body.book_id)
    .bind(file_id)
    .bind(user.id)
    .bind(body.book_id)
    .execute(&state.db)
    .await?;
    let id: i64 = sqlx::query_scalar(
        "SELECT id FROM demo_sends WHERE user_id = ? AND book_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(user.id)
    .bind(body.book_id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(DemoSendResult {
        id,
        status: "SIMULATED",
        message: "On its way to Demo Kindle. Follow the simulated delivery in Activity.",
    }))
}

#[derive(sqlx::FromRow)]
struct DemoGetRow {
    id: String,
    book_id: i64,
    title: String,
    started_at: i64,
    ready_at: i64,
    completed_at: Option<i64>,
    send_when_ready: i64,
}

pub async fn activity(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Result<Json<DemoActivity>, AppError> {
    ensure_demo_adult(&state, &user)?;
    let gets: Vec<DemoGetRow> = sqlx::query_as(
        "SELECT g.id, g.book_id, b.title, g.started_at, g.ready_at,
                g.completed_at, g.send_when_ready
         FROM demo_gets g JOIN books b ON b.id = g.book_id
         WHERE g.user_id = ? ORDER BY g.started_at DESC LIMIT 30",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&state.db)
        .await?;
    let gets: Vec<DemoGetActivity> = gets
        .into_iter()
        .map(
            |DemoGetRow {
                 id,
                 book_id,
                 title,
                 started_at,
                 ready_at,
                 completed_at,
                 send_when_ready,
             }| {
                let status = if completed_at.is_some() {
                    "READY"
                } else if now - started_at < 3 {
                    "LOOKING"
                } else if now - started_at < 5 {
                    "FOUND"
                } else {
                    "GETTING"
                };
                DemoGetActivity {
                    id,
                    book_id,
                    title,
                    status,
                    started_at,
                    ready_at,
                    completed_at,
                    send_when_ready: send_when_ready != 0,
                }
            },
        )
        .collect();
    let sends: Vec<(i64, i64, String, i64)> = sqlx::query_as(
        "SELECT s.id, s.book_id, b.title, s.created_at
         FROM demo_sends s JOIN books b ON b.id = s.book_id
         WHERE s.user_id = ? ORDER BY s.created_at DESC, s.id DESC LIMIT 30",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    let sends: Vec<DemoSendActivity> = sends
        .into_iter()
        .map(|(id, book_id, title, created_at)| DemoSendActivity {
            id,
            book_id,
            title,
            created_at,
            status: if now - created_at < 3 {
                "PREPARING"
            } else if now - created_at < 8 {
                "SENDING"
            } else {
                "DELIVERED"
            },
        })
        .collect();
    Ok(Json(DemoActivity { gets, sends }))
}

async fn paired_child_request(
    state: &AppState,
    user: &crate::auth::User,
    id: i64,
) -> Result<book_requests::BookRequest, AppError> {
    ensure_demo_adult(state, user)?;
    if crate::auth::profile_type(&state.db, user.id).await? != "adult" {
        return Err(AppError::Forbidden);
    }
    let (_, child_name) = pair_names(&user.username).ok_or(AppError::Forbidden)?;
    let child_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(child_name)
        .fetch_one(&state.db)
        .await?;
    let request = book_requests::raw(&state.db, id)
        .await?
        .ok_or(AppError::Forbidden)?;
    if request.user_id != child_id {
        return Err(AppError::Forbidden);
    }
    let sample_file: Option<i64> = sqlx::query_scalar(
        "SELECT f.id FROM book_files f JOIN editions e ON e.id = f.edition_id
         WHERE e.book_id = ? AND f.format = 'epub' LIMIT 1",
    )
    .bind(request.book_id)
    .fetch_optional(&state.db)
    .await?;
    if sample_file.is_none() {
        return Err(AppError::Forbidden);
    }
    Ok(request)
}

pub async fn approve_request(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    RoutePath(id): RoutePath<i64>,
) -> Result<Json<DemoRequestResult>, AppError> {
    let request = paired_child_request(&state, &user, id).await?;
    let mut tx = state.db.begin().await?;
    if !book_requests::mark_approved_tx(&mut tx, id, user.id).await? {
        return Err(AppError::Unprocessable(
            "this request was already decided".to_string(),
        ));
    }
    user_books::add_tx(&mut tx, request.user_id, request.book_id, "demo_request").await?;
    tx.commit().await?;
    notifications::create_for_book(
        &state.db,
        request.user_id,
        Some(request.book_id),
        "ready",
        "Your book request is ready",
        Some("An adult added this sample book to your shelf."),
        None,
    )
    .await?;
    Ok(Json(DemoRequestResult {
        request: book_requests::get(&state.db, id).await?,
    }))
}

pub async fn decline_request(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    RoutePath(id): RoutePath<i64>,
) -> Result<Json<DemoRequestResult>, AppError> {
    let request = paired_child_request(&state, &user, id).await?;
    if !book_requests::mark_declined(&state.db, id, user.id).await? {
        return Err(AppError::Unprocessable(
            "this request was already decided".to_string(),
        ));
    }
    notifications::create_for_book(
        &state.db,
        request.user_id,
        Some(request.book_id),
        "declined",
        "Your book request was declined",
        None,
        None,
    )
    .await?;
    Ok(Json(DemoRequestResult {
        request: book_requests::get(&state.db, id).await?,
    }))
}

/// Finish due demo work after a restart as well as during normal operation.
pub async fn tick(state: &AppState) -> Result<(), AppError> {
    if state.demo.is_none() {
        return Ok(());
    }
    let due: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT id, user_id, book_id, send_when_ready FROM demo_gets
         WHERE completed_at IS NULL AND ready_at <= unixepoch()
         ORDER BY ready_at LIMIT 64",
    )
    .fetch_all(&state.db)
    .await?;
    for (id, user_id, book_id, send_when_ready) in due {
        let mut tx = state.db.begin().await?;
        let claimed = sqlx::query(
            "UPDATE demo_gets SET completed_at = unixepoch()
             WHERE id = ? AND completed_at IS NULL",
        )
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        if claimed.rows_affected() == 0 {
            tx.rollback().await?;
            continue;
        }
        user_books::add_tx(&mut tx, user_id, book_id, "demo_get").await?;
        if send_when_ready != 0 {
            let file_id: Option<i64> = sqlx::query_scalar(
                "SELECT f.id FROM book_files f JOIN editions e ON e.id = f.edition_id
                 WHERE e.book_id = ? AND f.format = 'epub' ORDER BY f.id LIMIT 1",
            )
            .bind(book_id)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(file_id) = file_id {
                sqlx::query("INSERT INTO demo_sends (user_id, book_id, file_id) VALUES (?, ?, ?)")
                    .bind(user_id)
                    .bind(book_id)
                    .bind(file_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
    }
    Ok(())
}

async fn session_response(state: &AppState, user_id: i64) -> Result<Response, AppError> {
    let user = state
        .auth
        .user_by_id(user_id)
        .await?
        .ok_or(AppError::Unauthorized)?;
    let token = state.auth.create_session(user_id, false).await?;
    let secure = state
        .settings
        .get_bool(settings::SECURE_COOKIES, false)
        .await?;
    Ok((
        [(header::SET_COOKIE, set_cookie_header(&token, secure, false))],
        Json(json!({ "user": crate::routes::auth::user_with_notifications(state, &user).await? })),
    )
        .into_response())
}

pub async fn guard(state: &AppState, request: Request, next: Next) -> Result<Response, AppError> {
    if state.demo.is_none() {
        return Ok(next.run(request).await);
    }
    let path = request.uri().path();
    if path.starts_with("/api/admin")
        || path.starts_with("/mcp")
        || path.starts_with("/opds")
        || path.starts_with("/users/")
        || path.starts_with("/syncs/")
        || path.starts_with("/api/profile/tokens")
        || path.starts_with("/api/profile/agent-tokens")
        || path.starts_with("/api/auth/users")
        || path.starts_with("/api/requests")
        || path.starts_with("/api/acquisitions")
        || path.starts_with("/api/delivery-targets")
        || path.starts_with("/api/deliveries")
        || path == "/api/activity/direct"
    {
        return Err(AppError::Forbidden);
    }
    if matches!(*request.method(), Method::GET | Method::HEAD) {
        return Ok(next.run(request).await);
    }
    if allowed_mutation(request.method(), path) {
        if path != "/api/demo/enter" {
            let peer = request
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .map(|info| info.0.ip());
            let trusted_proxy = state
                .settings
                .get_bool(settings::TRUSTED_PROXY, false)
                .await?;
            let key = client_key(request.headers(), peer, trusted_proxy);
            let demo = state.demo.as_ref().expect("demo enabled");
            let admitted = if is_browser_position_path(path) {
                demo.admit_position_save(&key)
            } else {
                demo.admit_mutation(&key)
            };
            if !admitted {
                return Err(AppError::RateLimited);
            }
        }
        return Ok(next.run(request).await);
    }
    Err(AppError::Forbidden)
}

fn allowed_mutation(method: &Method, path: &str) -> bool {
    if *method == Method::POST {
        return matches!(
            path,
            "/api/demo/enter"
                | "/api/demo/switch"
                | "/api/demo/get"
                | "/api/demo/send"
                | "/api/auth/logout"
                | "/api/notifications/read"
                | "/api/discover/like"
                | "/api/discover/authors/follow"
        ) || (path.starts_with("/api/authors/") && path.ends_with("/follow"))
            || (path.starts_with("/api/demo/requests/")
                && (path.ends_with("/approve") || path.ends_with("/decline")));
    }
    if *method == Method::PUT {
        return is_browser_position_path(path)
            || is_browser_direction_path(path)
            || (path.starts_with("/api/users/") && path.contains("/shelf/"))
            || path.starts_with("/api/books/")
                && (path.ends_with("/shelf") || path.ends_with("/preference"));
    }
    *method == Method::DELETE
        && ((path.starts_with("/api/books/") && path.ends_with("/shelf"))
            || (path.starts_with("/api/authors/") && path.ends_with("/follow")))
}

fn is_browser_position_path(path: &str) -> bool {
    match path.split('/').collect::<Vec<_>>().as_slice() {
        ["", "api", "books", book_id, "files", file_id, "position"] => {
            book_id.parse::<i64>().is_ok() && file_id.parse::<i64>().is_ok()
        }
        _ => false,
    }
}

fn is_browser_direction_path(path: &str) -> bool {
    match path.split('/').collect::<Vec<_>>().as_slice() {
        ["", "api", "books", book_id, "files", file_id, "direction"] => {
            book_id.parse::<i64>().is_ok() && file_id.parse::<i64>().is_ok()
        }
        _ => false,
    }
}

#[cfg(test)]
mod reader_guard_tests {
    use super::{Method, allowed_mutation};

    #[test]
    fn demo_allows_only_the_reader_position_mutation() {
        assert!(allowed_mutation(
            &Method::PUT,
            "/api/books/12/files/3/position"
        ));
        assert!(allowed_mutation(
            &Method::PUT,
            "/api/books/12/files/3/direction"
        ));
        assert!(!allowed_mutation(
            &Method::PUT,
            "/api/books/12/files/3/content"
        ));
        assert!(!allowed_mutation(
            &Method::PUT,
            "/api/books/12/files/3/position/extra"
        ));
    }
}
