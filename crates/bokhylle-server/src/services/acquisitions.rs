use sqlx::SqlitePool;

use crate::acquisition;
use crate::auth::{self, Role, User};
use crate::error::AppError;

/// File review changes the publication imported for a shared acquisition.
/// The adult who started it may decide; other participants keep that choice.
pub async fn require_review_access(
    pool: &SqlitePool,
    user: &User,
    id: &str,
) -> Result<(), AppError> {
    if !auth::can_acquire(pool, user).await? {
        return Err(AppError::Forbidden);
    }
    let acquisition = acquisition::get(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("acquisition not found".into()))?;
    if user.role == Role::Admin || acquisition.user_id == Some(user.id) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
