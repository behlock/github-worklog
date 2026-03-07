use crate::error::Result;
use crate::github::models::{Activity, CommitInfo, PullRequestInfo};
use futures::stream::{self, StreamExt};
use octocrab::Octocrab;
use serde::Deserialize;

const PR_FETCH_CONCURRENCY: usize = 5;

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
    let activities: Vec<Activity> = stream::iter(commits)
        .map(|commit| async {
            let pr = get_pr_for_commit(
                client,
                &commit.repository.owner,
                &commit.repository.name,
                &commit.sha,
            )
            .await?;

            Ok(Activity {
                commit,
                associated_pr: pr,
            })
        })
        .buffer_unordered(PR_FETCH_CONCURRENCY)
        .collect::<Vec<Result<Activity>>>()
        .await
        .into_iter()
        .collect::<Result<Vec<Activity>>>()?;

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
        Err(e) => {
            tracing::warn!(
                "Failed to fetch PR for {}/{} commit {}: {}",
                owner,
                repo,
                sha,
                e
            );
            Ok(None)
        }
    }
}
