use std::path::PathBuf;

use crate::{AppState, acquisition::Acquisition};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactOwner {
    DownloadClient,
    Bokhylle,
}

/// The importer's shared handoff from any retrieval method. The resolved
/// path must exist inside the configured downloads tree. Ownership governs
/// cleanup; qBittorrent's source stays intact unless explicitly configured.
#[derive(Debug, Clone)]
pub struct StagedArtifact {
    pub path: PathBuf,
    pub owner: ArtifactOwner,
    pub expected_format: Option<String>,
}

impl StagedArtifact {
    pub fn from_acquisition(state: &AppState, acquisition: &Acquisition) -> Option<Self> {
        let path = PathBuf::from(acquisition.content_path.as_deref()?);
        if !crate::paths::is_within(&state.paths.downloads_dir, &path) {
            return None;
        }
        let owner = if acquisition.download_provider.as_deref() == Some("http") {
            if !crate::http_acquisition::owned_file(state, &acquisition.id, &path) {
                return None;
            }
            ArtifactOwner::Bokhylle
        } else {
            ArtifactOwner::DownloadClient
        };
        Some(Self {
            path,
            owner,
            expected_format: acquisition.selected_release_format.clone(),
        })
    }
}
