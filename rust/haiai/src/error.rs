use thiserror::Error;

pub type Result<T> = std::result::Result<T, HaiError>;

#[derive(Debug, Error)]
pub enum HaiError {
    #[error("JACS config not found at {path}")]
    ConfigNotFound { path: String },

    #[error("invalid JACS config: {message}")]
    ConfigInvalid { message: String },

    #[error("jacsId is required for authenticated operations")]
    MissingJacsId,

    #[error("JACS provider error: {0}")]
    Provider(String),

    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HAI API error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("verify URL would exceed max length ({max_len})")]
    VerifyUrlTooLong { max_len: usize },

    #[error("verify link hosted mode requires jacsDocumentId, document_id, or id")]
    MissingHostedDocumentId,

    #[error("validation error on '{field}': {message}")]
    Validation { field: String, message: String },

    /// Issue 052: typed signal that a backend doesn't support a particular
    /// trait method (e.g., `RemoteJacsProvider::query_by_field` cannot run
    /// against a server whose envelope JSON lives in S3 rather than Postgres,
    /// per PRD §10 Non-Goal #19). Cross-language consumers can match on this
    /// without string-matching the error message; routes can fall back to a
    /// supported method (e.g., `search_documents`) programmatically.
    #[error("backend does not support method '{method}': {detail}")]
    BackendUnsupported { method: String, detail: String },

    #[error("{0}")]
    Message(String),
}
