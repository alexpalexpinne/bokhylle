//! Sending an owned book to the signed-in profile's reader. The file is
//! chosen with the same format-then-language rule as acquisition.

use crate::AppState;
use crate::auth::User;
use crate::delivery::{self, Delivery};
use crate::error::AppError;
use crate::library::queries;

pub struct AcquisitionDestination {
    target_id: i64,
    address: String,
}

/// Resolve an explicit reader before creating a book or acquisition. Only
/// the signed-in profile's enabled targets can become its destination.
pub async fn acquisition_destination(
    state: &AppState,
    user: &User,
    send_to_reader: bool,
    target_id: Option<i64>,
) -> Result<Option<AcquisitionDestination>, AppError> {
    let Some(target_id) = target_id else {
        return Ok(None);
    };
    if !send_to_reader {
        return Err(AppError::BadRequest(
            "targetId requires sendToReader".into(),
        ));
    }
    if crate::auth::profile_type(&state.db, user.id).await? == "child" {
        return Err(AppError::Forbidden);
    }
    let (_, address) = delivery::resolve_target(state, user.id, Some(target_id)).await?;
    Ok(Some(AcquisitionDestination { target_id, address }))
}

/// Commit an explicit destination with the request, so a fast completion or
/// a later default-reader change cannot send this book somewhere else.
#[allow(clippy::too_many_arguments)]
pub async fn create_acquisition(
    state: &AppState,
    user: &User,
    book_id: i64,
    preferred_format: Option<String>,
    languages: Vec<String>,
    send_to_reader: bool,
    ask_before_download: bool,
    destination: Option<AcquisitionDestination>,
) -> Result<(crate::acquisition::Acquisition, bool), AppError> {
    let Some(destination) = destination else {
        return crate::acquisition::create_with_languages(
            &state.db,
            book_id,
            Some(user.id),
            preferred_format,
            languages,
            send_to_reader,
            ask_before_download,
        )
        .await;
    };
    let mut tx = state
        .db
        .begin_with(sqlx::AssertSqlSafe("BEGIN IMMEDIATE".to_string()))
        .await?;
    let (id, duplicate) = crate::acquisition::create_tx_with_languages(
        &mut tx,
        book_id,
        Some(user.id),
        preferred_format,
        languages,
        false,
        ask_before_download,
    )
    .await?;
    sqlx::query(
        "UPDATE acquisition_requests SET deliver_on_ready = 1,
        delivery_target_id = ?, delivery_address = ? WHERE acquisition_id = ? AND user_id = ?",
    )
    .bind(destination.target_id)
    .bind(destination.address)
    .bind(&id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let acquisition = crate::acquisition::get(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".into()))?;
    Ok((acquisition, duplicate))
}

/// Schedule or cancel only the signed-in adult requester's delivery intent.
/// The conditional update closes the race with the transition to Ready.
pub async fn schedule_for_acquisition(
    state: &AppState,
    user: &User,
    acquisition_id: &str,
    enabled: bool,
    target_id: Option<i64>,
) -> Result<crate::acquisition::AcquisitionView, AppError> {
    if crate::auth::profile_type(&state.db, user.id).await? == "child" {
        return Err(AppError::Forbidden);
    }
    let acquisition = crate::acquisition::get(&state.db, acquisition_id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".into()))?;
    crate::services::sharing::require_access(&state.db, user.id, acquisition.book_id).await?;
    let member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM acquisition_requests WHERE acquisition_id = ? AND user_id = ?)",
    )
    .bind(acquisition_id)
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    if !member {
        return Err(AppError::Forbidden);
    }
    let (target, address) = if enabled {
        let (target, address) = delivery::resolve_target(state, user.id, target_id).await?;
        (target, Some(address))
    } else {
        (None, None)
    };
    let updated = sqlx::query(
        "UPDATE acquisition_requests
         SET deliver_on_ready = ?, delivery_target_id = ?, delivery_address = ?
         WHERE acquisition_id = ? AND user_id = ?
           AND EXISTS(SELECT 1 FROM acquisitions a WHERE a.id = acquisition_requests.acquisition_id
             AND a.status IN ('REQUESTED', 'SEARCHING', 'EVALUATING', 'QUEUED',
                 'DOWNLOADING', 'DOWNLOADED', 'INSPECTING', 'IDENTIFIED',
                 'IMPORTING', 'NEEDS_SELECTION', 'NEEDS_REVIEW'))",
    )
    .bind(enabled)
    .bind(target)
    .bind(address)
    .bind(acquisition_id)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "This download has finished or stopped. Refresh the book to send its available file."
                .into(),
        ));
    }
    crate::acquisition::view(&state.db, user.id, acquisition_id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".into()))
}

/// Children cannot send books; their shelf and downloads stay parent-managed.
pub async fn send_book(
    state: &AppState,
    user: &User,
    book_id: i64,
    target_id: Option<i64>,
) -> Result<Delivery, AppError> {
    crate::services::sharing::require_access(&state.db, user.id, book_id).await?;
    if crate::auth::profile_type(&state.db, user.id).await? == "child" {
        return Err(AppError::Forbidden);
    }
    let (languages, format) = crate::updates::user_preferences(state, user.id).await?;
    let preferred_format = format.or(user.preferred_format.clone());
    let Some(file_id) = queries::existing_file_in_languages(
        &state.db,
        book_id,
        preferred_format.as_deref(),
        &languages,
    )
    .await?
    else {
        return Err(AppError::NotFound(
            "no file on this book matches the reading preferences".to_string(),
        ));
    };
    delivery::deliver(state, user.id, book_id, file_id, target_id).await
}
