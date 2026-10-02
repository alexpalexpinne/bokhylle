//! Book requests: durable intent that an administrator decides. The HTTP routes and
//! the MCP tools both come through here.

use serde::Serialize;

use crate::AppState;
use crate::auth::{Role, User};
use crate::book_requests;
use crate::discovery;
use crate::error::AppError;
use crate::library::import_metadata;

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestOutcome {
    pub request: book_requests::BookRequestView,
    pub duplicate: bool,
}

/// Adults always may; a child only when their profile allows request
/// submission. Children without it receive books an adult assigns.
pub async fn may_request(state: &AppState, user_id: i64) -> Result<bool, AppError> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT profile_type, can_request FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    match row {
        Some((profile, can_request)) => Ok(profile != "child" || can_request != 0),
        None => Ok(false),
    }
}

/// Either child permission permits reading public catalogue metadata. Only
/// can_request permits creating an adult-approval request.
pub async fn may_browse_catalogue(state: &AppState, user_id: i64) -> Result<bool, AppError> {
    let row: Option<(String, i64, i64)> =
        sqlx::query_as("SELECT profile_type, can_request, can_discover FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(
        matches!(row, Some((ref profile, can_request, can_discover)) if profile != "child" || can_request != 0 || can_discover != 0),
    )
}

pub async fn may_discover(state: &AppState, user_id: i64) -> Result<bool, AppError> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT profile_type, can_discover FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(matches!(row, Some((ref profile, can_discover)) if profile != "child" || can_discover != 0))
}

pub async fn is_adult(state: &AppState, user_id: i64) -> Result<bool, AppError> {
    Ok(crate::auth::profile_type(&state.db, user_id).await? == "adult")
}

/// Creates a request from a catalogue identity, exactly like the HTTP route:
/// resolve metadata, upsert the book, register intent, notify the adults.
pub async fn create(
    state: &AppState,
    user: &User,
    provider: &str,
    provider_key: &str,
) -> Result<RequestOutcome, AppError> {
    create_with_sharing(state, user, provider, provider_key, None).await
}

pub async fn create_with_sharing(
    state: &AppState,
    user: &User,
    provider: &str,
    provider_key: &str,
    sharing: Option<crate::services::sharing::BookSharing>,
) -> Result<RequestOutcome, AppError> {
    if !may_request(state, user.id).await? {
        return Err(AppError::Forbidden);
    }
    let provider_key = provider_key.trim();
    if provider_key.is_empty() {
        return Err(AppError::BadRequest(
            "providerKey must not be empty".to_string(),
        ));
    }
    let metadata = discovery::resolve_metadata(state, Some(provider), provider_key).await?;
    let Some(metadata) = metadata else {
        return Err(AppError::NotFound(
            "the book was not found in the metadata catalogue".to_string(),
        ));
    };
    let book_id = import_metadata::upsert_book_from_metadata(&state.db, &metadata).await?;
    create_for_book_with_sharing(state, user, book_id, sharing).await
}

/// Registers a request for a book already in the catalogue.
pub async fn create_for_book(
    state: &AppState,
    user: &User,
    book_id: i64,
) -> Result<RequestOutcome, AppError> {
    create_for_book_with_sharing(state, user, book_id, None).await
}

pub async fn create_for_book_with_sharing(
    state: &AppState,
    user: &User,
    book_id: i64,
    sharing: Option<crate::services::sharing::BookSharing>,
) -> Result<RequestOutcome, AppError> {
    if !may_request(state, user.id).await? {
        return Err(AppError::Forbidden);
    }
    let (id, duplicate) =
        book_requests::create_with_sharing(&state.db, book_id, user.id, sharing).await?;
    if !duplicate {
        tracing::info!(
            request_id = id,
            book_id,
            requester = user.id,
            "request.created"
        );
        notify_admins(state, user.id, book_id).await;
    }
    let request = book_requests::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("request not found".to_string()))?;
    Ok(RequestOutcome { request, duplicate })
}

