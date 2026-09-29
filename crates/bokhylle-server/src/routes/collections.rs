use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::AuthUser;
use crate::collections;
use crate::error::AppError;
use crate::library::queries;

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDetail {
    id: i64,
    name: String,
    book_count: i64,
    books: Vec<queries::BookSummary>,
}

pub async fn list_collections(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<collections::CollectionSummary>>, AppError> {
    Ok(Json(collections::list(&state.db).await?))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateCollection {
    pub name: String,
}

pub async fn create_collection(
    _user: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateCollection>,
) -> Result<(StatusCode, Json<collections::CollectionSummary>), AppError> {
    let collection = collections::create(&state.db, &body.name).await?;
    Ok((StatusCode::CREATED, Json(collection)))
}

pub async fn get_collection(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<CollectionDetail>, AppError> {
    let Some(collection) = collections::get(&state.db, id).await? else {
        return Err(AppError::NotFound("collection not found".to_string()));
    };
    let books = queries::books_in_collection(&state.db, id).await?;
    Ok(Json(CollectionDetail {
        id: collection.id,
        name: collection.name,
        book_count: collection.book_count,
        books,
    }))
}

pub async fn delete_collection(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    if collections::delete(&state.db, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("collection not found".to_string()))
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddBook {
    pub book_id: i64,
}

pub async fn add_book(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AddBook>,
) -> Result<StatusCode, AppError> {
    collections::add_book(&state.db, id, body.book_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_book(
    _user: AuthUser,
    State(state): State<AppState>,
    Path((id, book_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    if collections::remove_book(&state.db, id, book_id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound(
            "book is not in this collection".to_string(),
        ))
    }
}

pub async fn book_collections(
    _user: AuthUser,
    State(state): State<AppState>,
    Path(book_id): Path<i64>,
) -> Result<Json<Vec<collections::CollectionSummary>>, AppError> {
    Ok(Json(collections::for_book(&state.db, book_id).await?))
}
