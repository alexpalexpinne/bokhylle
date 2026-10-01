use serde_json::Value;
use sqlx::SqlitePool;

use crate::error::AppError;

pub const LIBRARY_ROOT: &str = "library.root";
pub const DOWNLOADS_DIR: &str = "downloads.dir";
pub const SCAN_ON_STARTUP: &str = "library.scan_on_startup";
pub const PREFERRED_FORMAT: &str = "library.preferred_format";
pub const PREFERRED_LANGUAGE: &str = "library.preferred_language";
pub const METADATA_PROVIDER: &str = "metadata.provider";
pub const RATINGS_PROVIDER: &str = "ratings.provider";
pub const GOOGLE_BOOKS_API_KEY: &str = "integrations.google_books.api_key";
pub const SECURE_COOKIES: &str = "security.secure_cookies";
pub const TRUSTED_PROXY: &str = "security.trusted_proxy";
pub const PROWLARR_URL: &str = "integrations.prowlarr.url";
pub const PROWLARR_API_KEY: &str = "integrations.prowlarr.api_key";
pub const INDEXER_PROVIDER: &str = "integrations.indexer.provider";
pub const TORZNAB_URL: &str = "integrations.torznab.url";
pub const TORZNAB_API_KEY: &str = "integrations.torznab.api_key";
pub const TORZNAB_CATEGORIES: &str = "integrations.torznab.categories";
pub const NEWZNAB_URL: &str = "integrations.newznab.url";
pub const NEWZNAB_API_KEY: &str = "integrations.newznab.api_key";
pub const NEWZNAB_CATEGORIES: &str = "integrations.newznab.categories";
pub const SABNZBD_URL: &str = "integrations.sabnzbd.url";
pub const SABNZBD_API_KEY: &str = "integrations.sabnzbd.api_key";
pub const SABNZBD_CATEGORY: &str = "integrations.sabnzbd.category";
pub const QBITTORRENT_URL: &str = "integrations.qbittorrent.url";
pub const QBITTORRENT_API_KEY: &str = "integrations.qbittorrent.api_key";
pub const QBITTORRENT_USERNAME: &str = "integrations.qbittorrent.username";
pub const QBITTORRENT_PASSWORD: &str = "integrations.qbittorrent.password";
pub const QBITTORRENT_CATEGORY: &str = "integrations.qbittorrent.category";
pub const CLEANUP_DOWNLOADS: &str = "imports.cleanup_downloads";
pub const WATCH_ENABLED: &str = "imports.watch_enabled";
pub const WATCH_FOLDER: &str = "imports.watch_folder";
pub const SMTP_HOST: &str = "smtp.host";
pub const SMTP_PORT: &str = "smtp.port";
pub const SMTP_USERNAME: &str = "smtp.username";
pub const SMTP_PASSWORD: &str = "smtp.password";
pub const SMTP_FROM: &str = "smtp.from";
pub const SMTP_TLS: &str = "smtp.tls";
pub const KINDLE_ADDRESS: &str = "delivery.kindle_address";
pub const MAX_ATTACHMENT_MB: &str = "delivery.max_attachment_mb";
pub const IMPORT_STRATEGY: &str = "imports.strategy";
pub const SCAN_INTERVAL_HOURS: &str = "library.scan_interval_hours";
pub const AMAZON_DOMAIN: &str = "delivery.amazon_domain";
pub const BACKUP_INTERVAL_HOURS: &str = "backups.interval_hours";
pub const BACKUP_KEEP: &str = "backups.keep";
pub const UPDATE_CHECKS: &str = "updates.check_enabled";
pub const RETRIES_ENABLED: &str = "retries.enabled";
pub const RETRIES_MAX_DAYS: &str = "retries.max_days";

pub const SECRET_KEYS: [&str; 8] = [
    PROWLARR_API_KEY,
    TORZNAB_API_KEY,
    NEWZNAB_API_KEY,
    SABNZBD_API_KEY,
    QBITTORRENT_API_KEY,
    QBITTORRENT_PASSWORD,
    SMTP_PASSWORD,
    GOOGLE_BOOKS_API_KEY,
];

