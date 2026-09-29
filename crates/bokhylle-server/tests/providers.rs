mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use bokhylle_metadata::{MetadataError, MetadataProvider, MetadataQuery, MetadataResult};

#[derive(Default)]
struct CallCounts {
    get_book: AtomicUsize,
    search: AtomicUsize,
    ratings: AtomicUsize,
    covers: AtomicUsize,
}

struct CountingProvider {
    name: &'static str,
    counts: Arc<CallCounts>,
    results: Vec<MetadataResult>,
    ratings: Option<(f64, i64)>,
    cover: Option<Vec<u8>>,
}

impl CountingProvider {
    fn new(name: &'static str, results: Vec<MetadataResult>) -> Self {
        Self {
            name,
            counts: Arc::new(CallCounts::default()),
            results,
            ratings: None,
            cover: None,
        }
    }

    fn with_ratings(mut self, average: f64, count: i64) -> Self {
        self.ratings = Some((average, count));
        self
    }

    fn with_cover(mut self, bytes: Vec<u8>) -> Self {
        self.cover = Some(bytes);
        self
    }

    fn cover_calls(&self) -> usize {
        self.counts.covers.load(Ordering::SeqCst)
    }

    fn get_book_calls(&self) -> usize {
        self.counts.get_book.load(Ordering::SeqCst)
    }

    fn search_calls(&self) -> usize {
        self.counts.search.load(Ordering::SeqCst)
    }

