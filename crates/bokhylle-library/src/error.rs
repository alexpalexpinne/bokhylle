use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("pdf error: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("invalid ebook {path}: {reason}")]
    Invalid { path: PathBuf, reason: String },
}
