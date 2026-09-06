use crate::error::Result;
use crate::github::client::GitHubClient;
use crate::github::models::{CommitInfo, RepoInfo};
use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use serde::Deserialize;
use tracing::{debug, warn};

const PER_PAGE: usize = 100;
/// The search API never returns more than 1000 results; requesting past that
/// yields a 422.
const MAX_PAGES: u32 = 10;
/// How many times to re-request a page GitHub flagged as incomplete.
const MAX_INCOMPLETE_RETRIES: u32 = 2;

#[derive(Debug, Deserialize)]
struct SearchCommitsResponse {
    total_count: u64,
    items: Vec<SearchCommitItem>,
    /// Set when GitHub's search index timed out and returned a partial page.
    #[serde(default)]
    incomplete_results: bool,
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

/// Commits authored by the client's user during `date` in the local timezone.
///
/// GitHub's `author-date:YYYY-MM-DD` qualifier only has day granularity and
/// is evaluated in UTC, so a commit made at 23:30 BST is indexed under the
/// following day. To get "my day" semantics we search the surrounding three
/// UTC days (any local day lies within them, since offsets never reach 24h)
/// and keep only commits whose author time falls on `date` locally.
pub async fn search_commits_for_date(
    client: &GitHubClient,
    date: NaiveDate,
) -> Result<Vec<CommitInfo>> {
    let query = build_query(client.username(), date);
    debug!("Commit search query: {query}");

    let mut all_commits = Vec::new();
    let mut seen_total = 0usize;
    let mut page = 1u32;
    let mut incomplete_retries = 0u32;

    while page <= MAX_PAGES {
        let params = [
            ("q", query.clone()),
            ("sort", "author-date".to_string()),
            ("order", "desc".to_string()),
            ("per_page", PER_PAGE.to_string()),
            ("page", page.to_string()),
        ];
        let response: SearchCommitsResponse = client.get_json("/search/commits", &params).await?;

        // A partial page from an index timeout would otherwise look like the
        // last page and silently end pagination early. GitHub also sets the
        // flag on pages that are clearly complete, so only re-request when
        // the page is short of what total_count promises.
        let short_of_total = response.items.len() < PER_PAGE
            && (seen_total + response.items.len()) < response.total_count as usize;
        if response.incomplete_results
            && short_of_total
            && incomplete_retries < MAX_INCOMPLETE_RETRIES
        {
            incomplete_retries += 1;
            warn!(
                "GitHub returned an incomplete search page {page}; re-requesting ({incomplete_retries}/{MAX_INCOMPLETE_RETRIES})"
            );
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }
        if response.incomplete_results && short_of_total {
            warn!(
                "GitHub search page {page} is still incomplete after retries; results may be missing"
            );
        }

        let fetched = response.items.len();
        seen_total += fetched;
        all_commits.extend(
            response
                .items
                .into_iter()
                .map(CommitInfo::from)
                .filter(|c| is_on_local_date(c, date)),
        );

        debug!(
            "Fetched page {page}: {fetched} items ({seen_total} of {} in window, {} on {date})",
            response.total_count,
            all_commits.len()
        );
        if fetched < PER_PAGE || seen_total as u64 >= response.total_count {
            break;
        }
        if page == MAX_PAGES {
            warn!(
                "GitHub search returned only the first {seen_total} of {} commits in the window \
                 around {date}; some commits from that day may be missing from the recap",
                response.total_count
            );
        }
        page += 1;
    }

    Ok(all_commits)
}

fn is_on_local_date(commit: &CommitInfo, date: NaiveDate) -> bool {
    commit.date.with_timezone(&Local).date_naive() == date
}

/// `author:<user> author-date:<date-1>..<date+1>` (inclusive on both ends).
fn build_query(username: &str, date: NaiveDate) -> String {
    let start = date.checked_sub_days(Days::new(1)).unwrap_or(date);
    let end = date.checked_add_days(Days::new(1)).unwrap_or(date);
    format!(
        "author:{username} author-date:{}..{}",
        start.format("%Y-%m-%d"),
        end.format("%Y-%m-%d")
    )
}

impl From<SearchCommitItem> for CommitInfo {
    fn from(item: SearchCommitItem) -> Self {
        let (owner, name) = parse_repo_full_name(&item.repository.full_name);
        CommitInfo {
            sha: item.sha,
            message: item.commit.message,
            author: item.commit.author.name,
            date: item.commit.author.date,
            repository: RepoInfo { owner, name },
            html_url: item.html_url,
        }
    }
}

fn parse_repo_full_name(full_name: &str) -> (String, String) {
    match full_name.split_once('/') {
        Some((owner, name)) => (owner.to_string(), name.to_string()),
        None => ("unknown".to_string(), full_name.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn query_spans_surrounding_days() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 20).unwrap();
        assert_eq!(
            build_query("octocat", date),
            "author:octocat author-date:2026-03-19..2026-03-21"
        );
    }

    #[test]
    fn query_handles_month_and_year_boundaries() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert_eq!(
            build_query("octocat", date),
            "author:octocat author-date:2025-12-31..2026-01-02"
        );
    }

    #[test]
    fn local_date_filter_uses_local_timezone() {
        let mut commit = CommitInfo::from(sample_item());
        // Whatever the local offset, a commit at local noon is on its own date.
        let local_noon = Local
            .with_ymd_and_hms(2026, 3, 20, 12, 0, 0)
            .single()
            .unwrap();
        commit.date = local_noon.with_timezone(&Utc);
        assert!(is_on_local_date(&commit, local_noon.date_naive()));
        assert!(!is_on_local_date(
            &commit,
            local_noon.date_naive().succ_opt().unwrap()
        ));
    }

    #[test]
    fn parses_search_response_into_commit_info() {
        let commit = CommitInfo::from(sample_item());
        assert_eq!(commit.sha, "abc123");
        assert_eq!(commit.subject(), "fix auth bug");
        assert_eq!(commit.repository.owner, "acme");
        assert_eq!(commit.repository.name, "api");
        assert_eq!(commit.repository.full_name(), "acme/api");
        assert_eq!(
            commit.date,
            Utc.with_ymd_and_hms(2026, 3, 20, 10, 15, 0).unwrap()
        );
    }

    #[test]
    fn repo_name_without_slash_is_marked_unknown() {
        assert_eq!(
            parse_repo_full_name("weird"),
            ("unknown".to_string(), "weird".to_string())
        );
    }

    fn sample_item() -> SearchCommitItem {
        let json = r#"{
          "total_count": 1,
          "items": [{
            "sha": "abc123",
            "html_url": "https://github.com/acme/api/commit/abc123",
            "commit": {
              "message": "fix auth bug\n\nlonger description",
              "author": {"name": "Octo Cat", "date": "2026-03-20T10:15:00Z"}
            },
            "repository": {"full_name": "acme/api"}
          }]
        }"#;
        let parsed: SearchCommitsResponse = serde_json::from_str(json).unwrap();
        parsed.items.into_iter().next().unwrap()
    }
}
