//! Sending an owned book to the signed-in profile's reader. The file is
//! chosen with the same format-then-language rule as acquisition.

use crate::AppState;
use crate::auth::User;
use crate::delivery::{self, Delivery};
use crate::error::AppError;
use crate::library::queries;

/// Children cannot send books; their shelf and downloads stay parent-managed.
pub async fn send_book(
    state: &AppState,
    user: &User,
    book_id: i64,
    target_id: Option<i64>,
) -> Result<Delivery, AppError> {
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
