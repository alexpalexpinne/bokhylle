//! Per-profile book access, independent of personal shelf membership.
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::{auth::User, error::AppError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BookSharing {
    Private,
    Shared,
}

impl BookSharing {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
        }
    }
}

/// SQL identifiers come only from source code; viewer ids are integers.
pub fn predicate(book_column: &str, viewer_id: i64) -> String {
    format!("(EXISTS (SELECT 1 FROM books unmanaged WHERE unmanaged.id = {book_column} AND unmanaged.sharing_managed = 0)
        OR EXISTS (SELECT 1 FROM book_access access WHERE access.book_id = {book_column}
                   AND ((access.is_owner = 1 AND (access.user_id = {viewer_id} OR access.sharing = 'shared'))
                       OR (access.user_id = {viewer_id} AND EXISTS (SELECT 1 FROM users reader WHERE reader.id = access.user_id AND reader.profile_type = 'child'))
                       OR (EXISTS (SELECT 1 FROM users owner WHERE owner.id = access.user_id AND owner.profile_type = 'child')
                           AND EXISTS (SELECT 1 FROM users actor WHERE actor.id = {viewer_id} AND actor.role = 'admin' AND actor.profile_type = 'adult')))))")
}

/// An author known only through private books must not reveal those books.
/// Public catalogue identities and the viewer's own follows remain available.
pub fn author_predicate(author_column: &str, viewer_id: i64) -> String {
    let visibility = predicate("known.book_id", viewer_id);
    format!("(NOT EXISTS(SELECT 1 FROM book_authors known WHERE known.author_id = {author_column})
        OR EXISTS(SELECT 1 FROM book_authors known WHERE known.author_id = {author_column} AND {visibility})
        OR EXISTS(SELECT 1 FROM author_follows followed WHERE followed.author_id = {author_column} AND followed.user_id = {viewer_id}))")
}

pub async fn require_author_access(
    pool: &SqlitePool,
    user_id: i64,
    author_id: i64,
) -> Result<(), AppError> {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM authors a WHERE a.id = ? AND {})",
        author_predicate("a.id", user_id)
    );
    let visible: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(author_id)
        .fetch_one(pool)
        .await?;
    if !visible {
        return Err(AppError::NotFound("author not found".into()));
    }
    Ok(())
}

pub async fn can_access(pool: &SqlitePool, user_id: i64, book_id: i64) -> Result<bool, AppError> {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM books b WHERE b.id = ? AND {})",
        predicate("b.id", user_id)
    );
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(book_id)
        .fetch_one(pool)
        .await?)
}

pub async fn require_access(pool: &SqlitePool, user_id: i64, book_id: i64) -> Result<(), AppError> {
    if !can_access(pool, user_id, book_id).await? {
        return Err(AppError::NotFound("book not found".into()));
    }
    Ok(())
}

pub async fn require_access_tx(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    book_id: i64,
) -> Result<(), AppError> {
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM books b WHERE b.id = ? AND {})",
        predicate("b.id", user_id)
    );
    let visible: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(book_id)
        .fetch_one(&mut **tx)
        .await?;
    if !visible {
        return Err(AppError::NotFound("book not found".into()));
    }
    Ok(())
}

/// A shelf addition does not create adult ownership. Children retain their
/// explicitly assigned access independently of household browsing.
pub async fn grant_tx(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    book_id: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO book_access (user_id, book_id, sharing, is_owner)
        SELECT id, ?, 'private', 0 FROM users WHERE id = ? AND profile_type = 'child'
        ON CONFLICT(user_id, book_id) DO NOTHING",
    )
    .bind(book_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE books SET sharing_managed = 1 WHERE id = ?
        AND EXISTS(SELECT 1 FROM users WHERE id = ? AND profile_type = 'child')
        AND NOT EXISTS(SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id WHERE e.book_id = books.id)")
        .bind(book_id).bind(user_id).execute(&mut **tx).await?;
    Ok(())
}