pub async fn list(
    state: &AppState,
    user: &User,
) -> Result<Vec<book_requests::BookRequestView>, AppError> {
    if state.demo.is_some() && user.role == Role::Admin {
        let (_, child) = crate::demo::pair_names(&user.username).ok_or(AppError::Forbidden)?;
        let child_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
            .bind(child)
            .fetch_one(&state.db)
            .await?;
        return book_requests::list(&state.db, child_id, false).await;
    }
    let all = user.role == Role::Admin && is_adult(state, user.id).await?;
    book_requests::list(&state.db, user.id, all).await
}

async fn may_decide(state: &AppState, user: &User, requester_id: i64) -> Result<bool, AppError> {
    if user.role != Role::Admin || !is_adult(state, user.id).await? {
        return Ok(false);
    }
    if state.demo.is_some() {
        let (adult, child) = crate::demo::pair_names(&user.username).ok_or(AppError::Forbidden)?;
        let requester: Option<String> =
            sqlx::query_scalar("SELECT username FROM users WHERE id = ?")
                .bind(requester_id)
                .fetch_optional(&state.db)
                .await?;
        return Ok(requester.is_some_and(|name| name == adult || name == child));
    }
    Ok(true)
}

/// Approval: one transaction for the decision and the acquisition, or a
/// household copy that already satisfies the requester's policy. The deciding
/// administrator becomes the acquisition's manager.
pub async fn approve(
    state: &AppState,
    user: &User,
    id: i64,
) -> Result<book_requests::BookRequestView, AppError> {
    let Some(request) = book_requests::raw(&state.db, id).await? else {
        return Err(AppError::NotFound("request not found".to_string()));
    };
    if !may_decide(state, user, request.user_id).await? {
        return Err(AppError::Forbidden);
    }
    if request.status != "requested" {
        return Err(AppError::Unprocessable(
            "this request was already decided".to_string(),
        ));
    }

    let child_request = crate::auth::profile_type(&state.db, request.user_id).await? == "child";
    if child_request {
        crate::delivery::ensure_default_delivery(state, request.user_id).await?;
    }

    let (languages, preferred_format) =
        crate::updates::user_preferences(state, request.user_id).await?;

    // A copy the household already owns and the requester can read settles
    // the request without touching the acquisition pipeline.
    if let Some(file_id) = crate::library::queries::existing_file_in_languages(
        &state.db,
        request.book_id,
        preferred_format.as_deref(),
        &languages,
    )
    .await?
    {
        let mut tx = state.db.begin().await?;
        if !book_requests::mark_approved_tx(&mut tx, id, user.id).await? {
            tx.rollback().await.ok();
            return Err(AppError::Unprocessable(
                "this request was already decided".to_string(),
            ));
        }
        crate::user_books::add_tx(&mut tx, request.user_id, request.book_id, "book_request")
            .await?;
        apply_request_sharing(&mut tx, id, request.user_id, request.book_id).await?;
        tx.commit().await?;

        tracing::info!(
            request_id = id,
            book_id = request.book_id,
            file_id,
            decider = user.id,
            "request.fulfilled_from_library"
        );
        if child_request {
            match crate::delivery::deliver(state, request.user_id, request.book_id, file_id, None)
                .await
            {
                Ok(delivery) => {
                    let failed = delivery.status == "FAILED";
                    crate::notifications::create_for_book(
                        &state.db,
                        request.user_id,
                        Some(request.book_id),
                        if failed { "failed" } else { "sent" },
                        if failed {
                            "Could not send to your reader"
                        } else {
                            "Sent to your reader"
                        },
                        Some(
                            delivery
                                .error_message
                                .as_deref()
                                .unwrap_or(&delivery.address),
                        ),
                        None,
                    )
                    .await
                    .ok();
                }
                Err(error) => {
                    tracing::warn!(request_id = id, %error, "request.delivery.failed");
                    crate::notifications::create_for_book(
                        &state.db,
                        request.user_id,
                        Some(request.book_id),
                        "failed",
                        "Could not send to your reader",
                        Some(&error.to_string()),
                        None,
                    )
                    .await
                    .ok();
                }
            }
        } else {
            crate::notifications::create_for_book(
                &state.db,
                request.user_id,
                Some(request.book_id),
                "ready",
                "Your book request is ready",
                Some("It is already in the household library and was added to your shelf."),
                None,
            )
            .await
            .ok();
        }

        return book_requests::get(&state.db, id)
            .await?
            .ok_or_else(|| AppError::NotFound("request not found".to_string()));
    }

    // Otherwise the decision and the shared acquisition commit together; the
    // requester's membership (and shelf) comes from acquisition_requests.
    let mut tx = state.db.begin().await?;
    if !book_requests::mark_approved_tx(&mut tx, id, user.id).await? {
        tx.rollback().await.ok();
        return Err(AppError::Unprocessable(
            "this request was already decided".to_string(),
        ));
    }
    let (acquisition_id, duplicate) = crate::acquisition::create_tx_with_languages(
        &mut tx,
        request.book_id,
        Some(request.user_id),
        preferred_format,
        languages,
        child_request,
        false,
    )
    .await?;
    sqlx::query("UPDATE book_requests SET acquisition_id = ? WHERE id = ?")
        .bind(&acquisition_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE acquisition_requests SET source = 'book_request'
         WHERE acquisition_id = ? AND user_id = ?",
    )
    .bind(&acquisition_id)
    .bind(request.user_id)
    .execute(&mut *tx)
    .await?;
    apply_request_sharing(&mut tx, id, request.user_id, request.book_id).await?;
    tx.commit().await?;

    tracing::info!(
        request_id = id,
        acquisition_id = %acquisition_id,
        decider = user.id,
        "request.approved"
    );
    if !duplicate {
        crate::acquisition_pipeline::spawn(state, acquisition_id.clone());
    }

    crate::notifications::create_for_book(
        &state.db,
        request.user_id,
        Some(request.book_id),
        "approved",
        "Your book request was approved",
        Some(if child_request {
            "Bokhylle is getting it and will send it to your reader when ready."
        } else {
            "Bokhylle is getting it for your shelf."
        }),
        Some(&acquisition_id),
    )
    .await
    .ok();

    book_requests::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("request not found".to_string()))
}

