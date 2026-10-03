use lettre::message::header::ContentType;
use lettre::message::{Attachment, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::AppState;
use crate::error::AppError;
use crate::settings;

pub const DEFAULT_MAX_ATTACHMENT_MB: i64 = 25;
pub const DEFAULT_SMTP_PORT: i64 = 587;

#[derive(Debug, Clone, Serialize, FromRow, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryTarget {
    pub id: i64,
    pub user_id: i64,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub kind: String,
    pub name: String,
    pub address: String,
    pub connector: String,
    pub enabled: bool,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, FromRow, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub id: i64,
    pub book_id: i64,
    pub file_id: i64,
    pub target_id: Option<i64>,
    pub user_id: Option<i64>,
    pub address: String,
    pub status: String,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub async fn list_targets(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Vec<DeliveryTarget>, AppError> {
    let targets: Vec<DeliveryTarget> = sqlx::query_as(
        "SELECT id, user_id, type, name, address, connector, enabled, is_default, created_at, updated_at
         FROM delivery_targets
         WHERE user_id = ?
         ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(targets)
}

/// Connectors describe *how* a book reaches a reader. `email` is the only
/// push transport today; future integrations (Dropbox, WebDAV, ...) add a
/// variant here and a branch below rather than a new concept.
pub fn connector_kind(connector: &str) -> Option<&'static str> {
    match connector {
        "email" => Some("kindle"),
        _ => None,
    }
}

/// Device presets are labels for readers that share a transport; they are
/// metadata, not behavior. Kobo and BOOX use OPDS (pull) instead.
pub const DEVICE_PRESETS: [(&str, &str, &str); 3] = [
    ("kindle", "Kindle", "email"),
    ("pocketbook", "PocketBook", "email"),
    ("other", "Other", "email"),
];

pub async fn create_target(
    pool: &SqlitePool,
    user_id: i64,
    name: &str,
    address: &str,
    connector: &str,
    device_type: Option<&str>,
) -> Result<DeliveryTarget, AppError> {
    let Some(default_kind) = connector_kind(connector) else {
        return Err(AppError::Unprocessable(format!(
            "unsupported reader connector '{connector}'"
        )));
    };
    let kind = match device_type {
        None => default_kind,
        Some(value) => DEVICE_PRESETS
            .iter()
            .find(|(device, _, _)| *device == value)
            .map(|(_, _, _)| value)
            .ok_or_else(|| AppError::Unprocessable(format!("unsupported reader type '{value}'")))?,
    };
    let address = validate_address(address)?;
    let name = name.trim();
    let name = if name.is_empty() {
        DEVICE_PRESETS
            .iter()
            .find(|(device, _, _)| *device == kind)
            .map(|(_, label, _)| *label)
            .unwrap_or("Reader")
    } else {
        name
    };

    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM delivery_targets WHERE user_id = ? LIMIT 1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    let is_default = existing.is_none();

    let id = sqlx::query(
        "INSERT INTO delivery_targets (user_id, type, name, address, connector, is_default)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(kind)
    .bind(name)
    .bind(&address)
    .bind(connector)
    .bind(is_default)
    .execute(pool)
    .await?
    .last_insert_rowid();

    get_target(pool, user_id, id)
        .await?
        .ok_or_else(|| AppError::Unprocessable("delivery target disappeared after insert".into()))
}

pub async fn get_target(
    pool: &SqlitePool,
    user_id: i64,
    id: i64,
) -> Result<Option<DeliveryTarget>, AppError> {
    let target: Option<DeliveryTarget> = sqlx::query_as(
        "SELECT id, user_id, type, name, address, connector, enabled, is_default, created_at, updated_at
         FROM delivery_targets
         WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(target)
}

pub async fn update_target(
    pool: &SqlitePool,
    user_id: i64,
    id: i64,
    name: Option<String>,
    address: Option<String>,
    enabled: Option<bool>,
) -> Result<Option<DeliveryTarget>, AppError> {
    let Some(existing) = get_target(pool, user_id, id).await? else {
        return Ok(None);
    };

    let name = name.unwrap_or(existing.name);
    let address = match address {
        Some(address) => validate_address(&address)?,
        None => existing.address,
    };
    let enabled = enabled.unwrap_or(existing.enabled);
    let is_default = existing.is_default && enabled;

    sqlx::query(
        "UPDATE delivery_targets
         SET name = ?, address = ?, enabled = ?, is_default = ?, updated_at = unixepoch()
         WHERE id = ? AND user_id = ?",
    )
    .bind(&name)
    .bind(&address)
    .bind(enabled)
    .bind(is_default)
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    get_target(pool, user_id, id).await
}

pub async fn set_default(pool: &SqlitePool, user_id: i64, id: i64) -> Result<bool, AppError> {
    let Some(target) = get_target(pool, user_id, id).await? else {
        return Ok(false);
    };
    if !target.enabled {
        return Err(AppError::Unprocessable(
            "the delivery target is disabled".to_string(),
        ));
    }

    let mut transaction = pool.begin().await?;
    sqlx::query(
        "UPDATE delivery_targets SET is_default = 0, updated_at = unixepoch()
         WHERE user_id = ? AND is_default = 1",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE delivery_targets SET is_default = 1, updated_at = unixepoch()
         WHERE id = ? AND user_id = ?",
    )
    .bind(id)
    .bind(user_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(true)
}

pub async fn delete_target(pool: &SqlitePool, user_id: i64, id: i64) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM delivery_targets WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn default_reader(
    state: &AppState,
    user_id: i64,
) -> Result<(Option<String>, &'static str), AppError> {
    let target: Option<DeliveryTarget> = sqlx::query_as(
        "SELECT id, user_id, type, name, address, connector, enabled, is_default, created_at, updated_at
         FROM delivery_targets
         WHERE user_id = ? AND enabled = 1
         ORDER BY is_default DESC, id
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    if let Some(target) = target {
        return Ok((Some(target.address), "personal"));
    }

    let fallback = state
        .settings
        .get_string(settings::KINDLE_ADDRESS, "")
        .await?
        .trim()
        .to_string();
    if !fallback.is_empty() {
        return Ok((Some(fallback), "household"));
    }

    Ok((None, "none"))
}

pub async fn resolve_target(
    state: &AppState,
    user_id: i64,
    target_id: Option<i64>,
) -> Result<(Option<i64>, String), AppError> {
    if let Some(target_id) = target_id {
        let Some(target) = get_target(&state.db, user_id, target_id).await? else {
            return Err(AppError::NotFound("delivery target not found".to_string()));
        };
        if !target.enabled {
            return Err(AppError::Unprocessable(
                "the delivery target is disabled".to_string(),
            ));
        }
        return Ok((Some(target.id), target.address));
    }

    let target: Option<DeliveryTarget> = sqlx::query_as(
        "SELECT id, user_id, type, name, address, connector, enabled, is_default, created_at, updated_at
         FROM delivery_targets
         WHERE user_id = ? AND enabled = 1
         ORDER BY is_default DESC, id
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    if let Some(target) = target {
        return Ok((Some(target.id), target.address));
    }

    let fallback = state
        .settings
        .get_string(settings::KINDLE_ADDRESS, "")
        .await?;
    let fallback = fallback.trim().to_string();
    if fallback.is_empty() {
        return Err(AppError::Unprocessable(
            "no delivery target is configured".to_string(),
        ));
    }

    Ok((None, validate_address(&fallback)?))
}

/// Check the configuration before an adult approves a child's request. The
/// acquisition may take time, but its destination must already be known.
pub async fn ensure_default_delivery(state: &AppState, user_id: i64) -> Result<(), AppError> {
    resolve_target(state, user_id, None)
        .await
        .map_err(|error| match error {
        AppError::Unprocessable(_) => AppError::Unprocessable(
            "Add a reader for this child in Settings → Household, or set the household fallback in Settings → Delivery before approving this request"
                .to_string(),
        ),
            other => other,
        })?;
    if !smtp_configured(&state.settings).await {
        return Err(AppError::Unprocessable(
            "Configure SMTP delivery before approving this child's request".to_string(),
        ));
    }
    sender(&state.settings).await?;
    Ok(())
}

/// A PENDING row at startup can only mean the process died mid-send: sends
/// are synchronous. Do not resend (the mail may have gone out); mark it
/// failed so the user can retry deliberately.
pub async fn recover(state: &AppState) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE deliveries
         SET status = 'FAILED',
             error_message = 'Delivery was interrupted; retry if needed',
             updated_at = unixepoch()
         WHERE status = 'PENDING'",
    )
    .execute(&state.db)
    .await?;
    if result.rows_affected() > 0 {
        tracing::warn!(
            count = result.rows_affected(),
            "delivery.recovered_interrupted"
        );
    }
    Ok(result.rows_affected())
}

pub async fn deliver(
    state: &AppState,
    user_id: i64,
    book_id: i64,
    file_id: i64,
    target_id: Option<i64>,
) -> Result<Delivery, AppError> {
    crate::services::sharing::require_access(&state.db, user_id, book_id).await?;
    let (resolved_target_id, address) = resolve_target(state, user_id, target_id).await?;
    deliver_to_resolved(
        state,
        user_id,
        book_id,
        file_id,
        resolved_target_id,
        &address,
    )
    .await
}

/// Internal delivery of a previously validated, frozen destination. Scheduled
/// sends use this to keep the reader the requester selected at scheduling time.
pub(crate) async fn deliver_to_resolved(
    state: &AppState,
    user_id: i64,
    book_id: i64,
    file_id: i64,
    resolved_target_id: Option<i64>,
    address: &str,
) -> Result<Delivery, AppError> {
    crate::services::sharing::require_access(&state.db, user_id, book_id).await?;
    let address = validate_address(address)?;

    let file: Option<(String, String, i64, String, String)> = sqlx::query_as(
        "SELECT f.path, f.format, f.size, b.title, COALESCE((
             SELECT group_concat(a.name, ', ')
             FROM book_authors ba JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = b.id
         ), '')
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         JOIN books b ON b.id = e.book_id
         WHERE f.id = ? AND b.id = ?",
    )
    .bind(file_id)
    .bind(book_id)
    .fetch_optional(&state.db)
    .await?;

    let Some((path, format, size, title, authors)) = file else {
        return Err(AppError::NotFound("file not found".to_string()));
    };

    let max_mb = state
        .settings
        .get_int(settings::MAX_ATTACHMENT_MB, DEFAULT_MAX_ATTACHMENT_MB)
        .await?;
    if max_mb > 0 && size > max_mb * 1024 * 1024 {
        return Err(AppError::Unprocessable(format!(
            "the file exceeds the {max_mb} MB delivery limit"
        )));
    }

    let delivery_id = sqlx::query(
        "INSERT INTO deliveries (book_id, file_id, target_id, user_id, address, status)
         VALUES (?, ?, ?, ?, ?, 'PENDING')",
    )
    .bind(book_id)
    .bind(file_id)
    .bind(resolved_target_id)
    .bind(user_id)
    .bind(&address)
    .execute(&state.db)
    .await?
    .last_insert_rowid();

    send(
        state,
        delivery_id,
        &path,
        &format,
        &title,
        &authors,
        &address,
    )
    .await?;

    get_delivery(&state.db, delivery_id)
        .await?
        .ok_or_else(|| AppError::Unprocessable("delivery disappeared after insert".into()))
}

async fn send(
    state: &AppState,
    delivery_id: i64,
    path: &str,
    format: &str,
    title: &str,
    authors: &str,
    address: &str,
) -> Result<(), AppError> {
    tracing::info!(delivery_id, address, "delivery.started");

    match send_file(state, path, format, title, authors, address).await {
        Ok(()) => {
            sqlx::query(
                "UPDATE deliveries SET status = 'SENT', error_message = NULL, updated_at = unixepoch()
                 WHERE id = ?",
            )
            .bind(delivery_id)
            .execute(&state.db)
            .await?;
            // Sending a book is a strong signal that it belongs on the shelf.
            if let Ok(Some((book_id, user_id))) = sqlx::query_as::<_, (i64, Option<i64>)>(
                "SELECT book_id, user_id FROM deliveries WHERE id = ?",
            )
            .bind(delivery_id)
            .fetch_optional(&state.db)
            .await
                && let Some(user_id) = user_id
            {
                crate::user_books::add(&state.db, user_id, book_id, "sent")
                    .await
                    .ok();
            }
            tracing::info!(delivery_id, "delivery.completed");
        }
        Err(AppError::Unprocessable(message)) => {
            sqlx::query(
                "UPDATE deliveries SET status = 'FAILED', error_message = ?, updated_at = unixepoch()
                 WHERE id = ?",
            )
            .bind(&message)
            .bind(delivery_id)
            .execute(&state.db)
            .await?;
            tracing::warn!(delivery_id, error = %message, "delivery.failed");
        }
        Err(error) => {
            sqlx::query(
                "UPDATE deliveries SET status = 'FAILED', error_message = ?, updated_at = unixepoch()
                 WHERE id = ?",
            )
            .bind(error.to_string())
            .bind(delivery_id)
            .execute(&state.db)
            .await?;
            tracing::warn!(delivery_id, %error, "delivery.failed");
        }
    }

    Ok(())
}

async fn send_file(
    state: &AppState,
    path: &str,
    format: &str,
    title: &str,
    authors: &str,
    address: &str,
) -> Result<(), AppError> {
    let host = state.settings.get_string(settings::SMTP_HOST, "").await?;
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err(AppError::Unprocessable(
            "SMTP is not configured".to_string(),
        ));
    }

    let from = sender(&state.settings).await?;

    let attachment_path = path.to_string();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&attachment_path))
        .await
        .map_err(|error| AppError::Unavailable(error.to_string()))??;

    let content_type = match format {
        "pdf" => ContentType::parse("application/pdf").expect("valid content type"),
        "cbz" => ContentType::parse("application/vnd.comicbook+zip").expect("valid content type"),
        _ => ContentType::parse("application/epub+zip").expect("valid content type"),
    };

    let attachment_name = format!(
        "{} - {}.{}",
        sanitize_filename(title),
        sanitize_filename(authors),
        format
    );

    let email = Message::builder()
        .from(
            from.parse()
                .map_err(|_| AppError::Unprocessable("invalid sender address".to_string()))?,
        )
        .to(address
            .parse()
            .map_err(|_| AppError::Unprocessable("invalid recipient address".to_string()))?)
        .subject(format!("{title} - {authors}"))
        .multipart(
            MultiPart::mixed()
                .singlepart(SinglePart::plain(format!(
                    "Sending \"{title}\" from your library."
                )))
                .singlepart(Attachment::new(attachment_name).body(bytes, content_type)),
        )
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;

    let transport = build_transport(&state.settings).await?;
    transport
        .send(email)
        .await
        .map_err(|error| AppError::Unprocessable(format!("SMTP send failed: {error}")))?;

    Ok(())
}