/// Freeze sharing on an explicit acquisition, preserving an existing owner's
/// choice. Borrowing a household book never calls this operation.
pub async fn own_tx(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    book_id: i64,
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO book_access (user_id, book_id, sharing, is_owner)
        SELECT id, ?, CASE WHEN profile_type = 'child' THEN 'private'
            WHEN EXISTS (SELECT 1 FROM books WHERE id = ? AND sharing_managed = 0)
             AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id WHERE e.book_id = ?)
            THEN 'shared' ELSE default_book_sharing END, 1
        FROM users WHERE id = ? ON CONFLICT(user_id, book_id) DO UPDATE SET
            sharing = CASE WHEN book_access.is_owner = 1 THEN book_access.sharing ELSE excluded.sharing END,
            is_owner = 1")
        .bind(book_id).bind(book_id).bind(book_id).bind(user_id).execute(&mut **tx).await?;
    sqlx::query("UPDATE books SET sharing_managed = 1 WHERE id = ? AND (
        EXISTS(SELECT 1 FROM users WHERE id = ? AND profile_type = 'adult')
        OR NOT EXISTS(SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id WHERE e.book_id = books.id))")
        .bind(book_id).bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// A choice made while getting a book is recorded before background work
/// starts. It changes only the requester's grant.
pub async fn choose(
    pool: &SqlitePool,
    user_id: i64,
    book_id: i64,
    sharing: Option<BookSharing>,
) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    own_tx(&mut tx, user_id, book_id).await?;
    if let Some(sharing) = sharing {
        sqlx::query("UPDATE book_access SET sharing = ? WHERE user_id = ? AND book_id = ? AND EXISTS (SELECT 1 FROM users WHERE id = ? AND profile_type = 'adult')")
            .bind(sharing.as_str()).bind(user_id).bind(book_id).bind(user_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookSharingState {
    pub sharing: Option<BookSharing>,
    pub shared_in_household: bool,
}

pub async fn state(
    pool: &SqlitePool,
    user_id: i64,
    book_id: i64,
) -> Result<BookSharingState, AppError> {
    let sharing: Option<String> = sqlx::query_scalar(
        "SELECT sharing FROM book_access WHERE user_id = ? AND book_id = ? AND is_owner = 1",
    )
    .bind(user_id)
    .bind(book_id)
    .fetch_optional(pool)
    .await?;
    let shared_in_household = can_access(pool, -1, book_id).await?;
    Ok(BookSharingState {
        sharing: sharing.map(|value| {
            if value == "private" {
                BookSharing::Private
            } else {
                BookSharing::Shared
            }
        }),
        shared_in_household,
    })
}

/// All ids are checked before any write. Each reader changes only their own
/// grant; another reader's access or decision to share is never overwritten.
pub async fn set(
    pool: &SqlitePool,
    user: &User,
    book_ids: &[i64],
    sharing: BookSharing,
) -> Result<(), AppError> {
    if crate::auth::profile_type(pool, user.id).await? != "adult" {
        return Err(AppError::Forbidden);
    }
    if book_ids.is_empty() || book_ids.len() > 1000 {
        return Err(AppError::Unprocessable(
            "select between 1 and 1,000 books".into(),
        ));
    }
    let mut tx = pool.begin().await?;
    for &book_id in book_ids {
        let owns: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM book_access WHERE user_id = ? AND book_id = ? AND is_owner = 1)",
        )
        .bind(user.id)
        .bind(book_id)
        .fetch_one(&mut *tx)
        .await?;
        if !owns {
            return Err(AppError::Forbidden);
        }
    }
    for &book_id in book_ids {
        sqlx::query("UPDATE books SET sharing_managed = 1 WHERE id = ?")
            .bind(book_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE book_access SET sharing = ? WHERE user_id = ? AND book_id = ?")
            .bind(sharing.as_str())
            .bind(user.id)
            .bind(book_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
