use crate::error::Result;
use crate::github::models::{Activity, CommitInfo};
use chrono::NaiveDate;
use octocrab::Octocrab;
use std::sync::Arc;

pub struct GitHubClient {
    client: Arc<Octocrab>,
    username: String,
}

impl GitHubClient {
    pub fn new(token: &str, username: &str) -> Result<Self> {
        let client = Octocrab::builder()
            .personal_token(token.to_string())
            .build()
            .map_err(|e| crate::error::RecapError::GitHubApi(e))?;

        Ok(Self {
            client: Arc::new(client),
            username: username.to_string(),
        })
    }

    pub fn client(&self) -> &Octocrab {
        &self.client
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub async fn search_commits_for_date(&self, date: NaiveDate) -> Result<Vec<CommitInfo>> {
        super::commits::search_commits_for_date(&self.client, &self.username, date).await
    }

    pub async fn enrich_commits_with_prs(&self, commits: Vec<CommitInfo>) -> Result<Vec<Activity>> {
        super::pulls::enrich_commits_with_prs(&self.client, commits).await
    }

    pub async fn get_activities_for_date(&self, date: NaiveDate) -> Result<Vec<Activity>> {
        let commits = self.search_commits_for_date(date).await?;
        self.enrich_commits_with_prs(commits).await
    }
}
