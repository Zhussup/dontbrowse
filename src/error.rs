use thiserror::Error;

/// Application-level error with enough classification for actionable UI messages.
#[derive(Debug, Error, Clone)]
pub enum AppError {
    #[error("network error: {0}")]
    Network(String),
    #[error("yt-dlp is not installed")]
    YtDlpMissing,
    #[error("mpv is not installed")]
    MpvMissing,
    #[error("could not extract article: {0}")]
    Extraction(String),
    #[error("cookies required for {0}")]
    CookiesRequired(String),
    #[error("paywall blocked: {0}")]
    PaywallBlocked(String),
    #[error("unsupported content: {0}")]
    Unsupported(String),
    #[error("{0}")]
    Tool(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        AppError::Other(value.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Other(value.to_string())
    }
}
