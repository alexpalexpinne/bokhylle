mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation, ProtocolVersion,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{ClientLifecycleMode, ClientServiceExt, service::RunningService};
use serde_json::{Value, json};
use tower::ServiceExt;

use bokhylle_metadata::MetadataResult;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Role;

const TOOL_NAMES: [&str; 9] = [
    "add_book",
    "add_catalogue_book",
    "continue_reading",
    "get_book",
    "list_my_shelf",
    "list_requests",
    "request_book",
    "search_books",
    "send_to_reader",
];

type Client = RunningService<rmcp::RoleClient, ClientConfig>;

async fn serve(app: common::TestApp) -> (String, common::TestApp) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = app.router.clone();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{addr}"), app)
}

async fn connect_with_origin(
    base: &str,
    token: &str,
    origin: Option<&str>,
) -> Result<Client, String> {
    let mut config =
        StreamableHttpClientTransportConfig::with_uri(format!("{base}/mcp")).auth_header(token);
    if let Some(origin) = origin {
        let mut headers = std::collections::HashMap::new();
        headers.insert(
            header::ORIGIN,
            axum::http::HeaderValue::from_str(origin).unwrap(),
        );
        config = config.custom_headers(headers);
    }
    let transport = StreamableHttpClientTransport::from_config(config);
    let client_info = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new("bokhylle-test", "0.0.1"),
    );
    client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                legacy_version: Some(ProtocolVersion::V_2025_11_25),
            },
        )
        .await
        .map_err(|error| error.to_string())
}

async fn connect(base: &str, token: &str) -> Client {
    connect_with_origin(base, token, None)
        .await
        .expect("connect")
}

async fn call(client: &Client, name: &str, args: Value) -> Result<Value, String> {
    let arguments = args.as_object().cloned().unwrap_or_default();
    let params = CallToolRequestParams::new(name.to_string()).with_arguments(arguments);
    match client.call_tool(params).await {
        Ok(result) => {
            let text = result
                .content
                .iter()
                .filter_map(|block| block.as_text().map(|text| text.text.clone()))
                .collect::<Vec<_>>()
                .join("\n");
            if result.is_error.unwrap_or(false) {
                return Err(text);
            }
            Ok(result.structured_content.unwrap_or(Value::Null))
        }
        Err(error) => Err(error.to_string()),
    }
}

