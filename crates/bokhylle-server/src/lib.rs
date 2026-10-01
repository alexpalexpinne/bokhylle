pub mod acquisition;
pub mod acquisition_pipeline;
pub mod acquisition_requests;
pub mod acquisition_tracker;
pub mod adopt;
pub mod agent_tokens;
pub mod artifact;
pub mod auth;
pub mod backup;
pub mod book_requests;
pub mod collections;
pub mod db;
pub mod delivery;
pub mod demo;
pub mod discovery;
pub mod error;
pub mod external_ids;
pub mod follows;
pub mod health;
pub mod http_acquisition;
pub mod import_pipeline;
pub mod keep_looking;
pub mod library;
pub mod maintenance;
mod mcp;
pub mod metadata_registry;
mod middleware;
pub mod notifications;
pub mod nzb_acquisition;
pub mod observability;
pub mod opds_catalog;
pub mod openapi;
pub mod paths;
pub mod providers;
pub mod reader_tokens;
pub mod remote_http;
mod routes;
pub use routes::image_cache::prune as prune_image_cache;
pub mod server;
pub mod services;
pub mod settings;
pub mod updates;
pub mod user_books;
pub mod watch_folder;

pub use routes::kosync::partial_md5;

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use bokhylle_metadata::MetadataProvider;
use sqlx::SqlitePool;
use tower_http::services::{ServeDir, ServeFile};

use crate::acquisition_pipeline::PipelineGuard;
use crate::auth::Auth;
use crate::library::ScanState;
use crate::paths::Paths;
use crate::providers::ProviderFactory;
use crate::settings::Settings;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub settings: Settings,
    pub paths: Arc<Paths>,
    pub auth: Auth,
    pub scan_state: Arc<ScanState>,
    pub metadata: Arc<dyn MetadataProvider>,
    pub metadata_fallback: Option<Arc<dyn MetadataProvider>>,
    pub ratings: Arc<dyn MetadataProvider>,
    pub registry: Arc<crate::metadata_registry::MetadataRegistry>,
    pub providers: Option<Arc<dyn ProviderFactory>>,
    pub pipeline: Arc<PipelineGuard>,
    pub imports: Arc<PipelineGuard>,
    pub demo: Option<Arc<demo::DemoState>>,
    pub server: Arc<server::ServerRuntime>,
}

pub fn app(state: AppState) -> Router {
    let web_root = state.paths.web_root.clone();

    let mut router = routes::registry::router().merge(mcp::router(state.clone()));

    router = if web_root.is_dir() {
        router
            .route_service("/assets/{*path}", ServeDir::new(&web_root))
            .fallback_service(
                ServeDir::new(&web_root).fallback(ServeFile::new(web_root.join("index.html"))),
            )
    } else {
        tracing::warn!(
            path = %web_root.display(),
            "web root not found; serving API only"
        );
        router.fallback(routes::fallback)
    };

    let mut openapi = openapi_base();
    let router = router.finish_api(&mut openapi);
    let document = Arc::new(complete_openapi(openapi));
    router
        .route(
            "/openapi.json",
            get(move || {
                let document = document.clone();
                async move { axum::Json((*document).clone()) }
            }),
        )
        .layer(axum::middleware::from_fn(middleware::cache_headers))
        .layer(axum::middleware::from_fn(middleware::origin_guard))
        .layer(axum::middleware::from_fn(middleware::security_headers))
        .layer(axum::middleware::from_fn({
            let guard_state = state.clone();
            move |request, next| {
                let state = guard_state.clone();
                async move { middleware::child_guard(&state, request, next).await }
            }
        }))
        .layer(axum::middleware::from_fn({
            let guard_state = state.clone();
            move |request, next| {
                let state = guard_state.clone();
                async move { demo::guard(&state, request, next).await }
            }
        }))
        .layer(axum::middleware::from_fn(middleware::request_id))
        .with_state(state)
}

fn openapi_base() -> aide::openapi::OpenApi {
    aide::openapi::OpenApi {
        info: aide::openapi::Info {
            title: "Bokhylle HTTP API".to_string(),
            version: bokhylle_core::VERSION.to_string(),
            description: Some(
                "Household library JSON API. Access depends on the signed-in profile and role."
                    .to_string(),
            ),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn openapi_document() -> serde_json::Value {
    let mut document = openapi_base();
    let _ = routes::registry::router().finish_api(&mut document);
    complete_openapi(document)
}

fn complete_openapi(document: aide::openapi::OpenApi) -> serde_json::Value {
    openapi::complete(document)
}
