use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use bokhylle_acquisition::provider::{DownloadProvider, IndexerProvider};
use bokhylle_metadata::MetadataProvider;
use bokhylle_metadata::testing::FakeMetadataProvider;
use bokhylle_server::auth::Auth;
use bokhylle_server::paths::Paths;
use bokhylle_server::providers::{ProviderFactory, StaticProviderFactory};
use bokhylle_server::settings::Settings;
use bokhylle_server::{AppState, app, db};
use tempfile::TempDir;
use tower::ServiceExt;

pub struct TestApp {
    pub router: Router,
    #[allow(dead_code)]
    pub state: AppState,
    pub _temp_dir: TempDir,
}

#[derive(Default)]
struct BuildOptions {
    config_root: Option<PathBuf>,
    web_root: Option<PathBuf>,
    library_root: Option<PathBuf>,
    downloads_root: Option<PathBuf>,
    metadata: Option<Arc<dyn MetadataProvider>>,
    fallback: Option<Arc<dyn MetadataProvider>>,
    ratings: Option<Arc<dyn MetadataProvider>>,
    providers: Option<Arc<dyn ProviderFactory>>,
}

#[allow(dead_code)]
pub async fn test_app() -> TestApp {
    build(BuildOptions::default()).await
}

#[allow(dead_code)]
pub async fn test_app_with_web_root(web_root: PathBuf) -> TestApp {
    build(BuildOptions {
        web_root: Some(web_root),
        ..Default::default()
    })
    .await
}

#[allow(dead_code)]
pub async fn test_app_with_library_root(library_root: PathBuf) -> TestApp {
    build(BuildOptions {
        library_root: Some(library_root),
        ..Default::default()
    })
    .await
}

#[allow(dead_code)]
pub async fn test_app_from_existing_config(config_root: PathBuf, library_root: PathBuf) -> TestApp {
    build(BuildOptions {
        config_root: Some(config_root),
        library_root: Some(library_root),
        ..Default::default()
    })
    .await
}

#[allow(dead_code)]
pub async fn test_app_with_provider_matrix(
    metadata: Arc<dyn MetadataProvider>,
    fallback: Option<Arc<dyn MetadataProvider>>,
    ratings: Arc<dyn MetadataProvider>,
) -> TestApp {
    build(BuildOptions {
        metadata: Some(metadata),
        fallback,
        ratings: Some(ratings),
        ..Default::default()
    })
    .await
}

#[allow(dead_code)]
pub async fn test_app_with_metadata(metadata: Arc<dyn MetadataProvider>) -> TestApp {
    build(BuildOptions {
        metadata: Some(metadata),
        ..Default::default()
    })
    .await
}

#[allow(dead_code)]
pub async fn test_app_with_library_and_metadata(
    library_root: PathBuf,
    metadata: Arc<dyn MetadataProvider>,
) -> TestApp {
    build(BuildOptions {
        library_root: Some(library_root),
        metadata: Some(metadata),
        ..Default::default()
    })
    .await
}

#[allow(dead_code)]
pub async fn test_app_with_providers(
    library_root: PathBuf,
    indexer: Arc<dyn IndexerProvider>,
    downloader: Arc<dyn DownloadProvider>,
) -> TestApp {
    build(BuildOptions {
        library_root: Some(library_root),
        providers: Some(Arc::new(StaticProviderFactory {
            indexer: Some(indexer),
            downloader: Some(downloader),
        })),
        ..Default::default()
    })
    .await
}

#[allow(dead_code)]
pub async fn test_app_with_factory(
    library_root: PathBuf,
    providers: Arc<dyn ProviderFactory>,
) -> TestApp {
    build(BuildOptions {
        library_root: Some(library_root),
        providers: Some(providers),
        ..Default::default()
    })
    .await
}

#[allow(dead_code)]
pub async fn test_app_full(
    library_root: PathBuf,
    metadata: Arc<dyn MetadataProvider>,
    indexer: Arc<dyn IndexerProvider>,
    downloader: Arc<dyn DownloadProvider>,
) -> TestApp {
    build(BuildOptions {
        library_root: Some(library_root),
        metadata: Some(metadata),
        providers: Some(Arc::new(StaticProviderFactory {
            indexer: Some(indexer),
            downloader: Some(downloader),
        })),
        ..Default::default()
    })
    .await
}

/// Like `test_app_full`, but the downloads directory is the caller's (so
/// tests can point it at the content they expect to import).
#[allow(dead_code)]
pub async fn test_app_full_with_downloads(
    library_root: PathBuf,
    downloads_root: PathBuf,
    metadata: Arc<dyn MetadataProvider>,
    indexer: Arc<dyn IndexerProvider>,
    downloader: Arc<dyn DownloadProvider>,
) -> TestApp {
    build(BuildOptions {
        library_root: Some(library_root),
        downloads_root: Some(downloads_root),
        metadata: Some(metadata),
        providers: Some(Arc::new(StaticProviderFactory {
            indexer: Some(indexer),
            downloader: Some(downloader),
        })),
        ..Default::default()
    })
    .await
}

async fn build(options: BuildOptions) -> TestApp {
    let temp_dir = tempfile::tempdir().expect("temporary directory");
    let config_dir = options
        .config_root
        .clone()
        .unwrap_or_else(|| temp_dir.path().join("config"));
    let pool = db::init(&config_dir.join("bokhylle.db"))
        .await
        .expect("database init");
    let settings = Settings::new(pool.clone());
    let mut paths = Paths::resolve(&settings, config_dir)
        .await
        .expect("path resolution");
    if let Some(web_root) = options.web_root {
        paths.web_root = web_root;
    }
    if let Some(library_root) = options.library_root {
        paths.library_root = library_root;
    }
    if let Some(downloads_root) = options.downloads_root {
        paths.downloads_dir = downloads_root;
    }

    let metadata = options
        .metadata
        .unwrap_or_else(|| Arc::new(FakeMetadataProvider::default()));
    let ratings = options.ratings.unwrap_or_else(|| metadata.clone());
    let registry = Arc::new(bokhylle_server::metadata_registry::MetadataRegistry::new(
        metadata.clone(),
        options.fallback.clone(),
        ratings.clone(),
    ));

    let state = AppState {
        db: pool.clone(),
        settings,
        paths: Arc::new(paths),
        auth: Auth::new(pool),
        scan_state: Arc::new(bokhylle_server::library::ScanState::default()),
        ratings,
        metadata_fallback: options.fallback,
        registry,
        metadata,
        providers: options.providers,
        pipeline: Arc::new(bokhylle_server::acquisition_pipeline::PipelineGuard::default()),
        imports: Arc::new(bokhylle_server::acquisition_pipeline::PipelineGuard::default()),
        demo: None,
    };

    TestApp {
        router: app(state.clone()),
        state,
        _temp_dir: temp_dir,
    }
}

#[allow(dead_code)]
pub async fn login(test_app: &TestApp, username: &str, password: &str) -> String {
    let body = serde_json::json!({ "username": username, "password": password });

    let response = test_app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK, "login failed");

    response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}
