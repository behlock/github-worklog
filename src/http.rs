//! HTTP policy shared by the GitHub client and the LLM providers: one user
//! agent, one backoff curve, one `Retry-After` parser, one body truncation.

use reqwest::Response;
use reqwest::header::RETRY_AFTER;
use std::time::Duration;

pub const USER_AGENT: &str = concat!("github-worklog/", env!("CARGO_PKG_VERSION"));

/// Longest server-requested pause we will honour before giving up.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Total attempts (first try plus retries) for any single HTTP call.
pub const MAX_ATTEMPTS: u32 = 3;

/// Characters of an error body worth keeping in a message.
const MAX_BODY_CHARS: usize = 500;

/// Exponential backoff: 2s, 4s, 8s, 16s, capped at 16s.
pub fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << attempt.clamp(1, 4))
}

/// `Retry-After` in seconds, if the server sent a numeric one.
pub fn retry_after(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Network-level failures worth retrying (timeouts, connection resets).
pub fn is_transient(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

pub fn truncate_body(body: String) -> String {
    if body.chars().count() <= MAX_BODY_CHARS {
        body
    } else {
        let mut s: String = body.chars().take(MAX_BODY_CHARS).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_delay(0), Duration::from_secs(2));
        assert_eq!(backoff_delay(1), Duration::from_secs(2));
        assert_eq!(backoff_delay(2), Duration::from_secs(4));
        assert_eq!(backoff_delay(3), Duration::from_secs(8));
        assert_eq!(backoff_delay(4), Duration::from_secs(16));
        assert_eq!(backoff_delay(9), Duration::from_secs(16));
    }

    #[test]
    fn truncate_keeps_short_bodies_and_marks_long_ones() {
        assert_eq!(truncate_body("ok".into()), "ok");
        let long = "x".repeat(600);
        let t = truncate_body(long);
        assert_eq!(t.chars().count(), MAX_BODY_CHARS + 1);
        assert!(t.ends_with('…'));
    }
}
