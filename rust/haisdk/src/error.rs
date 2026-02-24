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

    #[error("{0}")]
    Message(String),
}
