use bokhylle_acquisition::model::{AcquisitionMethod, ExpectedBook};
use bokhylle_acquisition::provider::IndexerProvider;
use bokhylle_acquisition::torznab::TorznabClient;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> TorznabClient {
    TorznabClient::new(&format!("{}/api", server.uri()), "secret-key", vec![7000]).unwrap()
}

fn feed() -> String {
    r#"<rss xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
      <item><title>Andy.Weir.Project.Hail.Mary.EN.EPUB</title><guid>release-1</guid>
        <link>/details/1</link>
        <enclosure url="/download?id=1&amp;apikey=secret-key" length="3145728" type="application/x-bittorrent"/>
        <torznab:attr name="indexer" value="Books Indexer"/>
        <torznab:attr name="seeders" value="14"/>
        <torznab:attr name="peers" value="2"/>
      </item>
      <item><title>NZB result</title><guid>release-2</guid>
        <enclosure url="/nzb/2" length="1000" type="application/x-nzb"/>
      </item>
    </channel></rss>"#.to_string()
}

#[tokio::test]
async fn searches_standard_rss_and_fetches_torrent_without_journaling_the_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api"))
        .and(query_param("t", "search"))
        .and(query_param("cat", "7000"))
        .and(query_param("apikey", "secret-key"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(feed(), "application/xml"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .and(query_param("apikey", "secret-key"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw("d4:infodee", "application/x-bittorrent"),
        )
        .mount(&server)
        .await;

    let client = client(&server);
    let releases = client.search("Project Hail Mary").await.unwrap();
    assert_eq!(releases.len(), 1);
    let release = &releases[0];
    assert_eq!(release.source.as_ref().unwrap().kind, "torznab");
    assert_eq!(release.seeders, Some(14));
    assert_eq!(release.size_bytes, 3_145_728);
    assert!(matches!(
        release.method,
        Some(AcquisitionMethod::Torrent { .. })
    ));
    assert!(
        !serde_json::to_string(release)
            .unwrap()
            .contains("secret-key")
    );
    let bytes = client.fetch_torrent(release).await.unwrap();
    assert_eq!(bytes.as_slice(), b"d4:infodee");
}

#[tokio::test]
async fn capabilities_and_book_queries_work_without_a_vendor_api() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api"))
        .and(query_param("t", "caps"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"<caps><server version="2.1"/><searching><search available="yes" supportedParams="q"/></searching></caps>"#,
            "application/xml",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api"))
        .and(query_param("t", "search"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(feed(), "application/xml"))
        .mount(&server)
        .await;

    let client = client(&server);
    assert_eq!(client.check().await.unwrap(), "2.1");
    let outcome = client
        .search_book(&ExpectedBook {
            title: "Project Hail Mary".to_string(),
            authors: vec!["Andy Weir".to_string()],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(outcome.candidates.len(), 1);
    assert_eq!(outcome.queries.len(), 1);
}

#[tokio::test]
async fn rejects_private_foreign_redirects_and_oversized_responses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            format!("<rss><channel><item><title>Book EPUB</title><enclosure url=\"{}/redirect\" length=\"3000000\"/></item></channel></rss>", server.uri()),
            "application/xml",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "http://127.0.0.1:9/private"),
        )
        .mount(&server)
        .await;
    let torznab = client(&server);
    let releases = torznab.search("Book").await.unwrap();
    assert!(torznab.fetch_torrent(&releases[0]).await.is_err());

    let oversized = MockServer::start().await;
    let body = format!(
        "<rss><channel>{}</channel></rss>",
        " ".repeat(8 * 1024 * 1024)
    );
    Mock::given(method("GET"))
        .and(path("/api"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/xml"))
        .mount(&oversized)
        .await;
    assert!(client(&oversized).search("Book").await.is_err());
}
