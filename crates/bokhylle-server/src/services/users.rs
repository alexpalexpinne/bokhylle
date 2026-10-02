//! Household account creation, including the administrator's initial child
//! shelf. Credentials, profile settings and assignments commit together.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::AppState;
use crate::auth::{PreparedUser, Role, User, infer_credential_type};
use crate::error::AppError;

/// IDs of the bundled, age-neutral Bokhylle profile marks.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProfileMark {
    Fox,
    Owl,
    Cat,
    Bear,
    Whale,
    Book,
    Tree,
    Mountain,
    Moon,
    Leaf,
}

impl ProfileMark {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fox => "fox",
            Self::Owl => "owl",
            Self::Cat => "cat",
            Self::Bear => "bear",
            Self::Whale => "whale",
            Self::Book => "book",
            Self::Tree => "tree",
            Self::Mountain => "mountain",
            Self::Moon => "moon",
            Self::Leaf => "leaf",
        }
    }
}

pub async fn set_profile_mark(
    pool: &sqlx::SqlitePool,
    user_id: i64,
    mark: Option<ProfileMark>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET avatar_preset = ? WHERE id = ?")
        .bind(mark.map(ProfileMark::as_str))
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserInput {
    pub username: String,
    pub password: Option<String>,
    pub credential: Option<String>,
    pub credential_type: Option<String>,
    pub role: Option<String>,
    pub display_name: Option<String>,
    pub preferred_languages: Option<Vec<String>>,
    pub profile_type: Option<String>,
    /// Children only: permits request submission and basic catalogue search.
    pub can_request: Option<bool>,
    /// Children only: allow public catalogue browsing and suggestions.
    pub can_discover: Option<bool>,
    /// Adults only: allow adding new books to the shared library.
    pub can_acquire: Option<bool>,
    /// Children only: owned household books to assign in the creation transaction.
    pub starting_book_ids: Option<Vec<i64>>,
    /// Optional bundled profile mark. Null or omitted uses initials.
    pub avatar_preset: Option<ProfileMark>,
}

pub struct CreatedUser {
    pub user: User,
    pub credential_type: String,
    pub preferred_languages: Vec<String>,
    pub can_request: bool,
    pub can_discover: bool,
    pub can_acquire: bool,
}

pub async fn create(
    state: &AppState,
    actor: &User,
    input: CreateUserInput,
) -> Result<CreatedUser, AppError> {
    let role = match input.role.as_deref() {
        None | Some("user") => Role::User,
        Some("admin") => Role::Admin,
        _ => {
            return Err(AppError::BadRequest(
                "role must be 'admin' or 'user'".into(),
            ));
        }
    };
    let profile = input.profile_type.as_deref().unwrap_or("adult");
    let starting_books = input.starting_book_ids.unwrap_or_default();
    if starting_books.len() > 1000 {
        return Err(AppError::Unprocessable(
            "choose at most 1000 starting books".into(),
        ));
    }
    if !starting_books.is_empty() && profile != "child" {
        return Err(AppError::Unprocessable(
            "starting books are only for child profiles".into(),
        ));
    }
    let starting_books: BTreeSet<i64> = starting_books.into_iter().collect();
    let secret = input
        .credential
        .or(input.password)
        .ok_or_else(|| AppError::Unprocessable("a PIN or password is required".into()))?;
    let credential_type = input
        .credential_type
        .unwrap_or_else(|| infer_credential_type(role, &secret).to_string());
    let prepared = PreparedUser::new(&input.username, &secret, role, &credential_type, profile)?;
    let display_name = input
        .display_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let preferred_languages: Vec<String> = input
        .preferred_languages
        .unwrap_or_default()
        .iter()
        .map(|language| language.trim().to_ascii_lowercase())
        .filter(|language| !language.is_empty())
        .collect();
    let languages_json = serde_json::to_string(&preferred_languages)
        .map_err(|error| AppError::Unprocessable(error.to_string()))?;
    let can_request = input.can_request.unwrap_or(true);
    let can_discover = profile == "child" && input.can_discover.unwrap_or(false);
    let can_acquire =
        profile == "adult" && (role == Role::Admin || input.can_acquire.unwrap_or(true));

    let mut tx = state.db.begin().await?;
    for book_id in &starting_books {
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM book_files f JOIN editions e ON e.id = f.edition_id WHERE e.book_id = ?)",
        ).bind(book_id).fetch_one(&mut *tx).await?;
        if !owned {
            return Err(AppError::Unprocessable(format!(
                "starting book {book_id} is not in the household library"
            )));
        }
        crate::services::sharing::require_access_tx(&mut tx, actor.id, *book_id).await?;
    }
    let mut user = prepared.insert(&mut tx).await?;
    sqlx::query(
        "UPDATE users SET display_name = ?, preferred_languages = ?, preferred_language = ?,
            can_request = ?, can_discover = ?, can_acquire = ?, avatar_preset = ? WHERE id = ?",
    )
    .bind(&display_name)
    .bind(languages_json)
    .bind(preferred_languages.first())
    .bind(can_request)
    .bind(can_discover)
    .bind(can_acquire)
    .bind(input.avatar_preset.map(ProfileMark::as_str))
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    for book_id in starting_books {
        crate::user_books::add_tx(&mut tx, user.id, book_id, "assigned").await?;
    }
    tx.commit().await?;
    user.display_name = display_name;
    user.preferred_language = preferred_languages.first().cloned();
    Ok(CreatedUser {
        user,
        credential_type,
        preferred_languages,
        can_request,
        can_discover,
        can_acquire,
    })
}
