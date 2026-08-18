use serde::ser::SerializeStruct;
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

    #[error("pressbooks error: {0}")]
    Pressbooks(String),

    #[error("model error: {0}")]
    Model(String),

    #[error("voice error: {0}")]
    Voice(String),

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("tts error: {0}")]
    Tts(String),

    #[error("DRM-protected content cannot be imported")]
    DrmProtected,

    #[error("tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("payment required: {0}")]
    PaymentRequired(String),

    #[error("rate limited: {0}")]
    RateLimited(String),
}

impl AppError {
    /// Stable identifier for this error, mirrored by `AppErrorKind` in
    /// `src/types/domain.ts`. The match is exhaustive on purpose: adding a
    /// variant will not compile until this is updated, and
    /// `scripts/ci/check-error-kinds.sh` fails if the TypeScript union drifts.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Database(_) => "database",
            Self::Pool(_) => "pool",
            Self::Io(_) => "io",
            Self::Serde(_) => "serde",
            Self::Http(_) => "http",
            Self::Readability(_) => "readability",
            Self::Epub(_) => "epub",
            Self::Pdf(_) => "pdf",
            Self::OpenStax(_) => "openstax",
            Self::LibreTexts(_) => "libretexts",
            Self::Pressbooks(_) => "pressbooks",
            Self::Model(_) => "model",
            Self::Voice(_) => "voice",
            Self::Auth(_) => "auth",
            Self::Tts(_) => "tts",
            Self::DrmProtected => "drm_protected",
            Self::Tauri(_) => "tauri",
            Self::InvalidInput(_) => "invalid_input",
            Self::Migration(_) => "migration",
            Self::PaymentRequired(_) => "payment_required",
            Self::RateLimited(_) => "rate_limited",
        }
    }

    /// The human-readable part, without the kind prefix that `Display` adds.
    /// `Display` keeps the prefix because it is useful in Rust logs; callers
    /// across the invoke boundary already have `kind` and do not need it twice.
    pub fn message(&self) -> String {
        match self {
            Self::Database(error) => error.to_string(),
            Self::Pool(error) => error.to_string(),
            Self::Io(error) => error.to_string(),
            Self::Serde(error) => error.to_string(),
            Self::Http(error) => error.to_string(),
            Self::Readability(error) => error.to_string(),
            Self::Epub(error) => error.to_string(),
            Self::Tauri(error) => error.to_string(),
            Self::Pdf(message)
            | Self::OpenStax(message)
            | Self::LibreTexts(message)
            | Self::Pressbooks(message)
            | Self::Model(message)
            | Self::Voice(message)
            | Self::Auth(message)
            | Self::Tts(message)
            | Self::InvalidInput(message)
            | Self::Migration(message)
            | Self::PaymentRequired(message)
            | Self::RateLimited(message) => message.clone(),
            Self::DrmProtected => self.to_string(),
        }
    }

    /// Whether retrying the same operation could plausibly succeed. Exhaustive
    /// so that a new variant forces a deliberate answer rather than inheriting
    /// a default.
    pub fn retryable(&self) -> bool {
        match self {
            // A timed-out or unestablished connection is worth another go; a
            // 404 or a malformed body is not.
            Self::Http(error) => error.is_timeout() || error.is_connect(),
            Self::Io(_) | Self::Pool(_) | Self::RateLimited(_) => true,
            Self::Database(_)
            | Self::Serde(_)
            | Self::Readability(_)
            | Self::Epub(_)
            | Self::Pdf(_)
            | Self::OpenStax(_)
            | Self::LibreTexts(_)
            | Self::Pressbooks(_)
            | Self::Model(_)
            | Self::Voice(_)
            | Self::Auth(_)
            | Self::Tts(_)
            | Self::DrmProtected
            | Self::Tauri(_)
            | Self::InvalidInput(_)
            | Self::Migration(_)
            | Self::PaymentRequired(_) => false,
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("AppError", 3)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.message())?;
        state.serialize_field("retryable", &self.retryable())?;
        state.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl From<pdfium_render::prelude::PdfiumError> for AppError {
    fn from(error: pdfium_render::prelude::PdfiumError) -> Self {
        Self::Pdf(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_kind_message_and_retryable() {
        let value = serde_json::to_value(AppError::InvalidInput("text is empty".into()))
            .expect("AppError should serialize");

        assert_eq!(value["kind"], "invalid_input");
        assert_eq!(value["message"], "text is empty");
        assert_eq!(value["retryable"], false);
    }

    #[test]
    fn message_drops_the_prefix_that_display_keeps() {
        let error = AppError::Model("checksum mismatch".into());

        // Display stays useful for Rust logs; the wire form does not repeat the
        // kind inside the message.
        assert_eq!(error.to_string(), "model error: checksum mismatch");
        assert_eq!(error.message(), "checksum mismatch");
    }

    #[test]
    fn drm_protected_carries_its_whole_sentence() {
        // This variant has no `{0}` payload, so `message` falls back to the
        // Display string. It is also the variant the old prefix-stripping
        // frontend could never match, because it contains no colon.
        let error = AppError::DrmProtected;

        assert_eq!(error.kind(), "drm_protected");
        assert_eq!(error.message(), "DRM-protected content cannot be imported");
        assert!(!error.retryable());
    }

    #[test]
    fn transient_io_is_retryable() {
        let error = AppError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "read timed out",
        ));

        assert_eq!(error.kind(), "io");
        assert!(error.retryable());
    }
}
