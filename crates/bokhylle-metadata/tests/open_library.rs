use bokhylle_metadata::open_library::OpenLibraryClient;
use bokhylle_metadata::{MetadataProvider, MetadataQuery};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SEARCH_FIXTURE: &str = r#"{
  "numFound": 2,
  "docs": [
    {
      "key": "/works/OL20894485W",
      "title": "Project Hail Mary",
      "author_name": ["Andy Weir"],
      "first_publish_year": 2021,
      "isbn": ["9780593135204", "0593135202", "9780593135211"],
      "edition_key": ["OL29406927M"],
      "cover_i": 10580234,
      "language": ["eng"]
    },
    {
      "key": "/works/OL1W",
      "title": "No Author Book"
    }
  ]
}"#;

#[tokio::test]
async fn author_profile_uses_a_valid_id_and_bounded_plain_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/authors/OL123A.json"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"bio":{"type":"/type/text","value":"<p>An author of many books.</p>"},"birth_date":" 1900 ","death_date":"1980"}"#,
            "application/json",
        ))
        .expect(1)
        .mount(&server)
        .await;
    let client = OpenLibraryClient::with_base_url(&server.uri(), &server.uri()).unwrap();
    let profile = client
        .get_author_profile("/authors/OL123A")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(profile.bio.as_deref(), Some("An author of many books."));
    assert_eq!(profile.birth_date.as_deref(), Some("1900"));
    assert_eq!(profile.death_date.as_deref(), Some("1980"));
    assert!(client.get_author_profile("../../bad").await.is_err());

    Mock::given(method("GET"))
        .and(path("/authors/OL456A.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "bio": "A".repeat(1200) })),
        )
        .mount(&server)
        .await;
    let long_profile = client.get_author_profile("OL456A").await.unwrap().unwrap();
    assert_eq!(long_profile.bio.unwrap().chars().count(), 800);
}

#[tokio::test]
async fn search_maps_provider_results() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(SEARCH_FIXTURE, "application/json"))
        .mount(&server)
        .await;

    let client = OpenLibraryClient::with_base_url(&server.uri(), &server.uri()).unwrap();
    let results = client
        .search(&MetadataQuery {
            title: Some("Project Hail Mary".to_string()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    let first = &results[0];
    assert_eq!(first.provider, "openlibrary");
    assert_eq!(first.provider_key, "/works/OL20894485W");
    assert_eq!(first.title, "Project Hail Mary");
    assert_eq!(first.authors, vec!["Andy Weir"]);
    assert_eq!(first.year, Some(2021));
    assert_eq!(first.isbn13.as_deref(), Some("9780593135204"));
    assert_eq!(first.isbn10.as_deref(), Some("0593135202"));
    assert_eq!(first.cover_id.as_deref(), Some("10580234"));
    assert_eq!(first.language.as_deref(), Some("en"));

    let second = &results[1];
    assert!(second.authors.is_empty());
    assert_eq!(second.cover_id, None);
}

#[tokio::test]
async fn search_retries_server_errors_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(SEARCH_FIXTURE, "application/json"))
        .mount(&server)
        .await;

    let client = OpenLibraryClient::with_base_url(&server.uri(), &server.uri()).unwrap();
    let results = client
        .search(&MetadataQuery {
            title: Some("retry".to_string()),
            limit: 5,
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn search_surfaces_status_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = OpenLibraryClient::with_base_url(&server.uri(), &server.uri()).unwrap();
    let error = client
        .search(&MetadataQuery {
            isbn: Some("9780593135204".to_string()),
            limit: 5,
            ..Default::default()
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        bokhylle_metadata::MetadataError::Status(404)
    ));
}

#[tokio::test]
async fn search_page_advances_past_unusable_provider_entries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"docs":[{"key":"/works/OL1W","title":"Found"},{"key":"/works/OL2W"}]}"#,
            "application/json",
        ))
        .mount(&server)
        .await;

    let client = OpenLibraryClient::with_base_url(&server.uri(), &server.uri()).unwrap();
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
async fn get_book_returns_none_for_missing_work() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/works/OL404W.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let client = OpenLibraryClient::with_base_url(&server.uri(), &server.uri()).unwrap();
    let result = client.get_book("/works/OL404W").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn work_detail_does_not_borrow_year_or_language_from_sampled_editions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/works/OL42W.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "title": "Dune",
            "first_publish_date": "1965",
            "authors": []
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/works/OL42W/editions.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "entries": [
                {"languages": [{"key": "/languages/rus"}], "publish_date": "2021"},
                {"languages": [{"key": "/languages/eng"}], "publish_date": "1965"}
            ]
        })))
        .mount(&server)
        .await;
    let client = OpenLibraryClient::with_base_url(&server.uri(), &server.uri()).unwrap();
    let book = client.get_book("/works/OL42W").await.unwrap().unwrap();
    assert_eq!(book.year, Some(1965));
    assert_eq!(book.language, None);
    assert_eq!(book.languages, ["ru", "en"]);
}

#[tokio::test]
async fn missing_artwork_is_a_real_not_found_response() {
    let server = MockServer::start().await;
    for artwork_path in [
        "/b/id/123-L.jpg",
        "/b/isbn/9781234567897-L.jpg",
        "/a/olid/OL123A-M.jpg",
    ] {
        Mock::given(method("GET"))
            .and(path(artwork_path))
            .and(query_param("default", "false"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
    }
    let client = OpenLibraryClient::with_base_url(&server.uri(), &server.uri()).unwrap();
    assert!(client.fetch_cover("123").await.unwrap().is_none());
    assert!(
        client
            .fetch_cover_by_isbn("9781234567897")
            .await
            .unwrap()
            .is_none()
    );
    assert!(client.fetch_author_photo("OL123A").await.unwrap().is_none());
}