async fn seed_book(app: &common::TestApp, key: &str, title: &str) -> i64 {
    bokhylle_server::library::import_metadata::upsert_book_from_metadata(
        &app.state.db,
        &MetadataResult {
            provider: "fake".to_string(),
            provider_key: key.to_string(),
            title: title.to_string(),
            authors: vec!["MCP Author".to_string()],
            language: Some("en".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

async fn add_file(app: &common::TestApp, book_id: i64, digest: &str) {
    let edition_id: i64 = sqlx::query_scalar("SELECT id FROM editions WHERE book_id = ? LIMIT 1")
        .bind(book_id)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE editions SET language = 'en' WHERE id = ?")
        .bind(edition_id)
        .execute(&app.state.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO book_files (edition_id, path, format, size, sha256)
         VALUES (?, ?, 'epub', 10, ?)",
    )
    .bind(edition_id)
    .bind(format!("/tmp/mcp-{digest}.epub"))
    .bind(digest)
    .execute(&app.state.db)
    .await
    .unwrap();
}

async fn raw_post(app: &common::TestApp, token: Option<&str>, origin: Option<&str>) -> StatusCode {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "bokhylle.test");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    app.router
        .clone()
        .oneshot(builder.body(Body::from("{}")).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn mcp_requires_a_token_and_validates_origin() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("agent", "password123", Role::User)
        .await
        .unwrap();
    let (_, token) = bokhylle_server::agent_tokens::create(&app.state.db, user.id, "test", "read")
        .await
        .unwrap();

    assert_eq!(
        raw_post(&app, None, None).await,
        StatusCode::UNAUTHORIZED,
        "no token"
    );
    assert_eq!(
        raw_post(&app, Some("not-a-token"), None).await,
        StatusCode::UNAUTHORIZED,
        "unknown token"
    );
    assert_eq!(
        raw_post(&app, Some(&token), Some("https://evil.test")).await,
        StatusCode::FORBIDDEN,
        "a browser origin from another host is refused"
    );
    let (id, revoked) =
        bokhylle_server::agent_tokens::create(&app.state.db, user.id, "revoked", "read")
            .await
            .unwrap();
    bokhylle_server::agent_tokens::revoke(&app.state.db, user.id, id)
        .await
        .unwrap();
    assert_eq!(
        raw_post(&app, Some(&revoked), None).await,
        StatusCode::UNAUTHORIZED,
        "a revoked token stops working"
    );

    // Over a real connection the browser-origin rule can be exercised end to
    // end: a same-host origin connects, another host is refused.
    let (base, _app) = serve(app).await;
    let client = connect_with_origin(&base, &token, Some(&base))
        .await
        .expect("same-host origin");
    client.cancel().await.unwrap();
    assert!(
        connect_with_origin(&base, &token, Some("https://evil.test"))
            .await
            .is_err(),
        "a browser origin from another host is refused"
    );
}

#[tokio::test]
async fn tools_list_preserves_existing_schemas() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("agent", "password123", Role::User)
        .await
        .unwrap();
    let (_, token) = bokhylle_server::agent_tokens::create(&app.state.db, user.id, "test", "write")
        .await
        .unwrap();
    let (base, app) = serve(app).await;
    let client = connect(&base, &token).await;

    let tools = client.list_tools(Default::default()).await.unwrap();
    let names: Vec<&str> = tools.tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, TOOL_NAMES, "the advertised tool set changed");

    // Input schemas are part of the public API: snapshot each tool's fields.
    let mut schema: Vec<(String, Vec<String>)> = tools
        .tools
        .iter()
        .map(|tool| {
            let mut keys: Vec<String> = tool
                .input_schema
                .get("properties")
                .and_then(Value::as_object)
                .map(|properties| properties.keys().cloned().collect())
                .unwrap_or_default();
            keys.sort();
            (tool.name.to_string(), keys)
        })
        .collect();
    schema.sort();
    assert_eq!(
        schema,
        vec![
            (
                "add_book".to_string(),
                vec!["bookId".to_string(), "sendToReader".to_string()]
            ),
            (
                "add_catalogue_book".to_string(),
                vec![
                    "preferredFormat".to_string(),
                    "preferredLanguage".to_string(),
                    "provider".to_string(),
                    "providerKey".to_string(),
                    "sendToReader".to_string()
                ]
            ),
            ("continue_reading".to_string(), vec!["limit".to_string()]),
            ("get_book".to_string(), vec!["bookId".to_string()]),
            ("list_my_shelf".to_string(), vec!["limit".to_string()]),
            ("list_requests".to_string(), vec![]),
            (
                "request_book".to_string(),
                vec!["provider".to_string(), "providerKey".to_string()]
            ),
            (
                "search_books".to_string(),
                vec![
                    "limit".to_string(),
                    "query".to_string(),
                    "scope".to_string()
                ]
            ),
            (
                "send_to_reader".to_string(),
                vec!["bookId".to_string(), "targetId".to_string()]
            ),
        ]
    );

    client.cancel().await.unwrap();
    let _ = app;
}

#[tokio::test]
async fn scopes_and_profiles_decide_what_mcp_can_do() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "/works/OLMCPW".to_string(),
        title: "MCP Catalogue Book".to_string(),
        authors: vec!["MCP Author".to_string()],
        language: Some("en".to_string()),
        ..Default::default()
    }]));
    let app = common::test_app_with_metadata(provider).await;
    let adult = app
        .state
        .auth
        .create_user("adult", "password123", Role::User)
        .await
        .unwrap();
    let child = app
        .state
        .auth
        .create_user_with_profile("kid", "password123", Role::User, "password", "child")
        .await
        .unwrap();
    let admin = app
        .state
        .auth
        .create_user("root", "password123", Role::Admin)
        .await
        .unwrap();

    let owned = seed_book(&app, "/works/OLMCPOWN", "Owned Shelf Book").await;
    add_file(&app, owned, "mcp-owned").await;
    let household = seed_book(&app, "/works/OLMCPHH", "Household Only Book").await;
    add_file(&app, household, "mcp-household").await;
    bokhylle_server::user_books::add(&app.state.db, child.id, owned, "request")
        .await
        .unwrap();

    let (_, read_token) =
        bokhylle_server::agent_tokens::create(&app.state.db, adult.id, "read", "read")
            .await
            .unwrap();
    let (_, write_token) =
        bokhylle_server::agent_tokens::create(&app.state.db, adult.id, "write", "write")
            .await
            .unwrap();
    let (_, child_token) =
        bokhylle_server::agent_tokens::create(&app.state.db, child.id, "kid", "write")
            .await
            .unwrap();
    let (_, admin_token) =
        bokhylle_server::agent_tokens::create(&app.state.db, admin.id, "root", "write")
            .await
            .unwrap();
    let household_book = seed_book(&app, "/works/OLMCPNEW", "Fresh Request Book").await;
    let fresh = seed_book(&app, "/works/OLMCPFRESH", "Fresh Adult Book").await;

    let (base, app) = serve(app).await;

    // A read token can search, but not write.
    let reader = connect(&base, &read_token).await;
    let hits = call(&reader, "search_books", json!({ "query": "Owned" }))
        .await
        .unwrap();
    assert_eq!(hits["library"][0]["title"], "Owned Shelf Book");
    let refused = call(&reader, "add_book", json!({ "bookId": owned })).await;
    assert!(refused.is_err(), "read tokens cannot add books");
    assert!(
        call(
            &reader,
            "add_catalogue_book",
            json!({ "provider": "fake", "providerKey": "/works/OLMCPW" })
        )
        .await
        .is_err()
    );
    reader.cancel().await.unwrap();

    // A child sees only their shelf in the library scope, and adding creates
    // a request instead of an acquisition.
    let kid = connect(&base, &child_token).await;
    let shelf = call(
        &kid,
        "search_books",
        json!({ "query": "Book", "scope": "library" }),
    )
    .await
    .unwrap();
    let titles: Vec<&str> = shelf["library"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        vec!["Owned Shelf Book"],
        "child library is the shelf"
    );
    let catalogue = call(
        &kid,
        "search_books",
        json!({ "query": "MCP Catalogue", "scope": "catalogue" }),
    )
    .await
    .unwrap();
    assert_eq!(catalogue["catalogue"][0]["providerKey"], "/works/OLMCPW");
    assert!(
        catalogue["catalogue"][0].get("ownedBookId").is_none(),
        "the catalogue never leaks ownership: {catalogue}"
    );

    let outcome = call(&kid, "add_book", json!({ "bookId": household_book }))
        .await
        .unwrap();
    assert_eq!(outcome["phase"], "requested", "{outcome}");
    let acquisitions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM acquisitions WHERE book_id = ?")
            .bind(household_book)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(
        acquisitions, 0,
        "a child's add_book never starts an acquisition"
    );
    let requests: i64 =
        sqlx::query_scalar("SELECT count(*) FROM book_requests WHERE book_id = ? AND user_id = ?")
            .bind(household_book)
            .bind(child.id)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(requests, 1);
    assert!(
        call(
            &kid,
            "add_catalogue_book",
            json!({ "provider": "fake", "providerKey": "/works/OLMCPW" })
        )
        .await
        .is_err(),
        "children cannot start a catalogue acquisition"
    );
    kid.cancel().await.unwrap();

    // An adult write token reuses an owned copy instead of downloading again.
    let adult_client = connect(&base, &write_token).await;
    let outcome = call(&adult_client, "add_book", json!({ "bookId": owned }))
        .await
        .unwrap();
    assert_eq!(outcome["phase"], "ready", "{outcome}");
    let acquisitions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM acquisitions WHERE book_id = ?")
            .bind(owned)
            .fetch_one(&app.state.db)
            .await
            .unwrap();
    assert_eq!(acquisitions, 0, "the owned copy is reused");
    let on_shelf: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM user_books WHERE user_id = ? AND book_id = ? AND on_shelf = 1",
    )
    .bind(adult.id)
    .bind(owned)
    .fetch_one(&app.state.db)
    .await
    .unwrap();
    assert_eq!(on_shelf, 1, "the owned copy lands on the adult's shelf");

    // A book the household does not own starts a normal acquisition.
    let outcome = call(&adult_client, "add_book", json!({ "bookId": fresh }))
        .await
        .unwrap();
    assert_eq!(outcome["phase"], "looking", "{outcome}");
    assert!(outcome["acquisitionId"].is_string());
    let catalogue_outcome = call(
        &adult_client,
        "add_catalogue_book",
        json!({ "provider": "fake", "providerKey": "/works/OLMCPW" }),
    )
    .await
    .unwrap();
    assert!(catalogue_outcome["bookId"].is_i64(), "{catalogue_outcome}");
    assert!(catalogue_outcome["id"].is_string(), "{catalogue_outcome}");
    let catalogue_book: i64 = catalogue_outcome["bookId"].as_i64().unwrap();
    let requester: i64 = sqlx::query_scalar("SELECT user_id FROM acquisitions WHERE id = ?")
        .bind(catalogue_outcome["id"].as_str().unwrap())
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(
        requester, adult.id,
        "the acquisition keeps the agent profile"
    );
    let title: String = sqlx::query_scalar("SELECT title FROM books WHERE id = ?")
        .bind(catalogue_book)
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(title, "MCP Catalogue Book");
    adult_client.cancel().await.unwrap();

    // An admin profile is still just a profile: the token cannot see or call
    // any admin surface because none is exposed.
    let root = connect(&base, &admin_token).await;
    let tools = root.list_tools(Default::default()).await.unwrap();
    assert_eq!(tools.tools.len(), TOOL_NAMES.len());
    root.cancel().await.unwrap();
}

