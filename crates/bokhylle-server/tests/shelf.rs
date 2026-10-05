use bokhylle_metadata::MetadataResult;

mod common;

async fn add_book(state: &bokhylle_server::AppState, key: &str, title: &str) -> i64 {
    bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: key.to_string(),
            title: title.to_string(),
            authors: vec!["Shelf Author".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

async fn add_file(state: &bokhylle_server::AppState, book_id: i64, digest: &str) {
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
        .bind(book_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, 'epub', 10, ?)",
    )
    .bind(edition_id)
    .bind(format!("/tmp/shelf-{digest}.epub"))
    .bind(digest)
    .execute(&state.db)
    .await
    .unwrap();
}

#[tokio::test]
async fn shelf_adds_removes_filters_and_claims() {
    let test_app = common::test_app().await;
    let user = test_app
        .state
        .auth
        .create_user(
            "shelf_user",
            "password123",
            bokhylle_server::auth::Role::User,
        )
        .await
        .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'shelf_user'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let _ = user;

    let first = add_book(&test_app.state, "/works/OLSHELF1W", "Shelf One").await;
    let second = add_book(&test_app.state, "/works/OLSHELF2W", "Shelf Two").await;
    add_file(&test_app.state, first, "shelf-one-digest").await;
    add_file(&test_app.state, second, "shelf-two-digest").await;

    // Adding is idempotent and shows up in the mine filter.
    bokhylle_server::user_books::add(&test_app.state.db, user_id, first, "manual")
        .await
        .unwrap();
    bokhylle_server::user_books::add(&test_app.state.db, user_id, first, "manual")
        .await
        .unwrap();

    let filters = bokhylle_server::library::queries::BookFilters {
        viewer_id: Some(user_id),
        mine: Some(user_id),
        kind: None,
        format: None,
        language: None,
        series: None,
        subject: None,
        collection: None,
        letter: None,
        missing: None,
    };
    let page = bokhylle_server::library::queries::list_books(
        &test_app.state.db,
        "recent",
        1,
        24,
        &filters,
    )
    .await
    .unwrap();
    assert_eq!(page.total, 1, "only the book on the shelf is listed");

    // Claim-all pulls the rest in.
    let claimed = bokhylle_server::user_books::claim_all(&test_app.state.db, user_id)
        .await
        .unwrap();
    assert!(claimed >= 1);
    assert!(
        bokhylle_server::user_books::contains(&test_app.state.db, user_id, second)
            .await
            .unwrap()
    );

    // Removing takes it back off.
    bokhylle_server::user_books::remove(&test_app.state.db, user_id, second)
        .await
        .unwrap();
    assert!(
        !bokhylle_server::user_books::contains(&test_app.state.db, user_id, second)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn requesting_and_sending_add_to_the_requester_shelf() {
    let test_app = common::test_app().await;
    let book_id = add_book(&test_app.state, "/works/OLSHELF3W", "Requested Shelf").await;
    add_file(&test_app.state, book_id, "shelf-three-digest").await;

    // Register a request directly: the hook must add the book to the shelf.
    let user_id: i64 = {
        test_app
            .state
            .auth
            .create_user(
                "shelf_req",
                "password123",
                bokhylle_server::auth::Role::User,
            )
            .await
            .unwrap();
        sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE username = 'shelf_req'")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap()
    };
    sqlx::query("INSERT INTO acquisitions (id, book_id, status) VALUES ('acq-shelf', ?, 'READY')")
        .bind(book_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    bokhylle_server::acquisition_requests::register(&test_app.state.db, "acq-shelf", user_id, true)
        .await
        .unwrap();
    assert!(
        bokhylle_server::user_books::contains(&test_app.state.db, user_id, book_id)
            .await
            .unwrap()
    );

    // A successful delivery also adds it.
    let target_id: i64 = sqlx::query_scalar(
        "INSERT INTO delivery_targets (user_id, type, name, address, enabled, is_default)
         VALUES (?, 'kindle', 'Shelf Kindle', 'shelf@example.com', 1, 1) RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    bokhylle_server::user_books::remove(&test_app.state.db, user_id, book_id)
        .await
        .unwrap();
    let delivery_id: i64 = sqlx::query_scalar(
        "INSERT INTO deliveries (book_id, file_id, target_id, user_id, address, status)
         VALUES (?, (SELECT id FROM book_files LIMIT 1), ?, ?, 'shelf@example.com', 'PENDING') RETURNING id",
    )
    .bind(book_id)
    .bind(target_id)
    .bind(user_id)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    // Simulate the send-success hook directly (SMTP is not configured here).
    sqlx::query("UPDATE deliveries SET status = 'SENT' WHERE id = ?")
        .bind(delivery_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let (delivered_book, delivered_user): (i64, Option<i64>) =
        sqlx::query_as("SELECT book_id, user_id FROM deliveries WHERE id = ?")
            .bind(delivery_id)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    bokhylle_server::user_books::add(
        &test_app.state.db,
        delivered_user.unwrap(),
        delivered_book,
        "sent",
    )
    .await
    .unwrap();
    assert!(
        bokhylle_server::user_books::contains(&test_app.state.db, user_id, book_id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn discover_marks_household_books_and_shelf_state() {
    use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
    use bokhylle_metadata::testing::FakeMetadataProvider;
    use std::sync::Arc;

    let metadata = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "/works/OLSHELFDW".to_string(),
        title: "Shelf Discover".to_string(),
        authors: vec!["Shelf Author".to_string()],
        ..Default::default()
    }]));
    let library = tempfile::tempdir().unwrap();
    let test_app = common::test_app_full(
        library.path().to_path_buf(),
        metadata,
        Arc::new(FakeIndexerProvider::default()),
        Arc::new(FakeDownloadProvider::default()),
    )
    .await;
    test_app
        .state
        .auth
        .create_user(
            "shelf_viewer",
            "password123",
            bokhylle_server::auth::Role::User,
        )
        .await
        .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'shelf_viewer'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    let book_id = add_book(&test_app.state, "/works/OLSHELFDW", "Shelf Discover").await;

    let results = bokhylle_server::discovery::search(
        &test_app.state,
        bokhylle_server::discovery::SearchKind::Any,
        "shelf discover",
        10,
        user_id,
    )
    .await
    .unwrap();
    let hit = results.first().expect("one result");
    assert_eq!(hit.owned_book_id, Some(book_id));
    assert!(
        !hit.on_shelf,
        "household ownership alone does not add a shelf"
    );

    bokhylle_server::user_books::add(&test_app.state.db, user_id, book_id, "manual")
        .await
        .unwrap();
    let results = bokhylle_server::discovery::search(
        &test_app.state,
        bokhylle_server::discovery::SearchKind::Any,
        "shelf discover",
        10,
        user_id,
    )
    .await
    .unwrap();
    assert!(
        results.first().unwrap().on_shelf,
        "shelf state is per viewer"
    );
}

async fn add_book_with_subjects(
    state: &bokhylle_server::AppState,
    key: &str,
    title: &str,
    subjects: &[&str],
) -> i64 {
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: key.to_string(),
            title: title.to_string(),
            authors: vec!["Shelf Author".to_string()],
            subjects: subjects.iter().map(|subject| subject.to_string()).collect(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    add_file(
        state,
        book_id,
        &format!("digest-{}", key.replace(['/', ':'], "")),
    )
    .await;
    book_id
}

#[tokio::test]
async fn likes_rank_home_and_not_for_me_is_respected() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("liker", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let user_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'liker'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();

    let mut space = Vec::new();
    for index in 0..4 {
        space.push(
            add_book_with_subjects(
                &test_app.state,
                &format!("/works/OLLIKESPACE{index}W"),
                &format!("Space Book {index}"),
                &["Space opera"],
            )
            .await,
        );
    }
    let mut domestic = Vec::new();
    for index in 0..4 {
        domestic.push(
            add_book_with_subjects(
                &test_app.state,
                &format!("/works/OLLIKEDOM{index}W"),
                &format!("Domestic Book {index}"),
                &["Domestic thriller", "Psychological fiction"],
            )
            .await,
        );
    }

    // A like lifts related books above household counts. The liked-books
    // rail takes priority over subject rails that repeat its recommendations.
    bokhylle_server::user_books::set_preference(
        &test_app.state.db,
        user_id,
        domestic[0],
        Some("liked"),
    )
    .await
    .unwrap();

    let rails = bokhylle_server::library::queries::home_rails(&test_app.state.db, user_id)
        .await
        .unwrap();
    assert_eq!(
        rails[0].title, "Based on books you liked",
        "the liked recommendations should lead the rails"
    );
    let liked_rail = &rails[0];
    assert_eq!(liked_rail.books.len(), 3);
    assert!(
        liked_rail
            .books
            .iter()
            .all(|book| domestic.contains(&book.id))
    );
    assert!(!liked_rail.books.iter().any(|book| book.id == domestic[0]));

    // "Not for me" books stay out of the rails.
    bokhylle_server::user_books::set_preference(
        &test_app.state.db,
        user_id,
        space[0],
        Some("not_for_me"),
    )
    .await
    .unwrap();
    let rails = bokhylle_server::library::queries::home_rails(&test_app.state.db, user_id)
        .await
        .unwrap();
    if let Some(rail) = rails
        .iter()
        .find(|rail| rail.subject.as_deref() == Some("space opera"))
    {
        assert!(
            !rail.books.iter().any(|book| book.id == space[0]),
            "a not-for-me book must not be recommended back"
        );
    }

    // Only the two defined preferences are accepted.
    assert!(
        bokhylle_server::user_books::set_preference(
            &test_app.state.db,
            user_id,
            domestic[1],
            Some("bogus"),
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn preferences_do_not_change_shelf_membership() {
    let test_app = common::test_app().await;
    let user_id: i64 = {
        test_app
            .state
            .auth
            .create_user("tastes", "password123", bokhylle_server::auth::Role::User)
            .await
            .unwrap();
        sqlx::query_scalar("SELECT id FROM users WHERE username = 'tastes'")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap()
    };
    let book_id = add_book(&test_app.state, "/works/OLTASTEW", "Taste Book").await;
    add_file(&test_app.state, book_id, "taste-digest").await;

    bokhylle_server::user_books::set_preference(
        &test_app.state.db,
        user_id,
        book_id,
        Some("liked"),
    )
    .await
    .unwrap();
    assert!(
        !bokhylle_server::user_books::contains(&test_app.state.db, user_id, book_id)
            .await
            .unwrap(),
        "liking must not claim shelf membership"
    );
    assert_eq!(
        bokhylle_server::user_books::preference(&test_app.state.db, user_id, book_id)
            .await
            .unwrap()
            .as_deref(),
        Some("liked")
    );

    // The shelf can still be joined explicitly, keeping the preference.
    bokhylle_server::user_books::add(&test_app.state.db, user_id, book_id, "manual")
        .await
        .unwrap();
    assert!(
        bokhylle_server::user_books::contains(&test_app.state.db, user_id, book_id)
            .await
            .unwrap()
    );

    // Taking it off keeps the preference row but drops membership.
    bokhylle_server::user_books::remove(&test_app.state.db, user_id, book_id)
        .await
        .unwrap();
    assert!(
        !bokhylle_server::user_books::contains(&test_app.state.db, user_id, book_id)
            .await
            .unwrap()
    );
    assert_eq!(
        bokhylle_server::user_books::preference(&test_app.state.db, user_id, book_id)
            .await
            .unwrap()
            .as_deref(),
        Some("liked")
    );

    // "Not for me" also stays off the shelf.
    bokhylle_server::user_books::set_preference(
        &test_app.state.db,
        user_id,
        book_id,
        Some("not_for_me"),
    )
    .await
    .unwrap();
    assert!(
        !bokhylle_server::user_books::contains(&test_app.state.db, user_id, book_id)
            .await
            .unwrap()
    );

    // The negative signal is affinity too: it must not claim membership.
    let other = add_book(&test_app.state, "/works/OLTASTE2W", "Taste Book Two").await;
    add_file(&test_app.state, other, "taste-digest-2").await;
    bokhylle_server::user_books::set_preference(
        &test_app.state.db,
        user_id,
        other,
        Some("not_for_me"),
    )
    .await
    .unwrap();
    assert!(
        !bokhylle_server::user_books::contains(&test_app.state.db, user_id, other)
            .await
            .unwrap(),
        "not-for-me must not claim shelf membership"
    );
    assert_eq!(
        bokhylle_server::user_books::preference(&test_app.state.db, user_id, other)
            .await
            .unwrap()
            .as_deref(),
        Some("not_for_me")
    );
}

#[tokio::test]
async fn reshelving_refreshes_the_membership_timestamp_once() {
    let test_app = common::test_app().await;
    let user = test_app
        .state
        .auth
        .create_user("reshelf", "password123", bokhylle_server::auth::Role::User)
        .await
        .unwrap();
    let book = add_book(&test_app.state, "/works/OLRESHELFW", "Reshelf Book").await;

    // A like creates a preference-only row with on_shelf = 0.
    bokhylle_server::user_books::set_preference(&test_app.state.db, user.id, book, Some("liked"))
        .await
        .unwrap();
    // Backdate it as if the like happened weeks ago.
    sqlx::query("UPDATE user_books SET added_at = unixepoch() - 21 * 86400 WHERE user_id = ? AND book_id = ?")
        .bind(user.id)
        .bind(book)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let before: i64 =
        sqlx::query_scalar("SELECT added_at FROM user_books WHERE user_id = ? AND book_id = ?")
            .bind(user.id)
            .bind(book)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();

    bokhylle_server::user_books::add(&test_app.state.db, user.id, book, "requested")
        .await
        .unwrap();
    let (on_shelf, after, source): (i64, i64, String) = sqlx::query_as(
        "SELECT on_shelf, added_at, source FROM user_books WHERE user_id = ? AND book_id = ?",
    )
    .bind(user.id)
    .bind(book)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    assert_eq!(on_shelf, 1);
    assert!(
        after > before,
        "joining the shelf must refresh added_at ({before} -> {after})"
    );
    assert_eq!(source, "requested");

    // Staying on the shelf keeps the membership timestamp and source.
    bokhylle_server::user_books::add(&test_app.state.db, user.id, book, "sent")
        .await
        .unwrap();
    let (again, source_again): (i64, String) =
        sqlx::query_as("SELECT added_at, source FROM user_books WHERE user_id = ? AND book_id = ?")
            .bind(user.id)
            .bind(book)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    assert_eq!(again, after, "a second add must not move the timestamp");
    assert_eq!(source_again, "requested");
}

#[tokio::test]
async fn existing_file_selection_respects_configured_languages() {
    let test_app = common::test_app().await;
    let book = add_book(&test_app.state, "/works/OLSTRICTW", "Strict Language Book").await;
    add_file(&test_app.state, book, "strict-language-digest").await;
    sqlx::query("UPDATE editions SET language = 'sv' WHERE book_id = ?")
        .bind(book)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let file_id: i64 =
        sqlx::query_scalar("SELECT id FROM book_files WHERE sha256 = 'strict-language-digest'")
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();

    let chosen = |languages: Vec<String>| {
        let db = test_app.state.db.clone();
        async move {
            bokhylle_server::library::queries::existing_file_in_languages(
                &db,
                book,
                Some("epub"),
                &languages,
            )
            .await
            .unwrap()
        }
    };

    assert_eq!(
        chosen(vec!["en".to_string()]).await,
        None,
        "a configured language must not fall back to a wrong-language file"
    );
    assert_eq!(
        chosen(vec!["sv".to_string()]).await,
        Some(file_id),
        "the matching language is served"
    );
    assert_eq!(
        chosen(vec![]).await,
        Some(file_id),
        "no preference means any file"
    );
}

#[tokio::test]
async fn existing_file_selection_prefers_format_before_language() {
    let test_app = common::test_app().await;
    let book = add_book(&test_app.state, "/works/OLORDERW", "Format First Book").await;
    let english_edition: i64 =
        sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
            .bind(book)
            .fetch_one(&test_app.state.db)
            .await
            .unwrap();
    sqlx::query("UPDATE editions SET language = 'en' WHERE id = ?")
        .bind(english_edition)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    let pdf_id: i64 = sqlx::query_scalar(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, '/tmp/order-first.pdf', 'pdf', 10, 'order-first-pdf') RETURNING id",
    )
    .bind(english_edition)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    let swedish_edition: i64 = sqlx::query_scalar(
        "INSERT INTO editions (book_id, title, language) VALUES (?, 'Format First Book', 'sv')
         RETURNING id",
    )
    .bind(book)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();
    let epub_id: i64 = sqlx::query_scalar(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, '/tmp/order-second.epub', 'epub', 10, 'order-second-epub') RETURNING id",
    )
    .bind(swedish_edition)
    .fetch_one(&test_app.state.db)
    .await
    .unwrap();

    // The evaluator's rule: format preference first (EPUB before the PDF
    // fallback), then the ordered languages, then a stable tiebreak. So a
    // Swedish EPUB beats an English PDF even though English is also accepted.
    let languages = vec!["sv".to_string(), "en".to_string()];
    let epub = bokhylle_server::library::queries::existing_file_in_languages(
        &test_app.state.db,
        book,
        Some("epub"),
        &languages,
    )
    .await
    .unwrap();
    assert_eq!(epub, Some(epub_id), "EPUB leads over the English PDF");

    let pdf = bokhylle_server::library::queries::existing_file_in_languages(
        &test_app.state.db,
        book,
        Some("pdf"),
        &languages,
    )
    .await
    .unwrap();
    assert_eq!(
        pdf,
        Some(pdf_id),
        "an explicit PDF preference reaches the PDF"
    );
}
