use std::path::PathBuf;
use std::sync::Arc;

use bokhylle_server::auth::{Auth, Role};
use bokhylle_server::paths::Paths;
use bokhylle_server::settings::Settings;
use bokhylle_server::{AppState, app, db};

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        std::process::exit(healthcheck());
    }
    if std::env::args().nth(1).as_deref() == Some("--openapi") {
        println!(
            "{}",
            serde_json::to_string_pretty(&bokhylle_server::openapi_document())
                .expect("OpenAPI JSON")
        );
        return;
    }

    bokhylle_server::observability::init();

    let addr = std::env::var("BOKHYLLE_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let config_dir =
        std::env::var("BOKHYLLE_CONFIG_DIR").unwrap_or_else(|_| "./config".to_string());
    let config_dir = PathBuf::from(config_dir);

    let demo_mode =
        std::env::var("BOKHYLLE_DEMO_MODE").is_ok_and(|value| value.eq_ignore_ascii_case("true"));
    if demo_mode && !config_dir.join(".bokhylle-demo").is_file() {
        panic!("refusing unsafe demo installation: demo marker is missing");
    }

    let pool = db::init(&config_dir.join("bokhylle.db"))
        .await
        .unwrap_or_else(|error| panic!("failed to initialize database: {error}"));
    if demo_mode {
        bokhylle_server::demo::validate_installation(&pool, &config_dir)
            .await
            .unwrap_or_else(|error| panic!("refusing unsafe demo installation: {error}"));
    }

    let settings = Settings::new(pool.clone());
    let paths = Paths::resolve(&settings, config_dir)
        .await
        .unwrap_or_else(|error| panic!("invalid configuration: {error}"));

    tracing::info!(
        library_root = %paths.library_root.display(),
        downloads_dir = %paths.downloads_dir.display(),
        "configuration loaded"
    );

    let auth = Auth::new(pool.clone());
    if !demo_mode {
        bootstrap_admin(&auth).await;
    }
    if let Err(error) = auth.delete_expired_sessions().await {
        tracing::warn!(%error, "failed to clean up expired sessions");
    }

    let scan_state = Arc::new(bokhylle_server::library::ScanState::default());

    let google_books_key = settings
        .get_string(bokhylle_server::settings::GOOGLE_BOOKS_API_KEY, "")
        .await
        .ok()
        .filter(|value| !value.is_empty());
    let metadata_choice = settings
        .get_string(bokhylle_server::settings::METADATA_PROVIDER, "automatic")
        .await
        .unwrap_or_else(|_| "openlibrary".to_string());
    let metadata: Arc<dyn bokhylle_metadata::MetadataProvider> =
        if metadata_choice == "google_books" {
            Arc::new(
                bokhylle_metadata::google_books::GoogleBooksClient::new(google_books_key.clone())
                    .unwrap_or_else(|error| {
                        panic!("failed to initialize the metadata provider: {error}")
                    }),
            )
        } else {
            Arc::new(
                bokhylle_metadata::open_library::OpenLibraryClient::new().unwrap_or_else(|error| {
                    panic!("failed to initialize the metadata provider: {error}")
                }),
            )
        };

    // "Only" modes are truthful: the fallback exists solely for Automatic,
    // which is Open Library primary with Google filling gaps.
    let metadata_fallback: Option<Arc<dyn bokhylle_metadata::MetadataProvider>> =
        if metadata_choice == "automatic" {
            Some(Arc::new(
                bokhylle_metadata::google_books::GoogleBooksClient::new(google_books_key.clone())
                    .unwrap_or_else(|error| {
                        panic!("failed to initialize the fallback provider: {error}")
                    }),
            ))
        } else {
            None
        };

    let ratings_choice = settings
        .get_string(
            bokhylle_server::settings::RATINGS_PROVIDER,
            "same_as_metadata",
        )
        .await
        .unwrap_or_else(|_| "same_as_metadata".to_string());
    let ratings: Arc<dyn bokhylle_metadata::MetadataProvider> = match ratings_choice.as_str() {
        "disabled" => Arc::new(bokhylle_metadata::disabled::DisabledProvider),
        "openlibrary" => Arc::new(
            bokhylle_metadata::open_library::OpenLibraryClient::new().unwrap_or_else(|error| {
                panic!("failed to initialize the ratings provider: {error}")
            }),
        ),
        "google_books" => Arc::new(
            bokhylle_metadata::google_books::GoogleBooksClient::new(google_books_key)
                .unwrap_or_else(|error| {
                    panic!("failed to initialize the ratings provider: {error}")
                }),
        ),
        _ => metadata.clone(),
    };

    let registry = Arc::new(bokhylle_server::metadata_registry::MetadataRegistry::new(
        metadata.clone(),
        metadata_fallback.clone(),
        ratings.clone(),
    ));

    let server = Arc::new(
        bokhylle_server::server::ServerRuntime::capture(&settings, &paths)
            .await
            .expect("running server configuration"),
    );
    let state = AppState {
        db: pool,
        settings,
        paths: Arc::new(paths),
        auth,
        scan_state,
        metadata,
        metadata_fallback,
        ratings,
        registry,
        providers: (!demo_mode).then(|| {
            Arc::new(bokhylle_server::providers::SettingsProviderFactory)
                as Arc<dyn bokhylle_server::providers::ProviderFactory>
        }),
        pipeline: Arc::new(bokhylle_server::acquisition_pipeline::PipelineGuard::default()),
        imports: Arc::new(bokhylle_server::acquisition_pipeline::PipelineGuard::default()),
        demo: demo_mode.then(|| Arc::new(bokhylle_server::demo::DemoState::default())),
        server,
    };

    match bokhylle_server::db::prune_metadata_cache(&state.db).await {
        Ok(pruned) if pruned > 0 => tracing::info!(pruned, "metadata_cache.pruned"),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "metadata_cache.prune_failed"),
    }
    {
        let config_dir = state.paths.config_dir.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = bokhylle_server::prune_image_cache(&config_dir).await {
                    tracing::warn!(%error, "image_cache.prune_failed");
                }
                tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)).await;
            }
        });
    }
    if !demo_mode {
        bokhylle_server::acquisition_pipeline::recover(&state).await;
        bokhylle_server::acquisition_tracker::spawn(state.clone());
        bokhylle_server::nzb_acquisition::spawn(state.clone());
        bokhylle_server::keep_looking::spawn_scheduler(state.clone());
        bokhylle_server::backup::spawn_scheduler(&state);
        bokhylle_server::server::releases::spawn_scheduler(&state);
        bokhylle_server::import_pipeline::recover(&state).await;
        if let Err(error) = bokhylle_server::watch_folder::recover(&state).await {
            tracing::warn!(%error, "watch_folder.recovery_failed");
        }
        bokhylle_server::watch_folder::spawn(state.clone());
        if let Err(error) = bokhylle_server::delivery::recover(&state).await {
            tracing::error!(%error, "delivery recovery failed");
        }

        let refresher = state.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) =
                    bokhylle_server::updates::refresh_followed_authors(&refresher).await
                {
                    tracing::warn!(%error, "followed author refresh failed");
                }
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        });
    }
    if demo_mode {
        let demo_state = state.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = bokhylle_server::demo::tick(&demo_state).await {
                    tracing::warn!(%error, "demo activity tick failed");
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }

    {
        let scheduler = state.clone();
        tokio::spawn(async move {
            let mut last_run: Option<std::time::Instant> = None;
            loop {
                let hours = scheduler
                    .settings
                    .get_float(bokhylle_server::settings::SCAN_INTERVAL_HOURS, 0.0)
                    .await
                    .unwrap_or(0.0);

                if hours > 0.0 {
                    let due = last_run
                        .map(|last| last.elapsed().as_secs_f64() >= hours * 3600.0)
                        .unwrap_or(true);
                    if due && bokhylle_server::library::scan_state::start_scan(&scheduler) {
                        tracing::info!(interval_hours = hours, "library.scan.scheduled");
                        last_run = Some(std::time::Instant::now());
                    }
                }

                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            }
        });
    }

    if state
        .settings
        .get_bool(bokhylle_server::settings::SCAN_ON_STARTUP, true)
        .await
        .unwrap_or(true)
        && bokhylle_server::library::scan_state::start_scan(&state)
    {
        tracing::info!("library scan started on startup");
    }

    let router = app(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|error| panic!("failed to bind {addr}: {error}"));

    tracing::info!(%addr, version = bokhylle_core::VERSION, "bokhylle server listening");

    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("server failed");
}

