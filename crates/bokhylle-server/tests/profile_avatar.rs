use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use bokhylle_server::auth::Role;
use tower::ServiceExt;

mod common;

fn picture() -> Vec<u8> {
    base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lQAAAABJRU5ErkJggg==",
    )
    .unwrap()
}

async fn request(
    app: &common::TestApp,
    method: &str,
    cookie: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri("/api/profile/avatar");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let response = app
        .router
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

async fn me_avatar_version(app: &common::TestApp, cookie: &str) -> serde_json::Value {
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/auth/me")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice::<serde_json::Value>(&body).unwrap()["user"]["avatarVersion"].clone()
}

async fn public_picture(app: &common::TestApp, path: &str) -> axum::response::Response {
    app.router
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn encoded_picture(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        width,
        height,
        image::Rgb([163, 70, 31]),
    ))
    .write_to(&mut bytes, format)
    .unwrap();
    bytes.into_inner()
}

#[tokio::test]
async fn sign_in_pictures_are_bounded_thumbnails_for_enabled_profiles_only() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("mira", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "mira", "password123").await;
    let path = format!("/api/auth/users/{}/avatar?v=1", user.id);
    assert_eq!(
        public_picture(&app, &path).await.status(),
        StatusCode::NOT_FOUND
    );
    for (format, mime) in [
        (image::ImageFormat::Png, "image/png"),
        (image::ImageFormat::Jpeg, "image/jpeg"),
        (image::ImageFormat::WebP, "image/webp"),
    ] {
        let original = encoded_picture(320, 240, format);
        assert_eq!(
            request(&app, "PUT", Some(&cookie), Some(mime), original.clone())
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        let response = public_picture(&app, &path).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        let bytes = to_bytes(response.into_body(), 160 * 160 * 4 + 4096)
            .await
            .unwrap();
        let thumbnail = image::load_from_memory(&bytes).unwrap();
        assert_eq!((thumbnail.width(), thumbnail.height()), (160, 120));
        assert_ne!(bytes.as_ref(), original.as_slice());
        // The private API still serves the account's original; it remains authenticated.
        assert_eq!(
            request(&app, "GET", Some(&cookie), None, vec![]).await.2,
            original
        );
        assert_eq!(
            request(&app, "GET", None, None, vec![]).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let response = public_picture(&app, "/api/auth/users").await;
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    let users: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        users["users"][0]["avatarUrl"],
        format!("/api/auth/users/{}/avatar?v=3", user.id)
    );
    assert_eq!(users["users"][0].as_object().unwrap().len(), 7);
    assert!(users["users"][0]["avatarPreset"].is_null());

    sqlx::query("UPDATE users SET disabled = 1 WHERE id = ?")
        .bind(user.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        public_picture(&app, &path).await.status(),
        StatusCode::NOT_FOUND
    );
    let response = public_picture(&app, "/api/auth/users").await;
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["users"],
        serde_json::json!([])
    );
    sqlx::query("UPDATE users SET disabled = 0, profile_type = 'child' WHERE id = ?")
        .bind(user.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    assert_eq!(public_picture(&app, &path).await.status(), StatusCode::OK);
    assert_eq!(
        request(&app, "DELETE", Some(&cookie), None, vec![]).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        public_picture(&app, &path).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        public_picture(&app, "/api/auth/users/99999/avatar")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn sign_in_picture_rejects_corrupt_and_excessive_dimensions() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("mira", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "mira", "password123").await;
    let path = format!("/api/auth/users/{}/avatar", user.id);
    for bytes in [picture(), encoded_picture(4097, 1, image::ImageFormat::Png)] {
        assert_eq!(
            request(&app, "PUT", Some(&cookie), Some("image/png"), bytes)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            public_picture(&app, &path).await.status(),
            StatusCode::NOT_FOUND
        );
    }
}

#[tokio::test]
async fn sign_in_picture_applies_orientation_and_strips_metadata() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("mira", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "mira", "password123").await;
    let mut picture = encoded_picture(320, 120, image::ImageFormat::Jpeg);
    // EXIF orientation 6 (90 degrees clockwise), with an unrelated private comment.
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let comment = b"private camera information";
    let mut metadata = vec![0xff, 0xe1];
    metadata.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    metadata.extend_from_slice(exif);
    metadata.extend_from_slice(&[0xff, 0xfe]);
    metadata.extend_from_slice(&((comment.len() + 2) as u16).to_be_bytes());
    metadata.extend_from_slice(comment);
    picture.splice(2..2, metadata);
    assert_eq!(
        request(&app, "PUT", Some(&cookie), Some("image/jpeg"), picture)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let response = public_picture(&app, &format!("/api/auth/users/{}/avatar", user.id)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let thumbnail = image::load_from_memory(&bytes).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (60, 160));
    assert!(!bytes.windows(comment.len()).any(|window| window == comment));
    assert!(!bytes.windows(4).any(|window| window == b"Exif"));
}

#[tokio::test]
async fn sign_in_picture_rejects_a_large_decoded_buffer_within_dimension_limits() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("mira", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "mira", "password123").await;
    // This compresses below the upload limit and fits the dimension limit, but
    // a 16-bit RGBA output buffer alone needs more than the 64 MB decode budget.
    let picture = image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
        3000,
        3000,
        image::Rgba([0_u16, 0, 0, u16::MAX]),
    ));
    let mut bytes = std::io::Cursor::new(Vec::new());
    picture
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    drop(picture);
    let bytes = bytes.into_inner();
    assert!(bytes.len() < 1024 * 1024);
    assert_eq!(
        request(&app, "PUT", Some(&cookie), Some("image/png"), bytes)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        public_picture(&app, &format!("/api/auth/users/{}/avatar", user.id))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn avatar_is_private_to_each_account_and_can_be_removed() {
    let app = common::test_app().await;
    let alice = app
        .state
        .auth
        .create_user("alice", "password123", Role::User)
        .await
        .unwrap();
    app.state
        .auth
        .create_user("bob", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET profile_type = 'child' WHERE id = ?")
        .bind(alice.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let alice_cookie = common::login(&app, "alice", "password123").await;
    let bob_cookie = common::login(&app, "bob", "password123").await;
    let picture = picture();
    assert!(me_avatar_version(&app, &alice_cookie).await.is_null());

    assert_eq!(
        request(&app, "GET", None, None, vec![]).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "PUT", None, Some("image/png"), picture.clone())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, "GET", Some(&alice_cookie), None, vec![])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            Some(&alice_cookie),
            Some("image/png"),
            picture.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, headers, data) = request(&app, "GET", Some(&alice_cookie), None, vec![]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(data, picture);
    assert_eq!(
        request(&app, "GET", Some(&bob_cookie), None, vec![])
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    let version: i64 = sqlx::query_scalar("SELECT avatar_version FROM users WHERE id = ?")
        .bind(alice.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(version, 1);
    let stored: (Vec<u8>, String) =
        sqlx::query_as("SELECT data, mime FROM user_avatars WHERE user_id = ?")
            .bind(alice.id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(stored, (picture, "image/png".into()));
    assert_eq!(me_avatar_version(&app, &alice_cookie).await, 1);
    assert_eq!(
        request(&app, "DELETE", Some(&alice_cookie), None, vec![])
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "GET", Some(&alice_cookie), None, vec![])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(me_avatar_version(&app, &alice_cookie).await.is_null());
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM user_avatars WHERE user_id = ?")
        .bind(alice.id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn avatar_rejects_invalid_type_and_oversize_without_replacing_picture() {
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let cookie = common::login(&app, "reader", "password123").await;
    let picture = picture();
    assert_eq!(
        request(
            &app,
            "PUT",
            Some(&cookie),
            Some("image/png"),
            picture.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let invalid = b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec();
    assert_eq!(
        request(&app, "PUT", Some(&cookie), Some("image/svg+xml"), invalid)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            Some(&cookie),
            Some("image/png"),
            vec![0; 1024 * 1024 + 1]
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            Some(&cookie),
            Some("image/jpeg"),
            picture.clone()
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(&app, "GET", Some(&cookie), None, vec![]).await.2,
        picture
    );
}

async fn profile_json(
    app: &common::TestApp,
    method: &str,
    cookie: &str,
    payload: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(if method == "PUT" {
                    if payload.get("avatarPreset").is_some() {
                        "/api/profile/avatar/preset"
                    } else {
                        "/api/profile"
                    }
                } else {
                    "/api/auth/me"
                })
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn profile_marks_are_optional_profile_scoped_and_separate_from_uploaded_photos() {
    use serde_json::json;
    let app = common::test_app().await;
    app.state
        .auth
        .create_user("mira", "password123", Role::User)
        .await
        .unwrap();
    app.state
        .auth
        .create_user_with_profile("nora", "482915", Role::User, "pin", "child")
        .await
        .unwrap();
    let mira = common::login(&app, "mira", "password123").await;
    let nora = common::login(&app, "nora", "482915").await;
    let initial = profile_json(&app, "GET", &mira, json!(null)).await.1;
    assert!(initial["user"]["avatarPreset"].is_null());
    assert!(initial["user"]["avatarVersion"].is_null());
    for preset in [
        "fox", "owl", "cat", "bear", "whale", "book", "tree", "mountain", "moon", "leaf",
    ] {
        let (status, body) =
            profile_json(&app, "PUT", &mira, json!({ "avatarPreset": preset })).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["user"]["avatarPreset"], preset);
        assert!(body["user"]["avatarVersion"].is_null());
    }
    let child = profile_json(&app, "PUT", &nora, json!({ "avatarPreset": "owl" })).await;
    assert_eq!(child.0, StatusCode::OK);
    assert_eq!(child.1["user"]["avatarPreset"], "owl");
    assert_eq!(
        profile_json(&app, "PUT", &nora, json!({ "displayName": "Not allowed" }))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        profile_json(
            &app,
            "PUT",
            &nora,
            json!({ "avatarPreset": "fox", "canAcquire": true })
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        profile_json(
            &app,
            "PUT",
            &nora,
            json!({ "avatarPreset": "fox", "displayName": "Not allowed" })
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        profile_json(&app, "GET", &mira, json!(null)).await.1["user"]["avatarPreset"],
        "leaf"
    );
    assert_eq!(
        profile_json(&app, "PUT", "", json!({ "avatarPreset": "cat" }))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );

    assert_eq!(
        request(&app, "PUT", Some(&mira), Some("image/png"), picture())
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let before = profile_json(&app, "GET", &mira, json!(null)).await.1;
    let after = profile_json(&app, "PUT", &mira, json!({ "avatarPreset": "fox" }))
        .await
        .1;
    assert_eq!(
        after["user"]["avatarVersion"],
        before["user"]["avatarVersion"]
    );
    assert_eq!(
        request(&app, "GET", Some(&mira), None, vec![]).await.2,
        picture()
    );
    let response = public_picture(&app, "/api/auth/users").await;
    let bytes = to_bytes(response.into_body(), 8192).await.unwrap();
    let users: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let public = users["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["username"] == "mira")
        .unwrap();
    assert_eq!(public["avatarPreset"], "fox");
    assert!(public["avatarUrl"].is_string());

    assert_eq!(
        request(&app, "DELETE", Some(&mira), None, vec![]).await.0,
        StatusCode::NO_CONTENT
    );
    let restored = profile_json(&app, "GET", &mira, json!(null)).await.1;
    assert_eq!(restored["user"]["avatarPreset"], "fox");
    assert!(restored["user"]["avatarVersion"].is_null());
    assert_eq!(
        profile_json(&app, "PUT", &mira, json!({ "displayName": "Mira" }))
            .await
            .1["user"]["avatarPreset"],
        "fox"
    );
    for (bad, expected) in [
        (json!("../fox"), StatusCode::UNPROCESSABLE_ENTITY),
        (json!("unknown"), StatusCode::UNPROCESSABLE_ENTITY),
        (json!(123), StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(
            profile_json(&app, "PUT", &mira, json!({ "avatarPreset": bad }))
                .await
                .0,
            expected
        );
        let unchanged = profile_json(&app, "GET", &mira, json!(null)).await.1;
        assert_eq!(unchanged["user"]["avatarPreset"], "fox");
        assert_eq!(unchanged["user"]["displayName"], "Mira");
    }
    assert!(
        profile_json(&app, "PUT", &mira, json!({ "avatarPreset": null }))
            .await
            .1["user"]["avatarPreset"]
            .is_null()
    );
    assert_eq!(
        profile_json(&app, "GET", &nora, json!(null)).await.1["user"]["avatarPreset"],
        "owl"
    );
}
