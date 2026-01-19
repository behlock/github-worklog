pub mod cli;
pub mod config;
pub mod error;
pub mod github;
pub mod recap;
pub mod summarizer;

pub use config::Settings;
pub use error::{RecapError, Result};
pub use github::GitHubClient;
#[allow(deprecated)]
pub use recap::append_to_file;
pub use recap::{prepend_to_file, RecapGenerator, WriteResult};
pub use summarizer::{summarize, Provider};
