pub mod acquisitions;
pub mod admin;
pub mod agent_tokens;
pub mod auth;
pub mod catalogues;
pub mod child_readers;
pub mod classification;
pub mod collections;
pub mod delivery;
pub mod discover;
pub mod health;
pub mod household;
pub mod image_cache;
pub mod kosync;
pub mod library;
pub mod notifications;
pub mod opds;
pub mod registry;
pub mod requests;
pub mod responses;
pub mod spotlight;

use crate::error::AppError;

pub async fn fallback() -> AppError {
    AppError::NotFound("resource not found".to_string())
}