async fn bootstrap_admin(auth: &Auth) {
    let user_count = match auth.count_users().await {
        Ok(count) => count,
        Err(error) => {
            tracing::error!(%error, "failed to count users");
            return;
        }
    };

    if user_count > 0 {
        return;
    }

    let Ok(password) = std::env::var("BOKHYLLE_ADMIN_PASSWORD") else {
        tracing::warn!(
            "no users exist; set BOKHYLLE_ADMIN_USERNAME and BOKHYLLE_ADMIN_PASSWORD to create the initial admin"
        );
        return;
    };

    if !bokhylle_server::auth::bootstrap_password_acceptable(&password) {
        tracing::error!(
            "BOKHYLLE_ADMIN_PASSWORD is missing, shorter than 8 characters, or a well-known default; refusing to create the initial admin"
        );
        return;
    }

    let username = std::env::var("BOKHYLLE_ADMIN_USERNAME").unwrap_or_else(|_| "admin".to_string());

    match auth.create_user(&username, &password, Role::Admin).await {
        Ok(_) => tracing::info!(%username, "created initial admin user"),
        Err(error) => tracing::error!(%error, "failed to create initial admin user"),
    }
}

async fn shutdown_signal() {
    // Containers stop with SIGTERM, not Ctrl-C; both must drain gracefully.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(terminate) => terminate,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                tracing::info!("shutdown signal received");
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("shutdown signal received");
}

fn healthcheck() -> i32 {
    use std::io::{Read, Write};

    let bind = std::env::var("BOKHYLLE_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let target = if bind.starts_with("0.0.0.0") {
        bind.replacen("0.0.0.0", "127.0.0.1", 1)
    } else {
        bind
    };

    let Ok(mut stream) = std::net::TcpStream::connect(&target) else {
        return 1;
    };

    if stream
        .write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .is_err()
    {
        return 1;
    }

    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return 1;
    }

    i32::from(!response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200"))
}
