use bokhylle_acquisition::model::{ExpectedBook, ReleaseCandidate};
use bokhylle_acquisition::prowlarr::{BOOK_CATEGORY, MAX_API_RESPONSE_BYTES, ProwlarrClient};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH_FIXTURE: &str = r#"[
  {
    "guid": "abc",
    "title": "Andy.Weir.Project.Hail.Mary.RETAIL.EN.EPUB",
    "indexer": "Indexer A",
    "size": 3145728,
    "seeders": 14,
    "leechers": 2,
    "downloadUrl": "http://prowlarr.test/download/1",
    "magnetUrl": "magnet:?xt=urn:btih:AAA",
    "infoUrl": "http://prowlarr.test/info/1"
  },
  {
    "title": "Project Hail Mary ebook pack",
    "size": 1048576,
    "seeders": 0
  }
]"#;

fn client(server: &MockServer) -> ProwlarrClient {
    ProwlarrClient::new(&server.uri(), "secret-key").unwrap()
}

#[tokio::test]
async fn search_normalizes_releases() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .and(header("X-Api-Key", "secret-key"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(SEARCH_FIXTURE, "application/json"))
        .mount(&server)
        .await;

    let candidates = client(&server)
        .search("project hail mary", &[BOOK_CATEGORY])
        .await
        .unwrap();

    assert_eq!(candidates.len(), 2);
    let first = &candidates[0];
    assert_eq!(first.id, "abc");
    assert_eq!(first.title, "Andy.Weir.Project.Hail.Mary.RETAIL.EN.EPUB");
    assert_eq!(first.indexer.as_deref(), Some("Indexer A"));
    assert_eq!(first.size_bytes, 3_145_728);
    assert_eq!(first.seeders, Some(14));
    assert_eq!(first.leechers, Some(2));
    assert_eq!(first.magnet_url.as_deref(), Some("magnet:?xt=urn:btih:AAA"));

    let second = &candidates[1];
    assert_eq!(second.id, second.title);
    assert!(second.indexer.is_none());
}

#[tokio::test]
async fn search_book_uses_progressive_queries() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .and(query_param("query", "Project Hail Mary Andy Weir"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("[]", "application/json"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(SEARCH_FIXTURE, "application/json"))
        .mount(&server)
        .await;

    let book = ExpectedBook {
        title: "Project Hail Mary".to_string(),
        authors: vec!["Andy Weir".to_string()],
        language: Some("en".to_string()),
        preferred_format: Some("epub".to_string()),
        ..Default::default()
    };

    let outcome = client(&server).search_book(&book).await.unwrap();

    assert_eq!(outcome.queries.len(), 2);
    assert!(outcome.queries[0].contains("0 candidates"));
    assert!(outcome.queries[1].contains("Project Hail Mary"));
    assert_eq!(outcome.candidates.len(), 2);
}

#[tokio::test]
async fn search_book_stops_after_a_matching_mixed_format_release() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .and(query_param("query", "The Historian Elizabeth Kostova"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"[{"guid":"historian","title":"Elizabeth Kostova - The Historian (azw3 epub mobi)","size":3000000,"seeders":4}]"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let book = ExpectedBook {
        title: "The Historian".to_string(),
        authors: vec!["Elizabeth Kostova".to_string()],
        preferred_format: Some("epub".to_string()),
        ..Default::default()
    };
    let outcome = client(&server).search_book(&book).await.unwrap();

    assert_eq!(outcome.queries.len(), 1);
    assert_eq!(outcome.candidates.len(), 1);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn test_connection_reports_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/system/status"))
        .and(header("X-Api-Key", "secret-key"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(r#"{"version": "1.30.0"}"#, "application/json"),
        )
        .mount(&server)
        .await;

    let version = client(&server).test_connection().await.unwrap();
    assert_eq!(version, "1.30.0");
}

#[tokio::test]
async fn a_numbered_pack_does_not_stop_the_search_for_a_standalone_book() {
    let server = MockServer::start().await;
    for (query, release) in [
        (
            "Dune Frank Herbert",
            serde_json::json!({
                "guid":"pack", "title":"Frank Herbert - [Dune 01-06] (epub)",
                "size":8_000_000, "seeders":80,
            }),
        ),
        (
            "Dune",
            serde_json::json!({
                "guid":"standalone", "title":"Frank Herbert - Dune 2003 Retail EPUB eBook-Fixture",
                "size":3_000_000, "seeders":128,
            }),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path("/api/v1/search"))
            .and(query_param("query", query))
            .and(query_param("categories", "7000"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![release]))
            .expect(1)
            .mount(&server)
            .await;
    }
    let book = ExpectedBook {
        title: "Dune".into(),
        authors: vec!["Frank Herbert".into()],
        ..Default::default()
    };
    let outcome = client(&server).search_book(&book).await.unwrap();
    assert_eq!(outcome.queries.len(), 2);
    assert_eq!(outcome.candidates[0].id, "standalone");
    let ranked = bokhylle_acquisition::evaluator::rank(&book, &outcome.candidates);
    assert_eq!(
        bokhylle_acquisition::evaluator::select(&ranked),
        bokhylle_acquisition::model::Selection::Auto { index: 0 }
    );
}

#[tokio::test]
async fn surfaces_http_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let error = client(&server)
        .search("anything", &[BOOK_CATEGORY])
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        bokhylle_acquisition::prowlarr::ProwlarrError::Status(401)
    ));
}

#[tokio::test]
async fn search_rejects_an_oversized_json_response() {
    let server = MockServer::start().await;
    let mut body = b"[]".to_vec();
    body.resize(MAX_API_RESPONSE_BYTES as usize + 1, b' ');
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .mount(&server)
        .await;

    let error = client(&server)
        .search("anything", &[BOOK_CATEGORY])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("size limit"));
}

