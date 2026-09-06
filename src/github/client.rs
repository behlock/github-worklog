use crate::error::{RecapError, Result};
use crate::github::models::{Activity, CommitInfo};
use crate::http::{self, MAX_ATTEMPTS, MAX_RETRY_AFTER, USER_AGENT};
use chrono::{NaiveDate, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::{Client, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;
use tracing::{debug, warn};

const API_BASE: &str = "https://api.github.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct GitHubClient {
    http: Client,
    username: String,
}

impl std::fmt::Debug for GitHubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHubClient")
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

impl GitHubClient {
    pub fn new(token: &str, username: &str) -> Result<Self> {
        let mut auth = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| RecapError::InvalidToken)?;
        auth.set_sensitive(true);

        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth);
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static("2022-11-28"),
        );

        let http = Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .timeout(REQUEST_TIMEOUT)
            .build()?;

        Ok(Self {
            http,
            username: username.to_string(),
        })
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    /// All commits authored by the user on `date` (in the local timezone),
    /// each enriched with its associated pull request where one exists.
    pub async fn get_activities_for_date(&self, date: NaiveDate) -> Result<Vec<Activity>> {
        let commits = self.search_commits_for_date(date).await?;
        self.enrich_commits_with_prs(commits).await
    }

    pub async fn search_commits_for_date(&self, date: NaiveDate) -> Result<Vec<CommitInfo>> {
        super::commits::search_commits_for_date(self, date).await
    }

    pub async fn enrich_commits_with_prs(&self, commits: Vec<CommitInfo>) -> Result<Vec<Activity>> {
        super::pulls::enrich_commits_with_prs(self, commits).await
    }

    /// GET a JSON endpoint under the API base, with retries for transient
    /// failures and translation of GitHub's auth and rate-limit responses.
    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let url = format!("{API_BASE}{path}");
        let mut attempt = 0u32;

        loop {
            attempt += 1;
            let can_retry = attempt < MAX_ATTEMPTS;

            let response = match self.http.get(&url).query(query).send().await {
                Ok(r) => r,
                Err(e) if can_retry && http::is_transient(&e) => {
                    warn!("GitHub request to {path} failed ({e}), retrying");
                    backoff(attempt).await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };

            let status = response.status();
            if status.is_success() {
                return Ok(response.json().await?);
            }
            if status == StatusCode::UNAUTHORIZED {
                return Err(RecapError::InvalidToken);
            }

            let throttled =
                status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS;
            if throttled {
                if let Some(reset_in) = primary_rate_limit_reset(&response) {
                    // Primary limit exhausted. Wait it out if the reset is
                    // close; otherwise report how long the caller must wait.
                    let wait = Duration::from_secs(reset_in.max(1));
                    if can_retry && wait <= MAX_RETRY_AFTER {
                        warn!(
                            "GitHub rate limit reached; waiting {}s for it to reset before retrying {path}",
                            wait.as_secs()
                        );
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    return Err(RecapError::RateLimitExceeded {
                        retry_after: reset_in,
                    });
                }
            }
            let retry_after = http::retry_after(&response);
            let message = error_message(response.text().await.unwrap_or_default());

            // Secondary (abuse) limits come back as 403/429, sometimes with a
            // Retry-After header and sometimes only with a message asking us
            // to wait "a few minutes".
            if throttled && (retry_after.is_some() || is_secondary_rate_limit(&message)) {
                let wait = retry_after.unwrap_or(MAX_RETRY_AFTER);
                if can_retry && wait <= MAX_RETRY_AFTER {
                    warn!(
                        "GitHub asked us to slow down; waiting {}s before retrying {path}",
                        wait.as_secs()
                    );
                    tokio::time::sleep(wait).await;
                    continue;
                }
                return Err(RecapError::RateLimitExceeded {
                    retry_after: wait.as_secs(),
                });
            }

            if status.is_server_error() && can_retry {
                warn!("GitHub returned {status} for {path}, retrying");
                backoff(attempt).await;
                continue;
            }

            return Err(RecapError::GitHubApi {
                status: status.as_u16(),
                message,
            });
        }
    }
}

async fn backoff(attempt: u32) {
    let delay = http::backoff_delay(attempt);
    debug!("Backing off for {}s", delay.as_secs());
    tokio::time::sleep(delay).await;
}

fn header_u64(response: &Response, name: &str) -> Option<u64> {
    response
        .headers()
        .get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// When the primary rate limit is exhausted GitHub sets `x-ratelimit-remaining: 0`
/// and `x-ratelimit-reset` to the epoch second at which it resets.
fn primary_rate_limit_reset(response: &Response) -> Option<u64> {
    if header_u64(response, "x-ratelimit-remaining")? != 0 {
        return None;
    }
    let reset = header_u64(response, "x-ratelimit-reset")? as i64;
    Some((reset - Utc::now().timestamp()).max(0) as u64)
}

fn is_secondary_rate_limit(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("secondary rate limit") || m.contains("abuse detection")
}

/// GitHub error bodies are `{"message": "...", ...}`; fall back to the raw text.
fn error_message(body: String) -> String {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        message: String,
    }
    match serde_json::from_str::<ErrorBody>(&body) {
        Ok(parsed) => parsed.message,
        Err(_) => http::truncate_body(body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_extracts_github_message() {
        let body = r#"{"message":"Bad credentials","documentation_url":"https://docs.github.com"}"#;
        assert_eq!(error_message(body.to_string()), "Bad credentials");
    }

    #[test]
    fn error_message_falls_back_to_raw_body() {
        assert_eq!(
            error_message("<html>oops</html>".to_string()),
            "<html>oops</html>"
        );
    }

    #[test]
    fn detects_secondary_rate_limit_message() {
        assert!(is_secondary_rate_limit(
            "You have exceeded a secondary rate limit. Please wait a few minutes"
        ));
        assert!(!is_secondary_rate_limit("Not Found"));
    }

    #[test]
    fn client_rejects_token_with_control_chars() {
        assert!(matches!(
            GitHubClient::new("bad\ntoken", "user"),
            Err(RecapError::InvalidToken)
        ));
    }
}
