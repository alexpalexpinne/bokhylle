use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;
use bokhylle_server::library::{import_metadata, queries};
use serde_json::{Value, json};
use tower::ServiceExt;

mod common;

async fn patch(app: &common::TestApp, id: i64, cookie: &str, value: Value) -> StatusCode {
    app.router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/admin/books/{id}"))
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

fn metadata() -> MetadataResult {
    MetadataResult {
        provider: "fake".into(),
        provider_key: "metadata-book".into(),
        title: "Project Hail Mary".into(),
        authors: vec!["Andy Weir".into()],
        isbn13: Some("9780593135204".into()),
        language: Some("en".into()),
        description: Some("An automatic description.".into()),
        year: Some(2021),
        series: Some("Imported series".into()),
        series_number: Some("1".into()),
        ..Default::default()
    }
}

async fn admin(app: &common::TestApp) -> String {
    app.state
        .auth
        .create_user("admin", "password123", Role::Admin)
        .await
        .unwrap();
    common::login(app, "admin", "password123").await
}

#[tokio::test]
async fn corrections_and_intentional_clears_survive_provider_merge_and_restart() {
    let initial_app = common::test_app().await;
    let app = &initial_app;
    let cookie = admin(app).await;
    let original = metadata();
    let id = import_metadata::upsert_book_from_metadata(&app.state.db, &original)
        .await
        .unwrap();
    let edit = json!({ "title": "My corrected title", "authors": ["Correct Author"],
        "description": null, "language": "sv", "series": "", "seriesNumber": null,
        "publicationYear": null });
    assert_eq!(patch(app, id, &cookie, edit).await, StatusCode::OK);
    let mut refreshed = original.clone();
    refreshed.title = "Provider changed title".into();
    refreshed.description = Some("Provider changed description".into());
    refreshed.authors = vec!["Unwanted Author".into()];
    refreshed.year = Some(2024);
    assert_eq!(
        import_metadata::upsert_book_from_metadata(&app.state.db, &refreshed)
            .await
            .unwrap(),
        id
    );
    let detail = queries::get_book(&app.state.db, id).await.unwrap().unwrap();
    assert_eq!(detail.title, "My corrected title");
    assert_eq!(detail.authors, ["Correct Author"]);
    assert_eq!(detail.description, None);
    assert_eq!(detail.language.as_deref(), Some("sv"));
    assert_eq!(detail.legacy_series_text, None);
    assert_eq!(detail.series_number, None);
    assert_eq!(detail.editions[0].publication_year, None);
    assert!(
        detail
            .metadata_sources
            .iter()
            .filter(|s| s.manual)
            .all(|s| s.source == "manual")
    );

    let config = app.state.paths.config_dir.clone();
    let library = app.state.paths.library_root.clone();
    app.state.db.close().await;
    let app = common::test_app_from_existing_config(config, library).await;
    let persisted = queries::get_book(&app.state.db, id).await.unwrap().unwrap();
    assert_eq!(persisted.title, "My corrected title");
    assert_eq!(persisted.authors, ["Correct Author"]);
    assert_eq!(persisted.description, None);
    let fields: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM book_metadata_fields WHERE book_id = ? AND manual = 1",
    )
    .bind(id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(fields, 6);

    assert_eq!(
        patch(
            &app,
            id,
            &cookie,
            json!({ "useAutomaticMetadata": [
        "title", "authors", "description", "language", "series", "seriesNumber", "publicationYear"
    ] })
        )
        .await,
        StatusCode::OK
    );
    let restored = queries::get_book(&app.state.db, id).await.unwrap().unwrap();
    assert_eq!(restored.title, refreshed.title);
    assert_eq!(restored.authors, refreshed.authors);
    assert_eq!(restored.description, refreshed.description);
    assert_eq!(restored.language, original.language);
    assert_eq!(restored.legacy_series_text, original.series);
    assert_eq!(restored.series_number, original.series_number);
    assert_eq!(restored.editions[0].publication_year, Some(2024));
    assert!(restored.metadata_sources.iter().all(|s| !s.manual));
    assert_eq!(
        restored
            .metadata_sources
            .iter()
            .find(|s| s.field == "description")
            .unwrap()
            .source,
        "fake"
    );
    let title: String = sqlx::query_scalar("SELECT title FROM books_fts WHERE rowid = ?")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(title, refreshed.title);
}

#[tokio::test]
async fn maintenance_keeps_manual_blanks_and_tracks_the_description_provider() {
    let primary = MetadataResult {
        description: None,
        ..metadata()
    };
    let fallback = MetadataResult {
        provider: "googlebooks".into(),
        description: Some("Fallback description".into()),
        ..metadata()
    };
    let app = common::test_app_with_provider_matrix(
        Arc::new(FakeMetadataProvider::new(vec![primary.clone()])),
        Some(Arc::new(
            FakeMetadataProvider::new(vec![fallback]).named("googlebooks"),
        )),
        Arc::new(FakeMetadataProvider::new(vec![])),
    )
    .await;
    let cookie = admin(&app).await;
    let id = import_metadata::upsert_book_from_metadata(&app.state.db, &primary)
        .await
        .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO book_files (edition_id, path, format, size, sha256) VALUES (?, 'fixture.epub', 'epub', 1, 'metadata-fixture')")
        .bind(edition_id).execute(&app.state.db).await.unwrap();
    assert_eq!(
        patch(&app, id, &cookie, json!({"description": ""})).await,
        StatusCode::OK
    );
    bokhylle_server::maintenance::run_metadata(&app.state, true)
        .await
        .unwrap();
    let detail = queries::get_book(&app.state.db, id).await.unwrap().unwrap();
    assert_eq!(detail.description, None);
    assert_eq!(
        patch(
            &app,
            id,
            &cookie,
            json!({"useAutomaticMetadata": ["description"]})
        )
        .await,
        StatusCode::OK
    );
    let detail = queries::get_book(&app.state.db, id).await.unwrap().unwrap();
    assert_eq!(detail.description.as_deref(), Some("Fallback description"));
    assert_eq!(
        detail
            .metadata_sources
            .iter()
            .find(|s| s.field == "description")
            .unwrap()
            .source,
        "googlebooks"
    );
}

#[tokio::test]
async fn importing_another_file_keeps_corrected_authors_language_and_year() {
    let library = tempfile::tempdir().unwrap();
    let app = common::test_app_with_library_root(library.path().to_owned()).await;
    let cookie = admin(&app).await;
    let id = import_metadata::upsert_book_from_metadata(&app.state.db, &metadata())
        .await
        .unwrap();
    assert_eq!(patch(&app, id, &cookie, json!({
        "title": "Manual title", "authors": ["Manual Author"], "language": "sv", "publicationYear": 1999
    })).await, StatusCode::OK);
    bokhylle_library::fixtures::generate_library(library.path(), 1).unwrap();
    std::fs::rename(
        library.path().join("Anonymous/Mystery Book.epub"),
        library
            .path()
            .join("Anonymous/Mystery Book - Filename Author.epub"),
    )
    .unwrap();
    bokhylle_server::library::index_library(&app.state)
        .await
        .unwrap();
    bokhylle_server::library::index_library(&app.state)
        .await
        .unwrap();
    let detail = queries::get_book(&app.state.db, id).await.unwrap().unwrap();
    assert_eq!(detail.title, "Manual title");
    assert_eq!(detail.authors, ["Manual Author"]);
    assert_eq!(detail.language.as_deref(), Some("sv"));
    assert_eq!(detail.editions[0].publication_year, Some(1999));
    assert_eq!(
        detail.editions[0].language.as_deref(),
        Some("en"),
        "the file keeps its actual edition language"
    );
    assert_eq!(detail.files.len(), 1);
    assert!(
        detail.editions[0]
            .metadata_sources
            .iter()
            .any(|s| s.field == "language" && s.source == "fake")
    );
    let filename_book: i64 =
        sqlx::query_scalar("SELECT id FROM books WHERE title = 'Mystery Book'")
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    let detail = queries::get_book(&app.state.db, filename_book)
        .await
        .unwrap()
        .unwrap();
    for field in ["title", "authors"] {
        let source = detail
            .metadata_sources
            .iter()
            .find(|s| s.field == field)
            .unwrap();
        assert_eq!(source.source, "filename");
        assert!(
            source.source_key.is_some(),
            "file origin is identified by its hash"
        );
    }
}

#[tokio::test]
async fn metadata_reset_requires_admin_and_conflicting_changes_are_atomic() {
    let app = common::test_app().await;
    let cookie = admin(&app).await;
    let id = import_metadata::upsert_book_from_metadata(&app.state.db, &metadata())
        .await
        .unwrap();
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let reader = common::login(&app, "reader", "password123").await;
    assert_eq!(
        patch(
            &app,
            id,
            &reader,
            json!({"useAutomaticMetadata": ["title"]})
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        patch(
            &app,
            id,
            &cookie,
            json!({"title": "Conflicting", "useAutomaticMetadata": ["title"]})
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    let detail = queries::get_book(&app.state.db, id).await.unwrap().unwrap();
    assert_eq!(detail.title, metadata().title);
    assert!(detail.metadata_sources.iter().all(|s| !s.manual));
    assert_eq!(
        patch(&app, id, &cookie, json!({"description": "Only this field"})).await,
        StatusCode::OK
    );
    let detail = queries::get_book(&app.state.db, id).await.unwrap().unwrap();
    assert_eq!(
        detail.metadata_sources.iter().filter(|s| s.manual).count(),
        1
    );
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/books/{id}"))
                .header(header::COOKIE, &reader)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(
        body["metadataSources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["field"] == "description" && s["source"] == "manual")
    );
}

#[tokio::test]
async fn migration_keeps_existing_origins_unknown_and_preserves_series_locks() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/0001_initial.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO books (id, title, normalized_title, description, series_number, series_link_locked) VALUES (1, 'Legacy', 'legacy', 'Local text', '9', 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO editions (book_id, title, provider, provider_key) VALUES (1, 'Legacy', 'openlibrary', '/works/LEGACY')")
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/0002_metadata_fields.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let sources: Vec<(String, bool)> =
        sqlx::query_as("SELECT source, manual FROM book_metadata_fields WHERE book_id = 1")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(sources.len(), 7);
    assert!(sources.iter().all(|(source, _)| source == "unknown"));
    assert_eq!(sources.iter().filter(|(_, manual)| *manual).count(), 1);
}