fn release_with_download_url(url: &str) -> ReleaseCandidate {
    ReleaseCandidate {
        source: None,
        method: None,
        id: "1".to_string(),
        title: "Project Hail Mary".to_string(),
        indexer: Some("Indexer A".to_string()),
        size_bytes: 1_000,
        seeders: Some(1),
        leechers: Some(0),
        download_url: Some(url.to_string()),
        magnet_url: None,
        info_url: None,
        detected_title: None,
        detected_author: None,
        detected_format: None,
        detected_language: None,
        detected_volume: None,
        is_collection: false,
        is_audiobook: false,
        is_comic: false,
    }
}

#[tokio::test]
async fn fetch_torrent_only_attaches_the_api_key_to_the_prowlarr_origin() {
    use bokhylle_acquisition::provider::IndexerProvider;

    let prowlarr = MockServer::start().await;
    let other = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/download/1"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"torrent-bytes".to_vec()))
        .mount(&prowlarr)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/2"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"other-bytes".to_vec()))
        .mount(&other)
        .await;

    let client = ProwlarrClient::new(&prowlarr.uri(), "secret-key")
        .unwrap()
        .with_allow_private_destinations(true);

    client
        .fetch_torrent(&release_with_download_url(&format!(
            "{}/download/1",
            prowlarr.uri()
        )))
        .await
        .unwrap();
    client
        .fetch_torrent(&release_with_download_url(&format!(
            "{}/download/2",
            other.uri()
        )))
        .await
        .unwrap();

    let prowlarr_requests = prowlarr.received_requests().await.unwrap();
    assert!(
        prowlarr_requests
            .iter()
            .any(|request| request.headers.get("x-api-key").is_some()),
        "same-origin downloads must carry the API key"
    );

    let other_requests = other.received_requests().await.unwrap();
    assert_eq!(other_requests.len(), 1);
    assert!(
        other_requests[0].headers.get("x-api-key").is_none(),
        "foreign origins must never receive the Prowlarr API key"
    );
}