#[tokio::test]
async fn send_to_reader_uses_the_shared_delivery_service() {
    let app = common::test_app().await;
    let user = app
        .state
        .auth
        .create_user("reader", "password123", Role::User)
        .await
        .unwrap();
    let book = seed_book(&app, "/works/OLMCPSEND", "Send Me Book").await;
    add_file(&app, book, "mcp-send").await;
    bokhylle_server::user_books::add(&app.state.db, user.id, book, "manual")
        .await
        .unwrap();
    let (_, token) = bokhylle_server::agent_tokens::create(&app.state.db, user.id, "t", "write")
        .await
        .unwrap();

    let (base, app) = serve(app).await;
    let client = connect(&base, &token).await;

    // No reader is configured: the shared delivery service reports that
    // instead of pretending the send worked.
    let result = call(&client, "send_to_reader", json!({ "bookId": book })).await;
    let error = result.unwrap_err();
    assert!(
        error.contains("no delivery target"),
        "the error comes from the delivery service: {error}"
    );

    // With a reader configured but SMTP absent, the failure names SMTP, which
    // proves the tool delegated to the same send path as the HTTP API.
    bokhylle_server::delivery::create_target(
        &app.state.db,
        user.id,
        "Kindle",
        "reader@kindle.com",
        "email",
        Some("kindle"),
    )
    .await
    .unwrap();
    let failed = call(&client, "send_to_reader", json!({ "bookId": book }))
        .await
        .unwrap();
    assert_eq!(failed["status"], "FAILED", "{failed}");
    assert!(
        failed["errorMessage"]
            .as_str()
            .unwrap_or_default()
            .contains("SMTP is not configured"),
        "the shared send path ran: {failed}"
    );
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn acquisition_permission_applies_to_agent_tokens_too() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "/works/OLMCPASK".to_string(),
        title: "Ask for This Book".to_string(),
        authors: vec!["Request Author".to_string()],
        ..Default::default()
    }]));
    let app = common::test_app_with_metadata(provider).await;
    let user = app
        .state
        .auth
        .create_user("reader_only", "password123", Role::User)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET can_acquire = 0 WHERE id = ?")
        .bind(user.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let owned = seed_book(&app, "/works/OLMCPOWNED", "Shared Book").await;
    add_file(&app, owned, "mcp-shared").await;
    let missing = seed_book(&app, "/works/OLMCPMISSING", "Missing Book").await;
    let (_, token) = bokhylle_server::agent_tokens::create(&app.state.db, user.id, "t", "write")
        .await
        .unwrap();
    let (base, app) = serve(app).await;
    let client = connect(&base, &token).await;

    let owned_result = call(&client, "add_book", json!({ "bookId": owned }))
        .await
        .unwrap();
    assert_eq!(owned_result["phase"], "ready");
    assert!(
        call(&client, "add_book", json!({ "bookId": missing }))
            .await
            .is_err()
    );
    assert!(
        call(
            &client,
            "add_catalogue_book",
            json!({ "provider": "fake", "providerKey": "/works/OLMCPASK" }),
        )
        .await
        .is_err()
    );
    assert!(
        call(
            &client,
            "request_book",
            json!({ "provider": "fake", "providerKey": "/works/OLMCPASK" }),
        )
        .await
        .is_ok()
    );
    let acquisitions: i64 = sqlx::query_scalar("SELECT count(*) FROM acquisitions")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    assert_eq!(acquisitions, 0);
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn catalogue_search_honours_can_request() {
    let provider = Arc::new(FakeMetadataProvider::new(vec![MetadataResult {
        provider: "fake".to_string(),
        provider_key: "/works/OLMCPGATED".to_string(),
        title: "Gated Catalogue Book".to_string(),
        authors: vec!["MCP Author".to_string()],
        language: Some("en".to_string()),
        ..Default::default()
    }]));
    let app = common::test_app_with_metadata(provider).await;
    let child = app
        .state
        .auth
        .create_user_with_profile("gated", "password123", Role::User, "password", "child")
        .await
        .unwrap();
    let shelf_book = seed_book(&app, "/works/OLMCPGATEDSHELF", "Gated Shelf Book").await;
    add_file(&app, shelf_book, "mcp-gated-shelf").await;
    bokhylle_server::user_books::add(&app.state.db, child.id, shelf_book, "request")
        .await
        .unwrap();
    sqlx::query("UPDATE users SET can_request = 0 WHERE id = ?")
        .bind(child.id)
        .execute(&app.state.db)
        .await
        .unwrap();
    let (_, token) =
        bokhylle_server::agent_tokens::create(&app.state.db, child.id, "gated", "write")
            .await
            .unwrap();
    let (base, _app) = serve(app).await;
    let client = connect(&base, &token).await;

    // The shelf stays readable.
    let shelf = call(
        &client,
        "search_books",
        json!({ "query": "Gated", "scope": "library" }),
    )
    .await
    .unwrap();
    assert_eq!(shelf["library"][0]["title"], "Gated Shelf Book");

    // The catalogue is refused, and `auto` must not fall through to it.
    assert!(
        call(
            &client,
            "search_books",
            json!({ "query": "Gated", "scope": "catalogue" })
        )
        .await
        .is_err(),
        "the request catalogue is barred"
    );
    let auto = call(
        &client,
        "search_books",
        json!({ "query": "Gated Catalogue", "scope": "auto" }),
    )
    .await
    .unwrap();
    assert_eq!(
        auto["catalogue"].as_array().unwrap().len(),
        0,
        "auto must not reach the catalogue when requests are off: {auto}"
    );

    // And no write intent gets through either.
    assert!(
        call(
            &client,
            "request_book",
            json!({ "provider": "fake", "providerKey": "/works/OLMCPGATED" })
        )
        .await
        .is_err()
    );
    assert!(
        call(&client, "add_book", json!({ "bookId": shelf_book }))
            .await
            .is_err()
    );
    client.cancel().await.unwrap();
}