pub async fn decline(
    state: &AppState,
    user: &User,
    id: i64,
) -> Result<book_requests::BookRequestView, AppError> {
    let Some(request) = book_requests::raw(&state.db, id).await? else {
        return Err(AppError::NotFound("request not found".to_string()));
    };
    if !may_decide(state, user, request.user_id).await? {
        return Err(AppError::Forbidden);
    }
    if !book_requests::mark_declined(&state.db, id, user.id).await? {
        return Err(AppError::Unprocessable(
            "this request was already decided".to_string(),
        ));
    }
    crate::notifications::create_for_book(
        &state.db,
        request.user_id,
        Some(request.book_id),
        "declined",
        "Your book request was declined",
        None,
        None,
    )
    .await
    .ok();
    book_requests::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound("request not found".to_string()))
}

async fn notify_admins(state: &AppState, requester_id: i64, book_id: i64) {
    let requester: Option<String> =
        sqlx::query_scalar("SELECT COALESCE(display_name, username) FROM users WHERE id = ?")
            .bind(requester_id)
            .fetch_optional(&state.db)
            .await
            .ok()
            .flatten();
    let requester = requester.unwrap_or_else(|| "Someone".to_string());
    let admins: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM users
         WHERE profile_type = 'adult' AND role = 'admin' AND disabled = 0 AND id != ?
         ORDER BY id",
    )
    .bind(requester_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    for adult_id in admins {
        crate::notifications::create_for_book(
            &state.db,
            adult_id,
            Some(book_id),
            "request",
            &format!("{requester} requested a book"),
            Some("Open Requests to approve or decline."),
            None,
        )
        .await
        .ok();
    }
}

async fn apply_request_sharing(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request_id: i64,
    user_id: i64,
    book_id: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE book_access SET sharing = CASE
            WHEN EXISTS(SELECT 1 FROM users WHERE id = book_access.user_id AND profile_type = 'child') THEN 'private'
            ELSE (SELECT sharing FROM book_requests WHERE id = ?) END
        WHERE user_id = ? AND book_id = ?",
    )
    .bind(request_id)
    .bind(user_id)
    .bind(book_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
