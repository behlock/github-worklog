use crate::error::Result;
use crate::github::models::{Activity, CommitInfo, PullRequestInfo};
use octocrab::Octocrab;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PullRequestResponse {
    number: u64,
    title: String,
    state: String,
    html_url: String,
}

pub async fn enrich_commits_with_prs(
    client: &Octocrab,
    commits: Vec<CommitInfo>,
) -> Result<Vec<Activity>> {
    let mut activities = Vec::new();

    for commit in commits {
        let pr = get_pr_for_commit(
            client,
            &commit.repository.owner,
            &commit.repository.name,
            &commit.sha,
        )
        .await?;

        activities.push(Activity {
            commit,
            associated_pr: pr,
        });

        // Small delay to avoid rate limits
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }

    Ok(activities)
}

async fn get_pr_for_commit(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    sha: &str,
) -> Result<Option<PullRequestInfo>> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/commits/{}/pulls",
        owner, repo, sha
    );

    let response: std::result::Result<Vec<PullRequestResponse>, _> =
        client.get(&url, None::<&()>).await;

    match response {
        Ok(prs) => {
            if let Some(pr) = prs.into_iter().next() {
                Ok(Some(PullRequestInfo {
                    number: pr.number,
                    title: pr.title,
                    state: pr.state,
                    html_url: pr.html_url,
                }))
            } else {
                Ok(None)
            }
        }
        Err(_) => {
            // If we can't fetch PR info, just return None (not an error)
            Ok(None)
        }
    }
}
