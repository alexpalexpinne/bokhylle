use bokhylle_metadata::MetadataProvider;
use bokhylle_metadata::disabled::DisabledProvider;
use bokhylle_metadata::google_books::GoogleBooksClient;

#[test]
fn google_books_is_an_enrichment_provider() {
    let client = GoogleBooksClient::with_base_url("http://127.0.0.1:1", None).unwrap();
    let capabilities = client.capabilities();
    assert!(
        !capabilities.durable_identity,
        "Google must not own a durable catalogue identity"
    );
    assert!(
        !capabilities.persistent_metadata,
        "Google records get the short cache TTL"
    );
    assert!(capabilities.covers);
    assert!(
        !capabilities.author_search,
        "Google Books has no stable author search"
    );
}

#[test]
fn open_library_keeps_the_durable_defaults() {
    let client = bokhylle_metadata::open_library::OpenLibraryClient::new().unwrap();
    let capabilities = client.capabilities();
    assert!(capabilities.durable_identity);
    assert!(capabilities.persistent_metadata);
    assert!(capabilities.author_search);
}

#[test]
fn a_disabled_provider_declares_nothing() {
    let capabilities = DisabledProvider.capabilities();
    assert!(!capabilities.durable_identity);
    assert!(!capabilities.persistent_metadata);
    assert!(!capabilities.covers);
    assert!(!capabilities.ratings);
    assert!(!capabilities.author_search);
}