    fn rating_calls(&self) -> usize {
        self.counts.ratings.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl MetadataProvider for CountingProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn search(&self, _query: &MetadataQuery) -> Result<Vec<MetadataResult>, MetadataError> {
        self.counts.search.fetch_add(1, Ordering::SeqCst);
        Ok(self.results.clone())
    }

    async fn get_book(&self, _provider_key: &str) -> Result<Option<MetadataResult>, MetadataError> {
        self.counts.get_book.fetch_add(1, Ordering::SeqCst);
        Ok(self.results.first().cloned())
    }

    async fn fetch_ratings(
        &self,
        _provider_key: &str,
    ) -> Result<Option<(f64, i64)>, MetadataError> {
        self.counts.ratings.fetch_add(1, Ordering::SeqCst);
        Ok(self.ratings)
    }

    async fn fetch_cover(&self, _cover_id: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        self.counts.covers.fetch_add(1, Ordering::SeqCst);
        Ok(self.cover.clone())
    }
}

fn metadata(provider: &str, key: &str, title: &str, description: Option<&str>) -> MetadataResult {
    MetadataResult {
        provider: provider.to_string(),
        provider_key: key.to_string(),
        title: title.to_string(),
        authors: vec!["Test Author".to_string()],
        description: description.map(str::to_string),
        ..Default::default()
    }
}

async fn seed_book(app: &common::TestApp, isbn: &str) -> i64 {
    let book = metadata("openlibrary", "/works/OL1W", "Counting Book", None);
    let book_id =
        bokhylle_server::library::import_metadata::upsert_book_from_metadata(&app.state.db, &book)
            .await
            .unwrap();
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ?")
        .bind(book_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE editions SET isbn13 = ? WHERE id = ?")
        .bind(format!("9780000000{isbn}"))
        .bind(edition_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, '/tmp/counting.epub', 'epub', 10, ?)",
    )
    .bind(edition_id)
    .bind(format!("counting-{isbn}"))
    .execute(&app.state.db)
    .await
    .unwrap();
    book_id
}

#[tokio::test]
async fn google_metadata_with_openlibrary_ratings_never_uses_openlibrary_for_books() {
    let google = Arc::new(CountingProvider::new(
        "google_books",
        vec![metadata(
            "google_books",
            "volume-1",
            "Counting Book",
            Some("A description from Google."),
        )],
    ));
    let openlibrary = Arc::new(
        CountingProvider::new(
            "openlibrary",
            vec![metadata(
                "openlibrary",
                "/works/OL1W",
                "Counting Book",
                None,
            )],
        )
        .with_ratings(4.1, 42),
    );

    let app =
        common::test_app_with_provider_matrix(google.clone(), None, openlibrary.clone()).await;
    let book_id = seed_book(&app, "one").await;

    bokhylle_server::maintenance::run_metadata(&app.state, false)
        .await
        .unwrap();

    assert!(google.search_calls() > 0, "google must identify the book");
    assert_eq!(
        google.get_book_calls(),
        0,
        "an Open Library key must never be sent to Google"
    );
    assert_eq!(
        openlibrary.get_book_calls(),
        0,
        "openlibrary must not be asked for book metadata"
    );
    assert!(
        openlibrary.rating_calls() > 0,
        "openlibrary is the configured ratings source"
    );

    let (rating, source): (Option<f64>, Option<String>) =
        sqlx::query_as("SELECT rating, rating_source FROM books WHERE id = ?")
            .bind(book_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(rating, Some(4.1));
    assert_eq!(source.as_deref(), Some("openlibrary"));
}

#[tokio::test]
async fn google_only_with_disabled_ratings_makes_no_openlibrary_calls() {
    let google = Arc::new(CountingProvider::new(
        "google_books",
        vec![metadata(
            "google_books",
            "volume-1",
            "Counting Book",
            Some("A description from Google."),
        )],
    ));
    let openlibrary = Arc::new(CountingProvider::new(
        "openlibrary",
        vec![metadata(
            "openlibrary",
            "/works/OL1W",
            "Counting Book",
            None,
        )],
    ));

    let app = test_app_disabled_ratings(google.clone(), openlibrary.clone()).await;
    seed_book(&app, "two").await;

    bokhylle_server::maintenance::run_metadata(&app.state, false)
        .await
        .unwrap();

    assert!(google.search_calls() > 0);
    assert_eq!(google.get_book_calls(), 0);
    assert_eq!(openlibrary.get_book_calls(), 0);
    assert_eq!(openlibrary.search_calls(), 0);
    assert_eq!(openlibrary.rating_calls(), 0);
}

async fn test_app_disabled_ratings(
    metadata: Arc<dyn MetadataProvider>,
    _openlibrary: Arc<CountingProvider>,
) -> common::TestApp {
    common::test_app_with_provider_matrix(
        metadata,
        None,
        Arc::new(bokhylle_metadata::disabled::DisabledProvider),
    )
    .await
}

#[tokio::test]
async fn cross_provider_ratings_need_a_confident_match() {
    let google = Arc::new(CountingProvider::new(
        "google_books",
        vec![metadata(
            "google_books",
            "volume-1",
            "Counting Book",
            Some("A description from Google."),
        )],
    ));
    // The ratings provider answers the ISBN search with a different work.
    let openlibrary = Arc::new(
        CountingProvider::new(
            "openlibrary",
            vec![metadata(
                "openlibrary",
                "/works/OLOTHERW",
                "Completely Different Book",
                None,
            )],
        )
        .with_ratings(4.9, 500),
    );

    let app =
        common::test_app_with_provider_matrix(google.clone(), None, openlibrary.clone()).await;
    let book_id = seed_book(&app, "five").await;

    bokhylle_server::maintenance::run_metadata(&app.state, false)
        .await
        .unwrap();

    assert!(openlibrary.search_calls() > 0, "the ISBN is searched");
    let rating: Option<f64> = sqlx::query_scalar("SELECT rating FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        rating, None,
        "an unconvincing ratings candidate must not be stored"
    );
}

#[tokio::test]
async fn fallback_supplies_a_cover_only_when_none_exists() {
    let openlibrary = Arc::new(CountingProvider::new(
        "openlibrary",
        vec![metadata(
            "openlibrary",
            "/works/OL1W",
            "Counting Book",
            Some("Already described."),
        )],
    ));
    let mut fallback_metadata = metadata(
        "google_books",
        "volume-1",
        "Counting Book",
        Some("Google description."),
    );
    fallback_metadata.cover_id = Some("https://example.test/cover.jpg".to_string());
    let google = Arc::new(
        CountingProvider::new("google_books", vec![fallback_metadata]).with_cover(vec![0u8; 4096]),
    );

    let app = common::test_app_with_provider_matrix(
        openlibrary.clone(),
        Some(google.clone()),
        openlibrary.clone(),
    )
    .await;
    let book_id = seed_book(&app, "four").await;

    bokhylle_server::maintenance::run_metadata(&app.state, false)
        .await
        .unwrap();

    assert!(google.cover_calls() > 0, "fallback cover must be fetched");
    let cover_path: Option<String> =
        sqlx::query_scalar("SELECT cover_path FROM books WHERE id = ?")
            .bind(book_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    let cover_path = cover_path.expect("cover must be stored");
    assert!(std::path::Path::new(&cover_path).exists());
}

#[tokio::test]
async fn automatic_fills_a_missing_description_from_the_fallback() {
    let openlibrary = Arc::new(CountingProvider::new(
        "openlibrary",
        vec![metadata(
            "openlibrary",
            "/works/OL1W",
            "Counting Book",
            None,
        )],
    ));
    let google = Arc::new(CountingProvider::new(
        "google_books",
        vec![metadata(
            "google_books",
            "volume-1",
            "Counting Book",
            Some("Filled in by the fallback provider."),
        )],
    ));

    let app = common::test_app_with_provider_matrix(
        openlibrary.clone(),
        Some(google.clone()),
        openlibrary.clone(),
    )
    .await;
    let book_id = seed_book(&app, "three").await;

    bokhylle_server::maintenance::run_metadata(&app.state, false)
        .await
        .unwrap();

    assert!(google.search_calls() > 0, "fallback must be consulted");
    let description: Option<String> =
        sqlx::query_scalar("SELECT description FROM books WHERE id = ?")
            .bind(book_id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        description.as_deref(),
        Some("Filled in by the fallback provider.")
    );
}
