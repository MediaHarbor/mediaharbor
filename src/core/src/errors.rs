use thiserror::Error;

#[derive(Debug, Error)]
pub enum MhError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Archive error: {0}")]
    Archive(#[from] zip::result::ZipError),

    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    #[error("Background task failed: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Subprocess error: {0}")]
    Subprocess(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unsupported: {0}")]
    Unsupported(String),

    /// The stream opened but nothing here can decode it. Kept structured so the
    /// player can pass the details on rather than a pre-rendered paragraph.
    #[error("Can't decode: {}", .0.detail)]
    Undecodable(Box<crate::ipc_contract::UndecodableStream>),

    #[error("{0}")]
    Forbidden(String),

    #[error("{0}")]
    RateLimited(String),

    #[error("Cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

pub type MhResult<T> = Result<T, MhError>;

impl From<serde_json::Error> for MhError {
    fn from(e: serde_json::Error) -> Self {
        MhError::Parse(e.to_string())
    }
}

impl From<std::string::FromUtf8Error> for MhError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        MhError::Parse(e.to_string())
    }
}

impl From<url::ParseError> for MhError {
    fn from(e: url::ParseError) -> Self {
        MhError::Parse(e.to_string())
    }
}

impl From<regex::Error> for MhError {
    fn from(e: regex::Error) -> Self {
        MhError::Parse(e.to_string())
    }
}

impl From<reqwest::header::InvalidHeaderName> for MhError {
    fn from(e: reqwest::header::InvalidHeaderName) -> Self {
        MhError::Other(e.to_string())
    }
}

impl From<reqwest::header::InvalidHeaderValue> for MhError {
    fn from(e: reqwest::header::InvalidHeaderValue) -> Self {
        MhError::Other(e.to_string())
    }
}
