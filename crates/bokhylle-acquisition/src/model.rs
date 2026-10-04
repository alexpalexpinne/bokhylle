use serde::{Deserialize, Serialize};

/// How a candidate's bytes are obtained. Missing on historical event rows,
/// which describe torrent releases before this field existed.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AcquisitionMethod {
    Torrent {
        magnet_url: Option<String>,
        download_url: Option<String>,
    },
    Http {
        url: String,
    },
    Nzb {
        guid: String,
    },
}

impl AcquisitionMethod {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Torrent { .. } => "torrent",
            Self::Http { .. } => "http",
            Self::Nzb { .. } => "nzb",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceIdentity {
    pub kind: String,
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone)]
pub struct ExpectedBook {
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    /// A set of acceptable languages from the reader's profile; when set it
    /// supersedes `language` for release matching.
    pub languages: Vec<String>,
    pub preferred_format: Option<String>,
    pub series_number: Option<String>,
}

impl Default for ExpectedBook {
    fn default() -> Self {
        Self {
            title: String::new(),
            authors: Vec::new(),
            year: None,
            isbn: None,
            language: None,
            languages: Vec::new(),
            preferred_format: Some("epub".to_string()),
            series_number: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCandidate {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<AcquisitionMethod>,
    pub title: String,
    pub indexer: Option<String>,
    pub size_bytes: i64,
    pub seeders: Option<i64>,
    pub leechers: Option<i64>,
    pub download_url: Option<String>,
    pub magnet_url: Option<String>,
    pub info_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_volume: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_collection: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_audiobook: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_comic: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    LanguageMismatch,
    Audiobook,
    ComicOrManga,
    UnsupportedFormat,
    UnrelatedTitle,
    AuthorMismatch,
    WrongVolume,
    OversizedRelease,
}

impl RejectionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LanguageMismatch => "explicit_language_mismatch",
            Self::Audiobook => "audiobook_not_requested",
            Self::ComicOrManga => "comic_or_manga",
            Self::UnsupportedFormat => "unsupported_format",
            Self::UnrelatedTitle => "unrelated_title",
            Self::AuthorMismatch => "author_mismatch",
            Self::WrongVolume => "wrong_volume",
            Self::OversizedRelease => "oversized_release",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScoreReason {
    pub weight: i32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluatedRelease {
    pub candidate: ReleaseCandidate,
    pub score: i32,
    pub confidence: f32,
    /// Format preference comes first (EPUB before the PDF fallback), then
    /// the reader's ordered language preference. Defaulted because candidate
    /// lists are persisted in events and outlive struct changes.
    #[serde(default)]
    pub format_tier: u8,
    #[serde(default)]
    pub language_index: usize,
    pub score_reasons: Vec<ScoreReason>,
    pub rejection_reasons: Vec<RejectionReason>,
}

impl EvaluatedRelease {
    pub fn rejected(&self) -> bool {
        !self.rejection_reasons.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Auto { index: usize },
    NeedsSelection,
    None,
}

impl Selection {
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Auto { .. } => "SELECTED",
            Self::NeedsSelection => "NEEDS_SELECTION",
            Self::None => "NO_RELEASE_FOUND",
        }
    }
}
