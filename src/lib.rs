pub mod cli;
pub mod config;
pub mod error;
pub mod github;
pub mod http;
pub mod recap;
pub mod summarizer;

pub use config::Settings;
pub use error::{RecapError, Result};
pub use github::GitHubClient;
pub use recap::{HeaderMatcher, RecapGenerator, WriteResult, prepend_to_file};
pub use summarizer::{Provider, summarize};