#[tokio::test]
async fn fetch_torrent_enforces_a_size_cap() {
    use bokhylle_acquisition::provider::IndexerProvider;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/download/1"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![0u8; 64]))
        .mount(&server)
        .await;

    let client = ProwlarrClient::new(&server.uri(), "secret-key")
        .unwrap()
        .with_max_torrent_bytes(16);

    let error = client
        .fetch_torrent(&release_with_download_url(&format!(
            "{}/download/1",
            server.uri()
        )))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("size limit"));
}

#[tokio::test]
async fn redirects_away_from_prowlarr_do_not_forward_the_api_key() {
    use bokhylle_acquisition::provider::IndexerProvider;

    let prowlarr = MockServer::start().await;
    let cdn = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/download/1"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", format!("{}/torrent", cdn.uri())),
        )
        .mount(&prowlarr)
        .await;
    Mock::given(method("GET"))
        .and(path("/torrent"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"torrent-from-cdn".to_vec()))
        .mount(&cdn)
        .await;

    let client = ProwlarrClient::new(&prowlarr.uri(), "secret-key")
        .unwrap()
        .with_allow_private_destinations(true);
    let bytes = client
        .fetch_torrent(&release_with_download_url(&format!(
            "{}/download/1",
            prowlarr.uri()
        )))
        .await
        .unwrap();

    assert_eq!(bytes.as_slice(), b"torrent-from-cdn");

    let prowlarr_requests = prowlarr.received_requests().await.unwrap();
    assert!(
        prowlarr_requests
            .iter()
            .any(|request| request.headers.get("x-api-key").is_some())
    );

    let cdn_requests = cdn.received_requests().await.unwrap();
    assert_eq!(cdn_requests.len(), 1);
    assert!(
        cdn_requests[0].headers.get("x-api-key").is_none(),
        "redirected requests must not carry the Prowlarr API key"
    );
}

#[tokio::test]
async fn redirects_within_prowlarr_keep_the_api_key() {
    use bokhylle_acquisition::provider::IndexerProvider;

    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/download/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/download/real"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/real"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"torrent-bytes".to_vec()))
        .mount(&server)
        .await;

    let client = ProwlarrClient::new(&server.uri(), "secret-key").unwrap();
    client
        .fetch_torrent(&release_with_download_url(&format!(
            "{}/download/redirect",
            server.uri()
        )))
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.headers.get("x-api-key").is_some()),
        "same-origin redirects may keep the API key"
    );
}

#[tokio::test]
async fn low_confidence_queries_keep_their_candidates_for_selection() {
    let server = MockServer::start().await;
    let weak_fixture = r#"[{
        "guid": "weak",
        "title": "Project Mary EPUB",
        "indexer": "Indexer A",
        "size": 1500000,
        "seeders": 1,
        "leechers": 0
    }]"#;

    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(weak_fixture, "application/json"))
        .mount(&server)
        .await;

    let book = ExpectedBook {
        title: "Project Hail Mary".to_string(),
        authors: vec!["Andy Weir".to_string()],
        language: Some("en".to_string()),
        preferred_format: Some("epub".to_string()),
        ..Default::default()
    };

    let outcome = client(&server).search_book(&book).await.unwrap();

    assert!(
        !outcome.candidates.is_empty(),
        "weak candidates must survive for manual selection, not become 'no release'"
    );
    assert!(
        outcome.queries.len() > 1,
        "the search should keep broadening queries for weak results"
    );
}

#[tokio::test]
async fn foreign_private_destinations_are_rejected() {
    use bokhylle_acquisition::provider::IndexerProvider;

    let prowlarr = MockServer::start().await;
    let private = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/download/1"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"private".to_vec()))
        .mount(&private)
        .await;

    let client = ProwlarrClient::new(&prowlarr.uri(), "secret-key").unwrap();
    let error = client
        .fetch_torrent(&release_with_download_url(&format!(
            "{}/download/1",
            private.uri()
        )))
        .await
        .unwrap_err();

    assert!(
        error.to_string().contains("not a public address"),
        "a loopback destination must be refused: {error}"
    );
    assert!(
        private.received_requests().await.unwrap().is_empty(),
        "the private destination must never be contacted"
    );
}
