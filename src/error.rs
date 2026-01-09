use thiserror::Error;

#[derive(Error, Debug)]
pub enum RecapError {
    #[error("GitHub API error: {0}")]
    GitHubApi(#[from] octocrab::Error),

    #[error("GitHub rate limit exceeded. Retry after {retry_after} seconds")]
    RateLimitExceeded { retry_after: u64 },

    #[error("Missing required configuration: {0}")]
    MissingConfig(String),

    #[error("File operation error on '{path}': {source}")]
    FileOp {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Date parsing error: expected YYYY-MM-DD format, got '{input}'")]
    DateParse { input: String },

    #[error("No commits found for {username} on {date}")]
    NoCommitsFound { username: String, date: String },

    #[error("Invalid GitHub token: authentication failed")]
    InvalidToken,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RecapError>;