/// Notification-style plain mail that shares the delivery SMTP settings.
pub async fn send_plain(
    settings: &settings::Settings,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), AppError> {
    let from = sender(settings).await?;
    let email = Message::builder()
        .from(
            from.parse()
                .map_err(|_| AppError::Unprocessable("invalid sender address".to_string()))?,
        )
        .to(to
            .parse()
            .map_err(|_| AppError::Unprocessable("invalid recipient address".to_string()))?)
        .subject(subject.to_string())
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;

    let transport = build_transport(settings).await?;
    transport
        .send(email)
        .await
        .map_err(|error| AppError::Unprocessable(format!("SMTP send failed: {error}")))?;
    Ok(())
}

async fn sender(settings: &settings::Settings) -> Result<String, AppError> {
    let from = settings.get_string(settings::SMTP_FROM, "").await?;
    let from = from.trim().to_string();
    let from = if from.is_empty() {
        settings
            .get_string(settings::SMTP_USERNAME, "")
            .await?
            .trim()
            .to_string()
    } else {
        from
    };
    if from.is_empty() {
        return Err(AppError::Unprocessable(
            "no sender address is configured".to_string(),
        ));
    }
    Ok(from)
}

pub async fn smtp_configured(settings: &settings::Settings) -> bool {
    settings
        .get_string(settings::SMTP_HOST, "")
        .await
        .map(|host| !host.trim().is_empty())
        .unwrap_or(false)
}

