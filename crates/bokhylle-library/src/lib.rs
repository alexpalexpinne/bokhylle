pub mod covers;
pub mod error;
pub mod extract;
pub mod fixtures;
pub mod scan;

pub use error::LibraryError;
pub use extract::{Cover, Extracted, ExtractedMetadata};
pub use scan::{ScannedFile, scan};
