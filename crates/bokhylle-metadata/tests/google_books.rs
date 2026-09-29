use bokhylle_metadata::google_books::GoogleBooksClient;
use bokhylle_metadata::{MetadataError, MetadataProvider, MetadataQuery};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn search_page_advances_past_unusable_provider_entries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/volumes"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"items":[{"id":"good","volumeInfo":{"title":"Found"}},{"id":"bad"}]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let client = GoogleBooksClient::with_base_url(&server.uri(), None).unwrap();
    let page = client
        .search_page(&MetadataQuery {
            title: Some("found".to_string()),
            limit: 2,
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.next.as_deref(), Some("2"));
}

#[tokio::test]
async fn rejects_oversized_json_response() {
    let server = MockServer::start().await;
    let large_response = format!(
        r#"{{"items":[],"padding":"{}"}}"#,
        "x".repeat(10 * 1024 * 1024)
    );
    Mock::given(method("GET"))
        .and(path("/volumes"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(large_response, "application/json"))
        .mount(&server)
        .await;

    let client = GoogleBooksClient::with_base_url(&server.uri(), None).unwrap();
    let error = client
        .search(&MetadataQuery {
            title: Some("found".to_string()),
            limit: 2,
            ..Default::default()
        })
        .await
        .unwrap_err();

    assert!(matches!(error, MetadataError::Status(413)));
}
