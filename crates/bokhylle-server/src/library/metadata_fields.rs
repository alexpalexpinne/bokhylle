//! Field ownership shared by provider enrichment, embedded metadata and admin
//! corrections. Automatic candidates are retained while a field is manual.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MetadataField {
    Title,
    Authors,
    Description,
    Language,
    Series,
    SeriesNumber,
    Cover,
    PublicationYear,
    Publisher,
}

impl MetadataField {
    pub fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Authors => "authors",
            Self::Description => "description",
            Self::Language => "language",
            Self::Series => "series",
            Self::SeriesNumber => "seriesNumber",
            Self::Cover => "cover",
            Self::PublicationYear => "publicationYear",
            Self::Publisher => "publisher",
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::SeriesNumber => "series_number",
            Self::Cover => "cover_path",
            Self::PublicationYear => "publication_year",
            _ => self.name(),
        }
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MetadataSource {
    pub field: String,
    pub source: String,
    pub source_key: Option<String>,
    pub manual: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum Scope {
    Book(i64),
    Edition(i64),
}

impl Scope {
    fn tables(self) -> (&'static str, &'static str, &'static str, i64) {
        match self {
            Self::Book(id) => ("books", "book_metadata_fields", "book_id", id),
            Self::Edition(id) => ("editions", "edition_metadata_fields", "edition_id", id),
        }
    }
}

pub(crate) async fn read(
    tx: &mut Transaction<'_, Sqlite>,
    scope: Scope,
    field: MetadataField,
) -> Result<Value, AppError> {
    let (table, _, _, id) = scope.tables();
    if field == MetadataField::Authors {
        let authors: Vec<String> = sqlx::query_scalar(
            "SELECT a.name FROM book_authors ba JOIN authors a ON a.id = ba.author_id
             WHERE ba.book_id = ? ORDER BY ba.position, a.name",
        )
        .bind(id)
        .fetch_all(&mut **tx)
        .await?;
        return Ok(json!(authors));
    }
    let query = sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM {table} WHERE id = ?",
        field.column()
    ));
    if field == MetadataField::PublicationYear {
        let value: Option<i64> = sqlx::query_scalar(query)
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
        Ok(json!(value))
    } else {
        let value: Option<String> = sqlx::query_scalar(query)
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
        Ok(json!(value))
    }
}

async fn write(
    tx: &mut Transaction<'_, Sqlite>,
    scope: Scope,
    field: MetadataField,
    value: &Value,
) -> Result<(), AppError> {
    let (table, _, _, id) = scope.tables();
    if field == MetadataField::Authors {
        sqlx::query("DELETE FROM book_authors WHERE book_id = ?")
            .bind(id)
            .execute(&mut **tx)
            .await?;
        let authors: Vec<String> = value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        super::import_metadata::link_authors(tx, id, &authors).await?;
    } else {
        let query = sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET {} = ?, updated_at = unixepoch() WHERE id = ?",
            field.column()
        ));
        if field == MetadataField::PublicationYear {
            sqlx::query(query)
                .bind(value.as_i64())
                .bind(id)
                .execute(&mut **tx)
                .await?;
        } else {
            sqlx::query(query)
                .bind(value.as_str())
                .bind(id)
                .execute(&mut **tx)
                .await?;
            if field == MetadataField::Title && matches!(scope, Scope::Book(_)) {
                sqlx::query("UPDATE books SET normalized_title = ? WHERE id = ?")
                    .bind(bokhylle_core::identity::normalize_text(
                        value.as_str().unwrap_or_default(),
                    ))
                    .bind(id)
                    .execute(&mut **tx)
                    .await?;
            }
        }
    }
    Ok(())
}

fn missing(value: &Value) -> bool {
    value.is_null()
        || value.as_str().is_some_and(|text| text.trim().is_empty())
        || value.as_array().is_some_and(Vec::is_empty)
}

