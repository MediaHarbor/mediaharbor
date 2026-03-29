use thiserror::Error;

#[derive(Debug, Error)]
pub enum MhError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

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

impl From<anyhow::Error> for MhError {
    fn from(e: anyhow::Error) -> Self {
        MhError::Other(e.to_string())
    }
}
