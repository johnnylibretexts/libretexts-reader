use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("article extraction error: {0}")]
    Readability(#[from] readability::error::Error),

    #[error("epub error: {0}")]
    Epub(#[from] epub::doc::DocError),

    #[error("pdf error: {0}")]
    Pdf(String),

    #[error("openstax error: {0}")]
    OpenStax(String),

    #[error("libretexts error: {0}")]
    LibreTexts(String),

    #[error("model error: {0}")]
    Model(String),

    #[error("voice error: {0}")]
    Voice(String),

    #[error("tts error: {0}")]
    Tts(String),

    #[error("DRM-protected content cannot be imported")]
    DrmProtected,

    #[error("tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl From<pdfium_render::prelude::PdfiumError> for AppError {
    fn from(error: pdfium_render::prelude::PdfiumError) -> Self {
        Self::Pdf(error.to_string())
    }
}