pub const KNOWN_KEYS: [&str; 46] = [
    LIBRARY_ROOT,
    DOWNLOADS_DIR,
    SCAN_ON_STARTUP,
    PREFERRED_FORMAT,
    PREFERRED_LANGUAGE,
    METADATA_PROVIDER,
    RATINGS_PROVIDER,
    GOOGLE_BOOKS_API_KEY,
    SECURE_COOKIES,
    TRUSTED_PROXY,
    PROWLARR_URL,
    PROWLARR_API_KEY,
    INDEXER_PROVIDER,
    TORZNAB_URL,
    TORZNAB_API_KEY,
    TORZNAB_CATEGORIES,
    NEWZNAB_URL,
    NEWZNAB_API_KEY,
    NEWZNAB_CATEGORIES,
    SABNZBD_URL,
    SABNZBD_API_KEY,
    SABNZBD_CATEGORY,
    QBITTORRENT_URL,
    QBITTORRENT_API_KEY,
    QBITTORRENT_USERNAME,
    QBITTORRENT_PASSWORD,
    QBITTORRENT_CATEGORY,
    CLEANUP_DOWNLOADS,
    WATCH_ENABLED,
    WATCH_FOLDER,
    SMTP_HOST,
    SMTP_PORT,
    SMTP_USERNAME,
    SMTP_PASSWORD,
    SMTP_FROM,
    SMTP_TLS,
    KINDLE_ADDRESS,
    MAX_ATTACHMENT_MB,
    IMPORT_STRATEGY,
    SCAN_INTERVAL_HOURS,
    AMAZON_DOMAIN,
    BACKUP_INTERVAL_HOURS,
    BACKUP_KEEP,
    UPDATE_CHECKS,
    RETRIES_ENABLED,
    RETRIES_MAX_DAYS,
];

pub fn is_secret(key: &str) -> bool {
    SECRET_KEYS.contains(&key)
}

/// Write-time validation: invalid values never reach the database, so a typo
/// cannot wedge a later read.
pub fn validate(key: &str, value: &Value) -> Result<(), AppError> {
    let want_string = || match value {
        Value::String(_) => Ok(()),
        _ => Err(AppError::Unprocessable(format!(
            "setting '{key}' must be a string"
        ))),
    };
    let want_bool = || match value {
        Value::Bool(_) => Ok(()),
        _ => Err(AppError::Unprocessable(format!(
            "setting '{key}' must be true or false"
        ))),
    };
    let want_int = |min: i64, max: i64| match value {
        Value::Number(number) => match number.as_i64() {
            Some(number) if number >= min && number <= max => Ok(()),
            _ => Err(AppError::Unprocessable(format!(
                "setting '{key}' must be between {min} and {max}"
            ))),
        },
        _ => Err(AppError::Unprocessable(format!(
            "setting '{key}' must be a number"
        ))),
    };
    let want_number = |min: f64, max: f64| match value {
        Value::Number(number) => match number.as_f64() {
            Some(number) if number >= min && number <= max => Ok(()),
            _ => Err(AppError::Unprocessable(format!(
                "setting '{key}' must be between {min} and {max}"
            ))),
        },
        _ => Err(AppError::Unprocessable(format!(
            "setting '{key}' must be a number"
        ))),
    };
    let want_one_of = |allowed: &[&str]| match value {
        Value::String(text) if allowed.contains(&text.as_str()) => Ok(()),
        _ => Err(AppError::Unprocessable(format!(
            "setting '{key}' must be one of {}",
            allowed.join(", ")
        ))),
    };

    match key {
        METADATA_PROVIDER => want_one_of(&["automatic", "openlibrary", "google_books"]),
        INDEXER_PROVIDER => want_one_of(&["", "auto", "prowlarr", "torznab", "newznab"]),
        TORZNAB_URL => match value {
            Value::String(text) if text.trim().is_empty()
                || bokhylle_acquisition::torznab::TorznabClient::new(text, "", Vec::new()).is_ok() => Ok(()),
            _ => Err(AppError::Unprocessable(
                "Torznab URL must be an HTTP(S) API endpoint without embedded credentials or an API key".to_string(),
            )),
        },
        NEWZNAB_URL => match value {
            Value::String(text) if text.trim().is_empty()
                || bokhylle_acquisition::newznab::NewznabClient::new(text, "", Vec::new()).is_ok() => Ok(()),
            _ => Err(AppError::Unprocessable("Newznab URL must be an HTTP(S) API endpoint without embedded credentials or an API key".into())),
        },
        WATCH_FOLDER => match value {
            Value::String(text) if text.trim().is_empty() || std::path::Path::new(text).is_absolute() => Ok(()),
            _ => Err(AppError::Unprocessable("watch folder must be an absolute path".into())),
        },
        SABNZBD_URL => match value {
            Value::String(text) if text.trim().is_empty()
                || bokhylle_acquisition::sabnzbd::SabnzbdClient::new(text, "").is_ok() => Ok(()),
            _ => Err(AppError::Unprocessable("SABnzbd URL must be an HTTP(S) API endpoint without embedded credentials or an API key".into())),
        },
        TORZNAB_CATEGORIES | NEWZNAB_CATEGORIES => match value {
            Value::String(text)
                if text
                    .split(',')
                    .filter(|part| !part.trim().is_empty())
                    .count()
                    <= 20
                    && text.split(',').all(|part| {
                        part.trim().is_empty()
                            || part.trim().parse::<i32>().is_ok_and(|number| number > 0)
                    }) =>
            {
                Ok(())
            }
            _ => Err(AppError::Unprocessable(
                "indexer categories must be comma-separated positive numbers".to_string(),
            )),
        },
        RATINGS_PROVIDER => want_one_of(&[
            "same_as_metadata",
            "openlibrary",
            "google_books",
            "disabled",
        ]),
        SMTP_TLS => want_one_of(&["starttls", "tls", "none"]),
        IMPORT_STRATEGY => want_one_of(&["hardlink", "move", "copy"]),
        SCAN_ON_STARTUP | SECURE_COOKIES | TRUSTED_PROXY | CLEANUP_DOWNLOADS | WATCH_ENABLED | RETRIES_ENABLED | UPDATE_CHECKS => {
            want_bool()
        }
        SMTP_PORT => want_int(1, 65535),
        MAX_ATTACHMENT_MB => want_int(0, 4096),
        SCAN_INTERVAL_HOURS => want_number(0.0, 24.0 * 365.0),
        BACKUP_INTERVAL_HOURS => want_number(0.0, 24.0 * 365.0),
        BACKUP_KEEP => want_int(1, 365),
        RETRIES_MAX_DAYS => want_int(0, 365),
        _ => want_string(),
    }
}

