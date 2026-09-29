use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use std::sync::Arc;

use bokhylle_acquisition::testing::{FakeDownloadProvider, FakeIndexerProvider};
use bokhylle_metadata::AuthorProfile;
use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;

mod common;

async fn get(test_app: &common::TestApp, uri: &str, cookie: &str) -> axum::response::Response {
    test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn get_json(test_app: &common::TestApp, uri: &str, cookie: &str) -> Value {
    let response = get(test_app, uri, cookie).await;
    assert_eq!(response.status(), StatusCode::OK, "uri: {uri}");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn send(
    test_app: &common::TestApp,
    method: &str,
    uri: &str,
    cookie: &str,
    payload: Option<Value>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if !cookie.is_empty() {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = match payload {
        Some(payload) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&payload).unwrap())
        }
        None => Body::empty(),
    };
    test_app
        .router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap()
}

async fn wait_for_scan(test_app: &common::TestApp, cookie: &str) -> Value {
    for _ in 0..200 {
        let status = get_json(test_app, "/api/library/scan/status", cookie).await;
        if status["running"] == false
            && (status["summary"].is_object() || status["error"].is_string())
        {
            return status;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("scan did not finish in time");
}

async fn trigger_scan(test_app: &common::TestApp, cookie: &str) -> StatusCode {
    test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/library/scan")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn author_photos_resolve_and_cache() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 2).unwrap();

    let metadata = Arc::new(FakeMetadataProvider::new(vec![]));
    metadata.set_author("Andy Weir", "OL123A", vec![0xFFu8; 4096]);

    let test_app = common::test_app_full(
        library_dir.path().to_path_buf(),
        metadata,
        Arc::new(FakeIndexerProvider::default()),
        Arc::new(FakeDownloadProvider::default()),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;
    assert_eq!(trigger_scan(&test_app, &cookie).await, StatusCode::ACCEPTED);
    wait_for_scan(&test_app, &cookie).await;

    let authors = get_json(&test_app, "/api/authors?scope=household", &cookie).await;
    let andy = authors
        .as_array()
        .unwrap()
        .iter()
        .find(|author| author["name"] == "Andy Weir")
        .expect("Andy Weir indexed");
    let andy_id = andy["id"].as_i64().unwrap();

    let response = get(&test_app, &format!("/api/authors/{andy_id}/photo"), &cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(body.len() >= 4096);

    // the resolved OLID is persisted for later requests
    let olid: Option<String> = sqlx::query_scalar("SELECT olid FROM authors WHERE id = ?")
        .bind(andy_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(olid.as_deref(), Some("OL123A"));

    // an author the provider cannot resolve has no photo
    let other = authors
        .as_array()
        .unwrap()
        .iter()
        .find(|author| author["name"] != "Andy Weir")
        .expect("another author indexed");
    let other_id = other["id"].as_i64().unwrap();
    let response = get(
        &test_app,
        &format!("/api/authors/{other_id}/photo"),
        &cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn transient_author_photos_use_the_provider_key_without_creating_an_author() {
    let metadata = Arc::new(FakeMetadataProvider::new(vec![]).named("openlibrary"));
    metadata.set_author("Example Author", "OL123A", vec![0xFFu8; 4096]);
    let app = common::test_app_with_metadata(metadata).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;

    let response = get(
        &app,
        "/api/discover/authors/photo?providerKey=OL123A",
        &cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );
    let response = get(
        &app,
        "/api/discover/authors/photo?providerKey=../../bad",
        &cookie,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM authors")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn transient_author_photo_misses_are_cached_but_errors_are_not() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![]).named("openlibrary"));
    let app = common::test_app_with_metadata(provider.clone()).await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;

    for _ in 0..2 {
        let response = get(
            &app,
            "/api/discover/authors/photo?providerKey=OL456A",
            &cookie,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(provider.calls(), 1);

    provider.set_failing(true);
    for _ in 0..2 {
        let response = get(
            &app,
            "/api/discover/authors/photo?providerKey=OL789A",
            &cookie,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(provider.calls(), 3, "provider errors must remain retryable");
}

#[tokio::test]
async fn author_profiles_use_stable_ids_cache_results_and_block_children() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![]).named("openlibrary"));
    provider.set_author_profile(
        "OL123A",
        AuthorProfile {
            bio: Some("A short biography.".to_string()),
            birth_date: Some("1900".to_string()),
            death_date: None,
        },
    );
    provider.set_author("Local Author", "OL456A", vec![]);
    provider.set_author_profile(
        "OL456A",
        AuthorProfile {
            bio: Some("A local author's biography.".to_string()),
            birth_date: None,
            death_date: None,
        },
    );
    let app = common::test_app_with_metadata(provider.clone()).await;
    app.state
        .auth
        .create_user("adult", "password123", Role::User)
        .await
        .unwrap();
    let child = app
        .state
        .auth
        .create_user("child", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(child.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let adult_cookie = common::login(&app, "adult", "password123").await;
    let child_cookie = common::login(&app, "child", "password123").await;
    let author_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('Example Author', 'example author') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    bokhylle_server::external_ids::link_author(&app.state.db, author_id, "openlibrary", "OL123A")
        .await
        .unwrap();
    let url = format!("/api/authors/{author_id}/profile");

    let first = get_json(&app, &url, &adult_cookie).await;
    assert_eq!(first["bio"], "A short biography.");
    assert_eq!(first["birthDate"], "1900");
    assert_eq!(first["sourceUrl"], "https://openlibrary.org/authors/OL123A");
    assert_eq!(get_json(&app, &url, &adult_cookie).await, first);
    assert_eq!(
        provider.author_profile_calls(),
        1,
        "fresh profile uses the shared cache"
    );
    let missing_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('Unlisted Author', 'unlisted author') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    bokhylle_server::external_ids::link_author(&app.state.db, missing_id, "openlibrary", "OL789A")
        .await
        .unwrap();
    let missing_url = format!("/api/authors/{missing_id}/profile");
    assert!(get_json(&app, &missing_url, &adult_cookie).await.is_null());
    assert!(get_json(&app, &missing_url, &adult_cookie).await.is_null());
    assert_eq!(provider.author_profile_calls(), 2, "misses are cached");

    let local_id: i64 = sqlx::query_scalar(
        "INSERT INTO authors (name, normalized_name) VALUES ('Local Author', 'local author') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    let local_profile = get_json(
        &app,
        &format!("/api/authors/{local_id}/profile"),
        &adult_cookie,
    )
    .await;
    assert_eq!(local_profile["bio"], "A local author's biography.");
    assert_eq!(
        bokhylle_server::external_ids::author_olid(&app.state.db, local_id)
            .await
            .unwrap()
            .as_deref(),
        Some("OL456A"),
        "an exact match is linked for subsequent visits"
    );
    assert_eq!(provider.author_profile_calls(), 3);
    assert_eq!(
        get(&app, &url, &child_cookie).await.status(),
        StatusCode::FORBIDDEN
    );

    let book_id: i64 = sqlx::query_scalar(
        "INSERT INTO books (title, normalized_title) VALUES ('Example Book', 'example book') RETURNING id",
    )
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO book_authors (book_id, author_id) VALUES (?, ?)")
        .bind(book_id)
        .bind(author_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let edition_id: i64 = sqlx::query_scalar(
        "INSERT INTO editions (book_id, title) VALUES (?, 'Example Book') RETURNING id",
    )
    .bind(book_id)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, '/library/example.epub', 'epub', 1, 'example-hash')",
    )
    .bind(edition_id)
    .execute(&app.state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO user_books (user_id, book_id) VALUES (?, ?)")
        .bind(child.id)
        .bind(book_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        get(&app, &url, &child_cookie).await.status(),
        StatusCode::FORBIDDEN,
        "author routes remain closed to children even when a book is on their shelf"
    );

    sqlx::query(
        "UPDATE metadata_cache SET expires_at = 0 WHERE key = 'author-profile:openlibrary:OL123A'",
    )
    .execute(&app.state.db)
    .await
    .unwrap();
    provider.set_failing(true);
    assert_eq!(
        get_json(&app, &url, &adult_cookie).await,
        first,
        "stale biography survives an outage"
    );
    assert_eq!(provider.author_profile_calls(), 4);
}

#[tokio::test]
async fn admins_can_edit_and_delete_books() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 3).unwrap();

    let test_app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;
    assert_eq!(trigger_scan(&test_app, &cookie).await, StatusCode::ACCEPTED);
    wait_for_scan(&test_app, &cookie).await;

    let books = get_json(&test_app, "/api/books?pageSize=50", &cookie).await;
    let book = books["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["title"] == "Project Hail Mary")
        .unwrap();
    let book_id = book["id"].as_i64().unwrap();

    // metadata edit refreshes the row and the search index
    let response = send(
        &test_app,
        "PUT",
        &format!("/api/admin/books/{book_id}"),
        &cookie,
        Some(json!({
            "title": "Project Hail Mary (Edited)",
            "authors": ["Andy Weir", "Editor Person"],
            "description": "A rescue mission.",
            "language": "en",
            "publicationYear": 2022
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let detail = get_json(&test_app, &format!("/api/books/{book_id}"), &cookie).await;
    assert_eq!(detail["title"], "Project Hail Mary (Edited)");
    assert_eq!(detail["authors"].as_array().unwrap().len(), 2);
    assert_eq!(detail["description"], "A rescue mission.");
    assert_eq!(detail["editions"][0]["publicationYear"], 2022);
    // Author refs carry the durable ids the UI links to, in author order.
    let refs = detail["authorRefs"].as_array().unwrap();
    assert_eq!(refs.len(), 2, "{detail}");
    assert_eq!(refs[0]["name"], "Andy Weir");
    assert_eq!(refs[1]["name"], "Editor Person");
    let author_id: i64 = sqlx::query_scalar("SELECT id FROM authors WHERE name = 'Andy Weir'")
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    assert_eq!(refs[0]["id"], author_id);
    let author_page = get_json(&test_app, &format!("/api/authors/{author_id}"), &cookie).await;
    assert!(
        author_page["books"]
            .as_array()
            .unwrap()
            .iter()
            .any(|book| book["id"] == book_id),
        "the linked author page lists the book: {author_page}"
    );

    let search = get_json(&test_app, "/api/books/search?q=edited", &cookie).await;
    assert!(
        search
            .as_array()
            .unwrap()
            .iter()
            .any(|book| book["id"] == book_id),
        "edited title should be searchable: {search}"
    );

    // a non-admin cannot touch the book
    test_app
        .state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    let bob_cookie = common::login(&test_app, "bob", "password123").await;
    let response = send(
        &test_app,
        "DELETE",
        &format!("/api/admin/books/{book_id}"),
        &bob_cookie,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // deleting one file removes it from disk but keeps the book
    let file_id = detail["files"][0]["id"].as_i64().unwrap();
    let file_path: String = sqlx::query_scalar("SELECT f.path FROM book_files f WHERE f.id = ?")
        .bind(file_id)
        .fetch_one(&test_app.state.db)
        .await
        .unwrap();
    let response = send(
        &test_app,
        "DELETE",
        &format!("/api/admin/books/{book_id}/files/{file_id}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!std::path::Path::new(&file_path).exists());

    let detail = get_json(&test_app, &format!("/api/books/{book_id}"), &cookie).await;
    assert!(detail["files"].as_array().unwrap().is_empty());

    // deleting the book removes the row and is visible as 404
    let response = send(
        &test_app,
        "DELETE",
        &format!("/api/admin/books/{book_id}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = get(&test_app, &format!("/api/books/{book_id}"), &cookie).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn covers_fall_back_to_the_metadata_provider() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 2).unwrap();

    let metadata = Arc::new(FakeMetadataProvider::new(vec![]));
    metadata.set_cover(vec![0xFFu8; 4096]);

    let test_app = common::test_app_full(
        library_dir.path().to_path_buf(),
        metadata,
        Arc::new(FakeIndexerProvider::default()),
        Arc::new(FakeDownloadProvider::default()),
    )
    .await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;
    assert_eq!(trigger_scan(&test_app, &cookie).await, StatusCode::ACCEPTED);
    wait_for_scan(&test_app, &cookie).await;

    let books = get_json(&test_app, "/api/books", &cookie).await;
    let hail_mary = books["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|book| book["title"] == "Project Hail Mary")
        .expect("Project Hail Mary indexed");
    let book_id = hail_mary["id"].as_i64().unwrap();

    // fixture covers are 1x1 stubs, so the provider fallback is used
    let response = get(&test_app, &format!("/api/books/{book_id}/cover"), &cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(body.len() >= 4096);
}

#[tokio::test]
async fn facets_and_filters_reflect_the_library() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 12).unwrap();

    let test_app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;
    assert_eq!(trigger_scan(&test_app, &cookie).await, StatusCode::ACCEPTED);
    wait_for_scan(&test_app, &cookie).await;

    let all = get_json(&test_app, "/api/books?pageSize=100", &cookie).await;
    let total = all["total"].as_u64().unwrap();

    let facets = get_json(&test_app, "/api/books/facets?scope=household", &cookie).await;
    let formats = facets["formats"].as_array().unwrap();
    let pdf = formats
        .iter()
        .find(|facet| facet["value"] == "pdf")
        .expect("pdf format facet");
    let epub = formats
        .iter()
        .find(|facet| facet["value"] == "epub")
        .expect("epub format facet");
    assert!(pdf["count"].as_u64().unwrap() > 0);
    assert!(epub["count"].as_u64().unwrap() > 0);
    assert_eq!(
        pdf["count"].as_u64().unwrap() + epub["count"].as_u64().unwrap(),
        total,
    );

    let filtered = get_json(&test_app, "/api/books?format=pdf&pageSize=100", &cookie).await;
    assert_eq!(
        filtered["total"].as_u64().unwrap(),
        pdf["count"].as_u64().unwrap(),
    );

    let language = facets["languages"]
        .as_array()
        .unwrap()
        .first()
        .expect("language facet");
    let language_value = language["value"].as_str().unwrap();
    let by_language = get_json(
        &test_app,
        &format!("/api/books?language={language_value}&pageSize=100"),
        &cookie,
    )
    .await;
    assert_eq!(
        by_language["total"].as_u64().unwrap(),
        language["count"].as_u64().unwrap(),
    );

    let series = facets["series"]
        .as_array()
        .unwrap()
        .first()
        .expect("series facet");
    let series_value = series["value"].as_str().unwrap();
    let by_series = get_json(
        &test_app,
        &format!(
            "/api/books?series={}&pageSize=100",
            series_value.replace(' ', "%20")
        ),
        &cookie,
    )
    .await;
    assert_eq!(
        by_series["total"].as_u64().unwrap(),
        series["count"].as_u64().unwrap(),
    );
}

#[tokio::test]
async fn scans_library_and_serves_library_api() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 10).unwrap();

    let test_app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    assert_eq!(trigger_scan(&test_app, &cookie).await, StatusCode::ACCEPTED);

    let status = wait_for_scan(&test_app, &cookie).await;
    let summary = &status["summary"];
    assert!(summary["filesFound"].as_u64().unwrap() >= 11);
    assert!(summary["indexed"].as_u64().unwrap() >= 11);
    assert!(summary["duplicates"].as_u64().unwrap() >= 1);
    assert!(summary["errors"].as_u64().unwrap() >= 1);

    let books = get_json(&test_app, "/api/books", &cookie).await;
    let total = books["total"].as_u64().unwrap();
    assert!(total >= 11);

    let items = books["items"].as_array().unwrap();
    let hail_mary = items
        .iter()
        .find(|book| book["title"] == "Project Hail Mary")
        .expect("Project Hail Mary should be indexed");
    assert_eq!(hail_mary["authors"][0], "Andy Weir");
    assert_eq!(hail_mary["language"], "en");
    // fixtures embed a 1x1 stub cover, which is treated as missing
    assert_eq!(hail_mary["hasCover"], false);

    let search = get_json(&test_app, "/api/books/search?q=project+hail", &cookie).await;
    assert!(
        search
            .as_array()
            .unwrap()
            .iter()
            .any(|book| book["title"] == "Project Hail Mary")
    );

    let book_id = hail_mary["id"].as_i64().unwrap();
    let detail = get_json(&test_app, &format!("/api/books/{book_id}"), &cookie).await;
    assert_eq!(detail["publicationYear"], 2021);
    assert_eq!(detail["editions"][0]["isbn13"], "9780593135204");
    assert_eq!(detail["editions"][0]["unknown"], false);
    assert_eq!(detail["files"][0]["format"], "epub");

    let cover = get(&test_app, &format!("/api/books/{book_id}/cover"), &cookie).await;
    assert_eq!(cover.status(), StatusCode::OK);
    assert_eq!(
        cover.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/svg+xml"
    );

    let file_id = detail["files"][0]["id"].as_i64().unwrap();
    let download = get(
        &test_app,
        &format!("/api/books/{book_id}/files/{file_id}/download"),
        &cookie,
    )
    .await;
    assert_eq!(download.status(), StatusCode::OK);
    let disposition = download
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(disposition.contains("Project Hail Mary - Andy Weir.epub"));

    let authors = get_json(&test_app, "/api/authors?scope=household", &cookie).await;
    let andy_weir = authors
        .as_array()
        .unwrap()
        .iter()
        .find(|author| author["name"] == "Andy Weir")
        .unwrap();
    assert_eq!(andy_weir["bookCount"], 1);

    let author_id = andy_weir["id"].as_i64().unwrap();
    let author = get_json(
        &test_app,
        &format!("/api/authors/{author_id}?scope=household"),
        &cookie,
    )
    .await;
    assert_eq!(author["books"][0]["title"], "Project Hail Mary");

    let recent = get_json(
        &test_app,
        "/api/books/recent?limit=5&scope=household",
        &cookie,
    )
    .await;
    assert_eq!(recent.as_array().unwrap().len(), 5);

    let highlights = get_json(
        &test_app,
        "/api/books/highlights?limit=4&scope=household",
        &cookie,
    )
    .await;
    assert_eq!(highlights.as_array().unwrap().len(), 4);
}

#[tokio::test]
async fn first_real_file_replaces_an_old_work_placeholder_language() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 1).unwrap();
    let test_app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    let book_id = bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &test_app.state.db,
        &MetadataResult {
            provider: "openlibrary".to_string(),
            provider_key: "/works/OLHAILMARYW".to_string(),
            title: "Project Hail Mary".to_string(),
            authors: vec!["Andy Weir".to_string()],
            language: Some("ru".to_string()),
            languages: vec!["ru".to_string(), "en".to_string()],
            year: Some(1999),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // This is how the previous adapter stored its arbitrary first edition.
    sqlx::query("UPDATE books SET language = 'ru' WHERE id = ?")
        .bind(book_id)
        .execute(&test_app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE editions SET language = 'ru', publication_year = 1999,
         isbn13 = '9780593135204', is_unknown = 0 WHERE book_id = ?",
    )
    .bind(book_id)
    .execute(&test_app.state.db)
    .await
    .unwrap();

    test_app
        .state
        .auth
        .create_user("scanner", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "scanner", "password123").await;
    assert_eq!(trigger_scan(&test_app, &cookie).await, StatusCode::ACCEPTED);
    wait_for_scan(&test_app, &cookie).await;

    let detail = get_json(&test_app, &format!("/api/books/{book_id}"), &cookie).await;
    assert_eq!(detail["language"], "en");
    assert_eq!(detail["publicationYear"], 2021);
    assert_eq!(detail["editions"][0]["language"], "en");
    assert_eq!(detail["editions"][0]["publicationYear"], 2021);
    assert_eq!(detail["files"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn rescan_is_idempotent() {
    let library_dir = tempfile::tempdir().unwrap();
    bokhylle_library::fixtures::generate_library(library_dir.path(), 5).unwrap();

    let test_app = common::test_app_with_library_root(library_dir.path().to_path_buf()).await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::Admin)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    trigger_scan(&test_app, &cookie).await;
    wait_for_scan(&test_app, &cookie).await;
    let first_total = get_json(&test_app, "/api/books", &cookie).await["total"]
        .as_u64()
        .unwrap();

    trigger_scan(&test_app, &cookie).await;
    let status = wait_for_scan(&test_app, &cookie).await;

    assert_eq!(status["summary"]["indexed"].as_u64().unwrap(), 0);
    assert!(status["summary"]["skipped"].as_u64().unwrap() >= 5);

    let second_total = get_json(&test_app, "/api/books", &cookie).await["total"]
        .as_u64()
        .unwrap();
    assert_eq!(first_total, second_total);
}

#[tokio::test]
async fn library_requires_authentication_and_handles_missing_books() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/books")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = get(&test_app, "/api/books/424242", &cookie).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "not_found");
}

#[tokio::test]
async fn scan_requires_admin_role() {
    let test_app = common::test_app().await;
    test_app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&test_app, "reader", "password123").await;

    assert_eq!(
        trigger_scan(&test_app, &cookie).await,
        StatusCode::FORBIDDEN
    );
}
