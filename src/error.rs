use thiserror::Error;

#[derive(Error, Debug)]
pub enum RecapError {
    #[error("Missing required configuration: {0}")]
    MissingConfig(String),

    #[error("Invalid GitHub token: authentication failed")]
    InvalidToken,

    #[error("GitHub API rate limit exceeded. Retry after {retry_after} seconds")]
    RateLimitExceeded { retry_after: u64 },

    #[error("GitHub API error (HTTP {status}): {message}")]
    GitHubApi { status: u16, message: String },

    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("File operation error on '{path}': {source}")]
    FileOp {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

impl RecapError {
    /// Process exit status. `2` marks errors that will not go away by
    /// retrying (configuration, credentials), matching clap's usage errors;
    /// `1` is everything else.
    pub fn exit_code(&self) -> u8 {
        match self {
            RecapError::MissingConfig(_) | RecapError::InvalidToken => 2,
            _ => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, RecapError>;