async fn build_transport(
    settings: &settings::Settings,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, AppError> {
    let host = settings
        .get_string(settings::SMTP_HOST, "")
        .await?
        .trim()
        .to_string();
    if host.is_empty() {
        return Err(AppError::Unprocessable(
            "SMTP is not configured".to_string(),
        ));
    }

    let port = settings
        .get_int(settings::SMTP_PORT, DEFAULT_SMTP_PORT)
        .await? as u16;
    let username = settings.get_string(settings::SMTP_USERNAME, "").await?;
    let password = settings.get_string(settings::SMTP_PASSWORD, "").await?;
    let tls = settings.get_string(settings::SMTP_TLS, "starttls").await?;

    let mut builder = match tls.trim().to_ascii_lowercase().as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
            .map_err(|error| AppError::Unprocessable(format!("invalid SMTP host: {error}")))?,
        "none" => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&host),
        _ => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
            .map_err(|error| AppError::Unprocessable(format!("invalid SMTP host: {error}")))?,
    };

    builder = builder.port(port);

    if !username.trim().is_empty() {
        builder = builder.credentials(Credentials::new(username.trim().to_string(), password));
    }

    Ok(builder.build())
}

pub async fn test_connection(state: &AppState) -> Result<(), AppError> {
    if state
        .settings
        .get_string(settings::SMTP_HOST, "")
        .await?
        .trim()
        .is_empty()
    {
        return Err(AppError::Unprocessable(
            "SMTP is not configured".to_string(),
        ));
    }

    let transport = build_transport(&state.settings).await?;
    transport
        .test_connection()
        .await
        .map_err(|error| AppError::Unprocessable(format!("SMTP connection failed: {error}")))?;
    Ok(())
}

