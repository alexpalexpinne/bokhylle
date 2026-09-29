use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_server::auth::Role;

mod common;

async fn request(
    test_app: &common::TestApp,
    method: &str,
    uri: &str,
    cookie: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
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
    let response = test_app
        .router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn get_json(test_app: &common::TestApp, uri: &str, cookie: &str) -> (StatusCode, Value) {
    request(test_app, "GET", uri, cookie, None).await
}

#[tokio::test]
async fn collections_group_books_and_filter_the_library() {
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

    let response = request(&test_app, "POST", "/api/library/scan", &cookie, None).await;
    assert_eq!(response.0, StatusCode::ACCEPTED);
    for _ in 0..200 {
        let (_, status) = get_json(&test_app, "/api/library/scan/status", &cookie).await;
        if status["running"] == false && status["summary"].is_object() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let (_, books) = get_json(&test_app, "/api/books?pageSize=50", &cookie).await;
    let book_id = books["items"][0]["id"].as_i64().unwrap();

    let (status, collection) = request(
        &test_app,
        "POST",
        "/api/collections",
        &cookie,
        Some(json!({ "name": "Favourites" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(collection["bookCount"], 0);
    let collection_id = collection["id"].as_i64().unwrap();

    let (status, _) = request(
        &test_app,
        "POST",
        "/api/collections",
        &cookie,
        Some(json!({ "name": "favourites" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, _) = request(
        &test_app,
        "POST",
        &format!("/api/collections/{collection_id}/books"),
        &cookie,
        Some(json!({ "bookId": book_id })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // adding twice is a no-op
    let (status, _) = request(
        &test_app,
        "POST",
        &format!("/api/collections/{collection_id}/books"),
        &cookie,
        Some(json!({ "bookId": book_id })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, list) = get_json(&test_app, "/api/collections", &cookie).await;
    assert_eq!(list[0]["bookCount"], 1);

    let (_, memberships) = get_json(
        &test_app,
        &format!("/api/books/{book_id}/collections"),
        &cookie,
    )
    .await;
    assert_eq!(memberships.as_array().unwrap().len(), 1);
    assert_eq!(memberships[0]["name"], "Favourites");

    let (_, detail) = get_json(
        &test_app,
        &format!("/api/collections/{collection_id}"),
        &cookie,
    )
    .await;
    assert_eq!(detail["books"][0]["id"], book_id);

    let (_, filtered) = get_json(
        &test_app,
        &format!("/api/books?collection={collection_id}&pageSize=50"),
        &cookie,
    )
    .await;
    assert_eq!(filtered["total"], 1);
    assert_eq!(filtered["items"][0]["id"], book_id);

    let (status, _) = request(
        &test_app,
        "DELETE",
        &format!("/api/collections/{collection_id}/books/{book_id}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = request(
        &test_app,
        "DELETE",
        &format!("/api/collections/{collection_id}/books/{book_id}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = request(
        &test_app,
        "DELETE",
        &format!("/api/collections/{collection_id}"),
        &cookie,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = get_json(
        &test_app,
        &format!("/api/collections/{collection_id}"),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
