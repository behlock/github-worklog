use crate::error::{RecapError, Result};
use crate::github::client::GitHubClient;
use crate::github::models::{Activity, CommitInfo, PullRequestInfo};
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt, TryStreamExt};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::warn;

/// GitHub's secondary rate limits are sensitive to concurrent requests; keep this low.
const PR_FETCH_CONCURRENCY: usize = 5;

#[derive(Debug, Deserialize)]
struct PullRequestResponse {
    number: u64,
    title: String,
    state: String,
    html_url: String,
    merged_at: Option<DateTime<Utc>>,
}

/// Look up the PR for every commit, a few at a time.
///
/// An invalid token aborts immediately. Hitting the rate limit stops all
/// further lookups but is not fatal: the remaining commits are recorded
/// without a PR so the day's recap is still written rather than lost until
/// someone re-runs it by hand.
pub async fn enrich_commits_with_prs(
    client: &GitHubClient,
    commits: Vec<CommitInfo>,
) -> Result<Vec<Activity>> {
    let rate_limited = AtomicBool::new(false);
    let rate_limited = &rate_limited;

    stream::iter(commits)
        .map(|commit| {
            Ok(async move {
                let associated_pr = if rate_limited.load(Ordering::Relaxed) {
                    None
                } else {
                    match get_pr_for_commit(client, &commit).await {
                        Ok(pr) => pr,
                        Err(RecapError::RateLimitExceeded { retry_after }) => {
                            if !rate_limited.swap(true, Ordering::Relaxed) {
                                warn!(
                                    "GitHub rate limit reached (resets in {retry_after}s); \
                                     recording the remaining commits without PR details"
                                );
                                eprintln!(
                                    "GitHub rate limit reached; some commits will lack PR details."
                                );
                            }
                            None
                        }
                        Err(e) => return Err(e),
                    }
                };
                Ok(Activity {
                    commit,
                    associated_pr,
                })
            })
        })
        .try_buffered(PR_FETCH_CONCURRENCY)
        .try_collect()
        .await
}

/// The pull request a commit belongs to, preferring a merged one.
///
/// Per-commit lookup failures degrade to "no PR" so a single flaky repo does
/// not lose the whole day. Auth and rate-limit failures are passed up for
/// the caller to decide.
async fn get_pr_for_commit(
    client: &GitHubClient,
    commit: &CommitInfo,
) -> Result<Option<PullRequestInfo>> {
    let path = format!(
        "/repos/{}/{}/commits/{}/pulls",
        commit.repository.owner, commit.repository.name, commit.sha
    );

    match client
        .get_json::<Vec<PullRequestResponse>>(&path, &[])
        .await
    {
        Ok(prs) => Ok(pick_pr(prs)),
        Err(e @ (RecapError::InvalidToken | RecapError::RateLimitExceeded { .. })) => Err(e),
        Err(e) => {
            warn!(
                "Failed to fetch PR for {} commit {}: {e}",
                commit.repository.full_name(),
                commit.sha
            );
            Ok(None)
        }
    }
}

fn pick_pr(prs: Vec<PullRequestResponse>) -> Option<PullRequestInfo> {
    let merged_idx = prs.iter().position(|pr| pr.merged_at.is_some());
    let pr = match merged_idx {
        Some(i) => prs.into_iter().nth(i)?,
        None => prs.into_iter().next()?,
    };
    Some(PullRequestInfo {
        number: pr.number,
        title: pr.title,
        state: pr.state,
        html_url: pr.html_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(number: u64, merged: bool) -> PullRequestResponse {
        PullRequestResponse {
            number,
            title: format!("PR {number}"),
            state: if merged { "closed" } else { "open" }.to_string(),
            html_url: format!("https://github.com/acme/api/pull/{number}"),
            merged_at: merged.then(Utc::now),
        }
    }

    #[test]
    fn prefers_merged_pr() {
        let picked = pick_pr(vec![pr(1, false), pr(2, true), pr(3, true)]).unwrap();
        assert_eq!(picked.number, 2);
    }

    #[test]
    fn falls_back_to_first_pr() {
        let picked = pick_pr(vec![pr(7, false), pr(8, false)]).unwrap();
        assert_eq!(picked.number, 7);
    }

    #[test]
    fn no_prs_yields_none() {
        assert!(pick_pr(vec![]).is_none());
    }
}