#[derive(Clone)]
pub struct Settings {
    db: SqlitePool,
}

impl Settings {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    pub async fn raw(&self, key: &str) -> Result<Option<Value>, AppError> {
        let row: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.db)
            .await?;

        match row {
            Some(text) => Ok(Some(serde_json::from_str(&text).map_err(|error| {
                AppError::Unprocessable(format!("setting '{key}' is not valid JSON: {error}"))
            })?)),
            None => Ok(None),
        }
    }

    pub async fn set(&self, key: &str, value: &Value) -> Result<(), AppError> {
        let text = serde_json::to_string(value)
            .map_err(|error| AppError::Unprocessable(error.to_string()))?;

        sqlx::query(
            "INSERT INTO settings (key, value, updated_at)
             VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(key) DO UPDATE
                SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(text)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    pub async fn delete(&self, key: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(key)
            .execute(&self.db)
            .await?;

        Ok(())
    }

    pub async fn get_string(&self, key: &str, default: &str) -> Result<String, AppError> {
        if let Some(value) = env_override(key)? {
            return Ok(value);
        }

        match self.raw(key).await? {
            Some(Value::String(text)) => Ok(text),
            Some(other) => Err(AppError::Unprocessable(format!(
                "setting '{key}' must be a string, found {other}"
            ))),
            None => Ok(default.to_string()),
        }
    }

    pub async fn get_int(&self, key: &str, default: i64) -> Result<i64, AppError> {
        if let Some(value) = env_override(key)? {
            return value.trim().parse::<i64>().map_err(|_| {
                AppError::Unprocessable(format!(
                    "setting '{key}' must be an integer, found '{value}'"
                ))
            });
        }

        match self.raw(key).await? {
            Some(Value::Number(number)) => number.as_i64().ok_or_else(|| {
                AppError::Unprocessable(format!("setting '{key}' must be an integer"))
            }),
            Some(other) => Err(AppError::Unprocessable(format!(
                "setting '{key}' must be an integer, found {other}"
            ))),
            None => Ok(default),
        }
    }

    pub async fn get_float(&self, key: &str, default: f64) -> Result<f64, AppError> {
        if let Some(value) = env_override(key)? {
            return value.trim().parse::<f64>().map_err(|_| {
                AppError::Unprocessable(format!(
                    "setting '{key}' must be a number, found '{value}'"
                ))
            });
        }

        match self.raw(key).await? {
            Some(Value::Number(number)) => number.as_f64().ok_or_else(|| {
                AppError::Unprocessable(format!("setting '{key}' must be a number"))
            }),
            Some(other) => Err(AppError::Unprocessable(format!(
                "setting '{key}' must be a number, found {other}"
            ))),
            None => Ok(default),
        }
    }

    pub async fn get_bool(&self, key: &str, default: bool) -> Result<bool, AppError> {
        if let Some(value) = env_override(key)? {
            return parse_bool(key, &value);
        }

        match self.raw(key).await? {
            Some(Value::Bool(value)) => Ok(value),
            Some(other) => Err(AppError::Unprocessable(format!(
                "setting '{key}' must be a boolean, found {other}"
            ))),
            None => Ok(default),
        }
    }
}

pub fn env_var_for(key: &str) -> Option<&'static str> {
    match key {
        LIBRARY_ROOT => Some("BOKHYLLE_LIBRARY_DIR"),
        DOWNLOADS_DIR => Some("BOKHYLLE_DOWNLOADS_DIR"),
        SCAN_ON_STARTUP => Some("BOKHYLLE_SCAN_ON_STARTUP"),
        PREFERRED_FORMAT => Some("BOKHYLLE_PREFERRED_FORMAT"),
        PREFERRED_LANGUAGE => Some("BOKHYLLE_PREFERRED_LANGUAGE"),
        SECURE_COOKIES => Some("BOKHYLLE_SECURE_COOKIES"),
        TRUSTED_PROXY => Some("BOKHYLLE_TRUSTED_PROXY"),
        RETRIES_ENABLED => Some("BOKHYLLE_RETRIES_ENABLED"),
        RETRIES_MAX_DAYS => Some("BOKHYLLE_RETRIES_MAX_DAYS"),
        PROWLARR_URL => Some("BOKHYLLE_PROWLARR_URL"),
        PROWLARR_API_KEY => Some("BOKHYLLE_PROWLARR_API_KEY"),
        INDEXER_PROVIDER => Some("BOKHYLLE_INDEXER_PROVIDER"),
        TORZNAB_URL => Some("BOKHYLLE_TORZNAB_URL"),
        TORZNAB_API_KEY => Some("BOKHYLLE_TORZNAB_API_KEY"),
        TORZNAB_CATEGORIES => Some("BOKHYLLE_TORZNAB_CATEGORIES"),
        NEWZNAB_URL => Some("BOKHYLLE_NEWZNAB_URL"),
        NEWZNAB_API_KEY => Some("BOKHYLLE_NEWZNAB_API_KEY"),
        NEWZNAB_CATEGORIES => Some("BOKHYLLE_NEWZNAB_CATEGORIES"),
        SABNZBD_URL => Some("BOKHYLLE_SABNZBD_URL"),
        SABNZBD_API_KEY => Some("BOKHYLLE_SABNZBD_API_KEY"),
        SABNZBD_CATEGORY => Some("BOKHYLLE_SABNZBD_CATEGORY"),
        QBITTORRENT_URL => Some("BOKHYLLE_QBITTORRENT_URL"),
        QBITTORRENT_API_KEY => Some("BOKHYLLE_QBITTORRENT_API_KEY"),
        QBITTORRENT_USERNAME => Some("BOKHYLLE_QBITTORRENT_USERNAME"),
        QBITTORRENT_PASSWORD => Some("BOKHYLLE_QBITTORRENT_PASSWORD"),
        QBITTORRENT_CATEGORY => Some("BOKHYLLE_QBITTORRENT_CATEGORY"),
        CLEANUP_DOWNLOADS => Some("BOKHYLLE_CLEANUP_DOWNLOADS"),
        WATCH_ENABLED => Some("BOKHYLLE_WATCH_ENABLED"),
        WATCH_FOLDER => Some("BOKHYLLE_WATCH_FOLDER"),
        SMTP_HOST => Some("BOKHYLLE_SMTP_HOST"),
        SMTP_PORT => Some("BOKHYLLE_SMTP_PORT"),
        SMTP_USERNAME => Some("BOKHYLLE_SMTP_USERNAME"),
        SMTP_PASSWORD => Some("BOKHYLLE_SMTP_PASSWORD"),
        GOOGLE_BOOKS_API_KEY => Some("BOKHYLLE_GOOGLE_BOOKS_API_KEY"),
        UPDATE_CHECKS => Some("BOKHYLLE_UPDATE_CHECKS"),
        SMTP_FROM => Some("BOKHYLLE_SMTP_FROM"),
        SMTP_TLS => Some("BOKHYLLE_SMTP_TLS"),
        KINDLE_ADDRESS => Some("BOKHYLLE_KINDLE_ADDRESS"),
        MAX_ATTACHMENT_MB => Some("BOKHYLLE_MAX_ATTACHMENT_MB"),
        IMPORT_STRATEGY => Some("BOKHYLLE_IMPORT_STRATEGY"),
        SCAN_INTERVAL_HOURS => Some("BOKHYLLE_SCAN_INTERVAL_HOURS"),
        AMAZON_DOMAIN => Some("BOKHYLLE_AMAZON_DOMAIN"),
        _ => None,
    }
}

fn env_override(key: &str) -> Result<Option<String>, AppError> {
    let Some(name) = env_var_for(key) else {
        return Ok(None);
    };

    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(AppError::Unprocessable(format!(
            "environment variable {name} is not valid UTF-8"
        ))),
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool, AppError> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(AppError::Unprocessable(format!(
            "setting '{key}' must be a boolean, found '{value}'"
        ))),
    }
}