pub async fn get_delivery(pool: &SqlitePool, id: i64) -> Result<Option<Delivery>, AppError> {
    let delivery: Option<Delivery> = sqlx::query_as(
        "SELECT id, book_id, file_id, target_id, user_id, address, status, error_message,
                created_at, updated_at
         FROM deliveries
         WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(delivery)
}

pub async fn list_deliveries(
    pool: &SqlitePool,
    viewer_id: i64,
    is_admin: bool,
    all: bool,
    book_id: Option<i64>,
) -> Result<Vec<Delivery>, AppError> {
    let mut query = String::from(
        "SELECT id, book_id, file_id, target_id, user_id, address, status, error_message,
                created_at, updated_at
         FROM deliveries",
    );
    let mut conditions = Vec::new();

    if !is_admin || !all {
        conditions.push("user_id = ?");
    }
    if book_id.is_some() {
        conditions.push("book_id = ?");
    }
    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }
    query.push_str(" ORDER BY created_at DESC, id DESC LIMIT 200");

    let mut request = sqlx::query_as::<_, Delivery>(sqlx::AssertSqlSafe(query));
    if !is_admin || !all {
        request = request.bind(viewer_id);
    }
    if let Some(book_id) = book_id {
        request = request.bind(book_id);
    }

    Ok(request.fetch_all(pool).await?)
}

