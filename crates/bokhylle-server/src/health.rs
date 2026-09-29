//! Admin integration health: prove the acquisition infrastructure Bokhylle
//! depends on works before a download fails silently later.

use serde::Serialize;

use bokhylle_acquisition::qbittorrent::DEFAULT_CATEGORY;

use crate::AppState;
use crate::error::AppError;
use crate::settings;

#[derive(Serialize, schemars::JsonSchema)]
pub struct IntegrationStatus {
    prowlarr: ConnectorStatus,
    torznab: ConnectorStatus,
    newznab: ConnectorStatus,
    sabnzbd: ConnectorStatus,
    qbittorrent: DownloadStatus,
    library: LibraryStatus,
    hardlinks: HardlinkStatus,
    smtp: SmtpStatus,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct ConnectorStatus {
    configured: bool,
    ok: bool,
    version: Option<String>,
    error: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadStatus {
    configured: bool,
    ok: bool,
    version: Option<String>,
    error: Option<String>,
    category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    downloads_dir: Option<String>,
    completed: usize,
    missing_paths: usize,
    path_examples: Vec<PathExample>,
    path_checked: bool,
    path_message: Option<String>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct PathExample {
    name: String,
    path: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct LibraryStatus {
    path: String,
    exists: bool,
    writable: bool,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct HardlinkStatus {
    supported: bool,
    message: String,
}

#[derive(Serialize, schemars::JsonSchema)]
pub struct SmtpStatus {
    configured: bool,
}

impl DownloadStatus {
    fn unavailable(error: Option<String>, configured: bool) -> Self {
        Self {
            configured,
            ok: false,
            version: None,
            error,
            category: DEFAULT_CATEGORY.to_string(),
            downloads_dir: None,
            completed: 0,
            missing_paths: 0,
            path_examples: Vec::new(),
            path_checked: false,
            path_message: None,
        }
    }
}

/// Probe names are unique per run and created exclusively, so two concurrent
/// checks cannot interfere and Bokhylle never touches a pre-existing file.
fn probe_path(root: &std::path::Path, prefix: &str) -> std::path::PathBuf {
    root.join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()))
}

pub async fn integrations(state: &AppState) -> Result<IntegrationStatus, AppError> {
    let prowlarr = prowlarr_check(state).await;
    let torznab = indexer_check(state, "torznab").await;
    let newznab = indexer_check(state, "newznab").await;
    let sabnzbd = sabnzbd_check(state).await;
    let qbittorrent = qbittorrent_check(state).await;
    let probe_state = state.clone();
    let (library, hardlinks) = tokio::task::spawn_blocking(move || {
        (library_check(&probe_state), hardlink_check(&probe_state))
    })
    .await
    .map_err(|error| AppError::Unavailable(format!("filesystem checks failed: {error}")))?;
    let smtp_configured = state
        .settings
        .get_string(settings::SMTP_HOST, "")
        .await
        .map(|host| !host.trim().is_empty())
        .unwrap_or(false);

    Ok(IntegrationStatus {
        prowlarr,
        torznab,
        newznab,
        sabnzbd,
        qbittorrent,
        library,
        hardlinks,
        smtp: SmtpStatus {
            configured: smtp_configured,
        },
    })
}

async fn prowlarr_check(state: &AppState) -> ConnectorStatus {
    indexer_check(state, "prowlarr").await
}

async fn sabnzbd_check(state: &AppState) -> ConnectorStatus {
    let Some(factory) = state.providers.as_ref() else {
        return ConnectorStatus {
            configured: false,
            ok: false,
            version: None,
            error: Some("integrations are not available".into()),
        };
    };
    match factory.nzb_downloader(state).await {
        Ok(None) => ConnectorStatus {
            configured: false,
            ok: false,
            version: None,
            error: None,
        },
        Ok(Some(client)) => match client.test_connection().await {
            Ok(version) => ConnectorStatus {
                configured: true,
                ok: true,
                version: Some(version),
                error: None,
            },
            Err(error) => ConnectorStatus {
                configured: true,
                ok: false,
                version: None,
                error: Some(error.to_string()),
            },
        },
        Err(error) => ConnectorStatus {
            configured: false,
            ok: false,
            version: None,
            error: Some(error.to_string()),
        },
    }
}

async fn indexer_check(state: &AppState, kind: &str) -> ConnectorStatus {
    let Some(factory) = state.providers.as_ref() else {
        return ConnectorStatus {
            configured: false,
            ok: false,
            version: None,
            error: Some("integrations are not available".to_string()),
        };
    };
    match factory.indexer_named(state, kind).await {
        Ok(None) => ConnectorStatus {
            configured: false,
            ok: false,
            version: None,
            error: None,
        },
        Ok(Some(indexer)) => match indexer.test_connection().await {
            Ok(version) => ConnectorStatus {
                configured: true,
                ok: true,
                version: Some(version),
                error: None,
            },
            Err(error) => ConnectorStatus {
                configured: true,
                ok: false,
                version: None,
                error: Some(error.to_string()),
            },
        },
        Err(error) => ConnectorStatus {
            configured: false,
            ok: false,
            version: None,
            error: Some(error.to_string()),
        },
    }
}

async fn qbittorrent_check(state: &AppState) -> DownloadStatus {
    let Some(factory) = state.providers.as_ref() else {
        return DownloadStatus::unavailable(
            Some("integrations are not available".to_string()),
            false,
        );
    };
    let downloader = match factory.downloader(state).await {
        Ok(Some(downloader)) => downloader,
        Ok(None) => return DownloadStatus::unavailable(None, false),
        Err(error) => return DownloadStatus::unavailable(Some(error.to_string()), false),
    };

    let version = match downloader.test_connection().await {
        Ok(version) => Some(version),
        Err(error) => {
            return DownloadStatus::unavailable(Some(error.to_string()), true);
        }
    };

    let category = state
        .settings
        .get_string(settings::QBITTORRENT_CATEGORY, DEFAULT_CATEGORY)
        .await
        .unwrap_or_else(|_| DEFAULT_CATEGORY.to_string());
    let category = match category.trim() {
        "" => DEFAULT_CATEGORY.to_string(),
        value => value.to_string(),
    };

    let mut completed = 0usize;
    let mut missing_count = 0usize;
    let mut missing: Vec<PathExample> = Vec::new();
    match downloader.list_category(&category).await {
        Ok(torrents) => {
            for torrent in torrents.iter().filter(|torrent| torrent.progress >= 1.0) {
                completed += 1;
                let path = torrent.content_path.clone().unwrap_or_else(|| {
                    std::path::Path::new(&torrent.save_path)
                        .join(&torrent.name)
                        .to_string_lossy()
                        .into_owned()
                });
                if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
                    missing_count += 1;
                    if missing.len() < 3 {
                        missing.push(PathExample {
                            name: torrent.name.clone(),
                            path,
                        });
                    }
                }
            }
        }
        Err(error) => {
            return DownloadStatus {
                configured: true,
                ok: true,
                version,
                error: None,
                category,
                downloads_dir: Some(state.paths.downloads_dir.to_string_lossy().into_owned()),
                completed: 0,
                missing_paths: 0,
                path_examples: Vec::new(),
                path_checked: false,
                path_message: Some(format!("Could not list downloads: {error}")),
            };
        }
    }

    let path_message = if completed == 0 {
        "No completed downloads to verify yet.".to_string()
    } else if missing_count == 0 {
        format!("All {completed} completed download paths are accessible.")
    } else {
        format!(
            "Bokhylle cannot access {missing_count} of {completed} completed download paths — check the container volume mapping."
        )
    };

    DownloadStatus {
        configured: true,
        ok: true,
        version,
        error: None,
        category,
        downloads_dir: Some(state.paths.downloads_dir.to_string_lossy().into_owned()),
        completed,
        missing_paths: missing_count,
        path_examples: missing,
        path_checked: true,
        path_message: Some(path_message),
    }
}

fn library_check(state: &AppState) -> LibraryStatus {
    let root = &state.paths.library_root;
    LibraryStatus {
        path: root.to_string_lossy().into_owned(),
        exists: root.is_dir(),
        writable: writable_probe(root),
    }
}

fn writable_probe(root: &std::path::Path) -> bool {
    let probe = probe_path(root, ".bokhylle-write-test");
    let writable = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .and_then(|mut file| std::io::Write::write_all(&mut file, b"bokhylle"))
        .is_ok();
    let _ = std::fs::remove_file(&probe);
    writable
}

fn hardlink_check(state: &AppState) -> HardlinkStatus {
    let name = format!(".bokhylle-hardlink-test-{}", uuid::Uuid::new_v4().simple());
    let source = state.paths.downloads_dir.join(&name);
    let target = state.paths.library_root.join(&name);
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source)
        .map_err(|error| error.to_string())
        .and_then(|_| std::fs::hard_link(&source, &target).map_err(|error| error.to_string()));
    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&target);

    match result {
        Ok(()) => HardlinkStatus {
            supported: true,
            message: "Downloads and library are on the same filesystem.".to_string(),
        },
        Err(error) => HardlinkStatus {
            supported: false,
            message: format!(
                "Hardlinks unavailable ({error}); Bokhylle will fall back to copying files."
            ),
        },
    }
}
