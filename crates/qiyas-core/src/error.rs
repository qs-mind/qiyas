use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("This drawing can't be opened: {0}")]
    Unreadable(String),
    /// The CAD writer could not write the drawing; its own words.
    #[error("{0}")]
    Unwritable(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("No sheet named {0}")]
    NoSuchSheet(String),
    #[error("No entity with handle {0:X}")]
    NoSuchEntity(u64),
    #[error("{0}")]
    InvalidCommand(String),
    /// A command of a batch failed: which one (from 0), what it was, and why.
    #[error("Batch step {index} ({label}): {error}")]
    Step {
        index: usize,
        label: String,
        error: Box<CoreError>,
    },
    /// The project file is not valid JSON for a project; the parser's own words.
    #[error("{0}")]
    Project(String),
    /// A PDF could not be read, or PDF support is not installed (`qiyas_pdf::PdfError`).
    #[error("{0}")]
    Pdf(String),
    /// A raster image could not be read; says which and why.
    #[error("{0}")]
    Image(String),
}

impl From<qiyas_pdf::PdfError> for CoreError {
    fn from(e: qiyas_pdf::PdfError) -> Self {
        CoreError::Pdf(e.to_string())
    }
}