pub async fn retry(
    state: &AppState,
    viewer_id: i64,
    is_admin: bool,
    delivery_id: i64,
) -> Result<Delivery, AppError> {
    let Some(delivery) = get_delivery(&state.db, delivery_id).await? else {
        return Err(AppError::NotFound("delivery not found".to_string()));
    };

    if !is_admin && delivery.user_id != Some(viewer_id) {
        return Err(AppError::Forbidden);
    }

    let file: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT f.path, f.format, b.title, COALESCE((
             SELECT group_concat(a.name, ', ')
             FROM book_authors ba JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = b.id
         ), '')
         FROM book_files f
         JOIN editions e ON e.id = f.edition_id
         JOIN books b ON b.id = e.book_id
         WHERE f.id = ?",
    )
    .bind(delivery.file_id)
    .fetch_optional(&state.db)
    .await?;

    let Some((path, format, title, authors)) = file else {
        return Err(AppError::NotFound("file not found".to_string()));
    };

    sqlx::query("UPDATE deliveries SET status = 'PENDING', updated_at = unixepoch() WHERE id = ?")
        .bind(delivery_id)
        .execute(&state.db)
        .await?;

    send(
        state,
        delivery_id,
        &path,
        &format,
        &title,
        &authors,
        &delivery.address,
    )
    .await?;

    get_delivery(&state.db, delivery_id)
        .await?
        .ok_or_else(|| AppError::Unprocessable("delivery disappeared".into()))
}

fn validate_address(address: &str) -> Result<String, AppError> {
    let address = address.trim().to_string();
    if address.is_empty() || !address.contains('@') {
        return Err(AppError::Unprocessable(
            "a valid email address is required".to_string(),
        ));
    }
    Ok(address)
}

fn sanitize_filename(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();

    let trimmed = sanitized.trim().to_string();
    if trimmed.is_empty() {
        "Book".to_string()
    } else {
        trimmed
    }
}
