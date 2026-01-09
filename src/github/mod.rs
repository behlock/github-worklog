mod client;
mod commits;
mod models;
mod pulls;

pub use client::GitHubClient;
pub use models::{Activity, CommitInfo, PullRequestInfo, RepoInfo};
