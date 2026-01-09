use crate::error::{RecapError, Result};
use crate::github::models::{CommitInfo, RepoInfo};
use chrono::{DateTime, NaiveDate, Utc};
use octocrab::Octocrab;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SearchCommitsResponse {
    total_count: u32,
    items: Vec<SearchCommitItem>,
}

#[derive(Debug, Deserialize)]
struct SearchCommitItem {
    sha: String,
    html_url: String,
    commit: CommitData,
    repository: RepoData,
}

#[derive(Debug, Deserialize)]
struct CommitData {
    message: String,
    author: CommitAuthor,
}

#[derive(Debug, Deserialize)]
struct CommitAuthor {
    name: String,
    date: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct RepoData {
    full_name: String,
}

pub async fn search_commits_for_date(
    client: &Octocrab,
    username: &str,
    date: NaiveDate,
) -> Result<Vec<CommitInfo>> {
    let date_str = date.format("%Y-%m-%d").to_string();
    let query = format!("author:{} author-date:{}", username, date_str);

    let mut all_commits = Vec::new();
    let mut page = 1u32;

    loop {
        let url = format!(
            "https://api.github.com/search/commits?q={}&sort=author-date&order=desc&per_page=100&page={}",
            urlencoding::encode(&query),
            page
        );

        let response: SearchCommitsResponse = client
            .get(&url, None::<&()>)
            .await
            .map_err(RecapError::GitHubApi)?;

        let items_count = response.items.len();

        for item in response.items {
            let (owner, name) = parse_repo_full_name(&item.repository.full_name);

            all_commits.push(CommitInfo {
                sha: item.sha,
                message: item.commit.message,
                author: item.commit.author.name,
                date: item.commit.author.date,
                repository: RepoInfo { owner, name },
                html_url: item.html_url,
            });
        }

        if items_count < 100 || all_commits.len() >= response.total_count as usize {
            break;
        }
        page += 1;

        // Avoid hitting rate limits too fast
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    Ok(all_commits)
}

fn parse_repo_full_name(full_name: &str) -> (String, String) {
    let parts: Vec<&str> = full_name.splitn(2, '/').collect();
    if parts.len() == 2 {
        (parts[0].to_string(), parts[1].to_string())
    } else {
        ("unknown".to_string(), full_name.to_string())
    }
}
