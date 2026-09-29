use bokhylle_acquisition::qbittorrent::{
    DEFAULT_CATEGORY, MAX_API_RESPONSE_BYTES, QbittorrentAuth, QbittorrentClient, TAG_PREFIX,
};
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn api_key_client(server: &MockServer) -> QbittorrentClient {
    QbittorrentClient::new(&server.uri(), QbittorrentAuth::ApiKey("secret".to_string())).unwrap()
}

#[tokio::test]
async fn api_key_mode_uses_bearer_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/app/version"))
        .and(header("Authorization", "Bearer secret"))
        .respond_with(ResponseTemplate::new(200).set_body_string("5.2.1"))
        .mount(&server)
        .await;

    let version = api_key_client(&server).test_connection().await.unwrap();
    assert_eq!(version, "5.2.1");
}

#[tokio::test]
async fn session_mode_logs_in_and_reuses_sid() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/login"))
        .and(body_string_contains("username=alice"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "SID=abc123; path=/; HttpOnly")
                .set_body_string("Ok."),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/app/version"))
        .and(header("Cookie", "SID=abc123"))
        .respond_with(ResponseTemplate::new(200).set_body_string("4.6.0"))
        .mount(&server)
        .await;

    let client = QbittorrentClient::new(
        &server.uri(),
        QbittorrentAuth::Credentials {
            username: "alice".to_string(),
            password: "hunter2".to_string(),
        },
    )
    .unwrap();

    assert_eq!(client.test_connection().await.unwrap(), "4.6.0");
    assert_eq!(client.test_connection().await.unwrap(), "4.6.0");
}

#[tokio::test]
async fn session_mode_reports_bad_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Fails."))
        .mount(&server)
        .await;

    let client = QbittorrentClient::new(
        &server.uri(),
        QbittorrentAuth::Credentials {
            username: "alice".to_string(),
            password: "wrong".to_string(),
        },
    )
    .unwrap();

    assert!(matches!(
        client.test_connection().await.unwrap_err(),
        bokhylle_acquisition::qbittorrent::QbittorrentError::Auth
    ));
}

#[tokio::test]
async fn add_magnet_sets_category_and_tag() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/torrents/add"))
        .and(body_string_contains("category=books-app"))
        .and(body_string_contains("tags=books-acquisition-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Ok."))
        .mount(&server)
        .await;

    api_key_client(&server)
        .add_magnet(
            "magnet:?xt=urn:btih:AAA",
            DEFAULT_CATEGORY,
            &format!("{TAG_PREFIX}abc"),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn torrent_info_parses_and_ownership_is_enforced() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/torrents/info"))
        .and(query_param("hashes", "abcdef"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"[{
                "hash": "abcdef",
                "name": "Project Hail Mary",
                "state": "downloading",
                "progress": 0.42,
                "size": 3145728,
                "downloaded": 1320000,
                "save_path": "/downloads",
                "content_path": "/downloads/Project Hail Mary",
                "category": "movies",
                "tags": "other",
                "eta": 60,
                "dlspeed": 1024
            }]"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let client = api_key_client(&server);
    let info = client.torrent_info("abcdef").await.unwrap().unwrap();
    assert_eq!(info.progress, 0.42);
    assert_eq!(info.state, "downloading");
    assert_eq!(info.save_path, "/downloads");
    assert_eq!(
        info.content_path.as_deref(),
        Some("/downloads/Project Hail Mary")
    );
    assert_eq!(info.dl_speed, Some(1024));
    assert!(!info.is_owned(DEFAULT_CATEGORY));

    let error = client
        .cancel_owned("abcdef", DEFAULT_CATEGORY, true)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        bokhylle_acquisition::qbittorrent::QbittorrentError::NotOwned
    ));
}

#[tokio::test]
async fn cancel_owned_deletes_owned_torrents() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/torrents/info"))
        .and(query_param("hashes", "abcdef"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"[{
                "hash": "abcdef",
                "name": "Project Hail Mary",
                "state": "uploading",
                "progress": 1.0,
                "size": 3145728,
                "downloaded": 3145728,
                "save_path": "/downloads",
                "category": "books-app",
                "tags": "books-acquisition-xyz"
            }]"#,
            "application/json",
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/torrents/delete"))
        .and(body_string_contains("hashes=abcdef"))
        .and(body_string_contains("deleteFiles=true"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let removed = api_key_client(&server)
        .cancel_owned("abcdef", DEFAULT_CATEGORY, true)
        .await
        .unwrap();
    assert!(removed);
}

#[tokio::test]
async fn missing_torrent_cancel_is_a_noop() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/torrents/info"))
        .and(query_param("hashes", "gone"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("[]", "application/json"))
        .mount(&server)
        .await;

    let removed = api_key_client(&server)
        .cancel_owned("gone", DEFAULT_CATEGORY, true)
        .await
        .unwrap();
    assert!(!removed);
}

#[tokio::test]
async fn add_reports_qbittorrent_rejections() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/torrents/add"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Fails."))
        .mount(&server)
        .await;

    let error = api_key_client(&server)
        .add_magnet(
            "magnet:?xt=urn:btih:AAA",
            DEFAULT_CATEGORY,
            "books-acquisition-x",
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("rejected"));
}

#[tokio::test]
async fn torrent_list_rejects_an_oversized_json_response() {
    let server = MockServer::start().await;
    let mut body = b"[]".to_vec();
    body.resize(MAX_API_RESPONSE_BYTES as usize + 1, b' ');
    Mock::given(method("GET"))
        .and(path("/api/v2/torrents/info"))
        .and(query_param("category", DEFAULT_CATEGORY))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .mount(&server)
        .await;

    let error = api_key_client(&server)
        .torrents_in_category(DEFAULT_CATEGORY)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("size limit"));
}