/// Fill a missing field, or make a verified repair. A manual field always wins;
/// its latest automatic candidate remains available for an explicit reset.
pub(crate) async fn automatic(
    tx: &mut Transaction<'_, Sqlite>,
    scope: Scope,
    field: MetadataField,
    mut value: Value,
    source: &str,
    source_key: Option<&str>,
    replace: bool,
) -> Result<bool, AppError> {
    if missing(&value) {
        return Ok(false);
    }
    let (_, provenance, foreign_key, id) = scope.tables();
    let record: Option<bool> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT manual FROM {provenance} WHERE {foreign_key} = ? AND field = ?"
    )))
    .bind(id)
    .bind(field.name())
    .fetch_optional(&mut **tx)
    .await?;
    let current = read(tx, scope, field).await?;
    let manual = record.unwrap_or(false);
    let mut selected_source = source;
    if !manual && !replace && !missing(&current) {
        if field == MetadataField::Authors {
            let mut authors = current.as_array().cloned().unwrap_or_default();
            for author in value.as_array().into_iter().flatten() {
                if !authors.iter().any(|existing| {
                    bokhylle_core::identity::normalize_text(existing.as_str().unwrap_or_default())
                        == bokhylle_core::identity::normalize_text(
                            author.as_str().unwrap_or_default(),
                        )
                }) {
                    authors.push(author.clone());
                }
            }
            value = json!(authors);
            if current != value {
                selected_source = "mixed";
            }
        }
        if current == value && record.is_none() {
            // Newly created rows: their initial value is from this source.
        } else if field != MetadataField::Authors || current == value {
            return Ok(false);
        }
    }
    if !manual {
        write(tx, scope, field, &value).await?;
    }
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {provenance} ({foreign_key}, field, automatic_value, source, source_key)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT({foreign_key}, field) DO UPDATE SET
             automatic_value = excluded.automatic_value, source = excluded.source,
             source_key = excluded.source_key, updated_at = unixepoch()"
    )))
    .bind(id)
    .bind(field.name())
    .bind(value.to_string())
    .bind(selected_source)
    .bind(if selected_source == "mixed" {
        None
    } else {
        source_key
    })
    .execute(&mut **tx)
    .await?;
    Ok(!manual && current != value)
}

pub(crate) async fn mark_manual(
    tx: &mut Transaction<'_, Sqlite>,
    scope: Scope,
    field: MetadataField,
) -> Result<(), AppError> {
    let current = read(tx, scope, field).await?;
    let (_, provenance, foreign_key, id) = scope.tables();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {provenance} ({foreign_key}, field, automatic_value, manual)
         VALUES (?, ?, ?, 1)
         ON CONFLICT({foreign_key}, field) DO UPDATE SET manual = 1, updated_at = unixepoch()"
    )))
    .bind(id)
    .bind(field.name())
    .bind(current.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn reset(
    tx: &mut Transaction<'_, Sqlite>,
    scope: Scope,
    field: MetadataField,
) -> Result<(), AppError> {
    let (_, provenance, foreign_key, id) = scope.tables();
    let saved: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT automatic_value FROM {provenance}
         WHERE {foreign_key} = ? AND field = ? AND manual = 1"
    )))
    .bind(id)
    .bind(field.name())
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(saved) = saved {
        let value =
            serde_json::from_str(&saved).map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
        write(tx, scope, field, &value).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {provenance} SET manual = 0, updated_at = unixepoch()
             WHERE {foreign_key} = ? AND field = ?"
        )))
        .bind(id)
        .bind(field.name())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(crate) async fn sources(
    pool: &SqlitePool,
    scope: Scope,
) -> Result<Vec<MetadataSource>, AppError> {
    let (_, provenance, foreign_key, id) = scope.tables();
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT field, CASE WHEN manual = 1 THEN 'manual' ELSE source END AS source,
                CASE WHEN manual = 1 THEN NULL ELSE source_key END AS source_key, manual
         FROM {provenance} WHERE {foreign_key} = ? ORDER BY field"
    )))
    .bind(id)
    .fetch_all(pool)
    .await?)
}
