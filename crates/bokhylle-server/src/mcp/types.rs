//! MCP input and output schemas. Existing tool fields are a public API:
//! agents cache definitions, so preserve their names and fields when adding
//! tools.

use rmcp::schemars;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchBooksInput {
    /// Title, author or ISBN text.
    pub query: String,
    /// library | catalogue | auto (library first, catalogue as fallback).
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBook {
    pub id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueBook {
    pub provider: String,
    pub provider_key: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchBooksOutput {
    /// Owned books, household-wide for adults and shelf-only for children.
    pub library: Vec<LibraryBook>,
    /// Public metadata catalogue results, never ownership or availability.
    pub catalogue: Vec<CatalogueBook>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetBookInput {
    pub book_id: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProgressOutput {
    pub percentage: f64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BookOutput {
    pub id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub series: Option<String>,
    pub series_number: Option<String>,
    pub description: Option<String>,
    pub publication_year: Option<i64>,
    pub on_shelf: bool,
    pub preference: Option<String>,
    pub available: bool,
    pub progress: Option<ProgressOutput>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShelfInput {
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShelfOutput {
    pub items: Vec<LibraryBook>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContinueItem {
    pub book: LibraryBook,
    pub percentage: f64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContinueOutput {
    pub items: Vec<ContinueItem>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddBookInput {
    pub book_id: i64,
    /// Also send it to the profile's default reader once available.
    #[serde(default)]
    pub send_to_reader: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddCatalogueBookInput {
    /// Metadata provider returned by search_books.
    pub provider: String,
    /// Provider book key returned by search_books.
    pub provider_key: String,
    /// epub or any; otherwise the profile preference applies.
    #[serde(default)]
    pub preferred_format: Option<String>,
    /// Override the profile's accepted languages for this acquisition.
    #[serde(default)]
    pub preferred_language: Option<String>,
    /// Send to the profile's default reader when ready.
    #[serde(default)]
    pub send_to_reader: Option<bool>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueAcquisitionOutput {
    pub id: String,
    pub status: String,
    pub duplicate: bool,
    pub book_id: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentOutput {
    pub book_id: i64,
    /// requested | looking | getting | ready | declined | unavailable
    pub phase: String,
    pub message: String,
    pub request_id: Option<i64>,
    pub acquisition_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SendToReaderInput {
    pub book_id: i64,
    #[serde(default)]
    pub target_id: Option<i64>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryOutput {
    pub book_id: i64,
    pub file_id: i64,
    pub status: String,
    pub address: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestBookInput {
    /// Metadata provider, e.g. openlibrary.
    pub provider: String,
    /// The provider's book key, exactly as search_books returned it.
    pub provider_key: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestOutput {
    pub id: i64,
    pub book_id: i64,
    pub title: String,
    pub authors: Vec<String>,
    pub requester: String,
    pub status: String,
    pub phase: String,
    pub acquisition_id: Option<String>,
    pub error_code: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RequestsOutput {
    pub items: Vec<RequestOutput>,
}
