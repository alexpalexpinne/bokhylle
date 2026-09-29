use serde::{Deserialize, Serialize};

pub mod identity;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    Epub,
    Pdf,
    Cbz,
}

impl BookFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Cbz => "cbz",
        }
    }

    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "epub" => Some(Self::Epub),
            "pdf" => Some(Self::Pdf),
            "cbz" => Some(Self::Cbz),
            _ => None,
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        Self::from_extension(value)
    }
}
