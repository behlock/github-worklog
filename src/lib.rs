pub mod cli;
pub mod config;
pub mod error;
pub mod github;
pub mod recap;
pub mod summarizer;

pub use config::Settings;
pub use error::{RecapError, Result};
pub use github::GitHubClient;
pub use recap::{append_to_file, RecapGenerator};
pub use summarizer::{summarize, Provider};
