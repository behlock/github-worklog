//! LLM-backed summarisation of a day's activity.
//!
//! Each provider is called over plain HTTPS. Transient failures (timeouts,
//! 429s, 5xxs) are retried with backoff, honouring `Retry-After`; anything
//! else is reported and the caller falls back to the un-summarised recap.

use crate::config::Settings;
use crate::github::Activity;
use crate::http::{self, MAX_ATTEMPTS, MAX_RETRY_AFTER, USER_AGENT};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::future::Future;
use std::time::Duration;
use tracing::{debug, warn};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Enables `fallbacks: "default"`, which re-runs a request that a safety
/// classifier declined on Anthropic's recommended substitute model.
const ANTHROPIC_BETA: &str = "server-side-fallback-2026-07-01";
const OPENAI_API_URL: &str = "https://api.openai.com/v1/chat/completions";
const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Current Claude models think before answering and that counts against
/// `max_tokens`, so a busy day can legitimately take a while. The official
/// SDKs default to ten minutes; five is plenty for a summary.
const CLOUD_TIMEOUT: Duration = Duration::from_secs(300);
/// Local models on modest hardware can take a while to load and generate.
const OLLAMA_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Provider {
    #[default]
    #[value(alias = "anthropic")]
    Claude,
    Ollama,
    #[value(name = "openai")]
    OpenAi,
    #[value(alias = "google")]
    Gemini,
}

/// Names and aliases live once, on the `ValueEnum` derive above; `Display`
/// and `FromStr` (used for `SUMMARIZER_PROVIDER`) read them from there.
impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = clap::ValueEnum::to_possible_value(self).expect("no skipped variants");
        f.write_str(value.get_name())
    }
}

impl std::str::FromStr for Provider {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <Self as clap::ValueEnum>::from_str(s.trim(), true).map_err(|_| {
            let names: Vec<String> = <Self as clap::ValueEnum>::value_variants()
                .iter()
                .filter_map(clap::ValueEnum::to_possible_value)
                .map(|v| v.get_name().to_string())
                .collect();
            format!(
                "unknown provider '{s}' (expected one of: {})",
                names.join(", ")
            )
        })
    }
}

#[derive(Debug)]
enum LlmError {
    /// Worth retrying: network trouble, rate limiting, server errors.
    /// Carries the server's requested pause, if it sent one.
    Transient {
        message: String,
        retry_after: Option<Duration>,
    },
    /// Retrying will not help: bad key, bad model, malformed response.
    Permanent(String),
}

impl LlmError {
    fn transient(message: impl Into<String>) -> Self {
        LlmError::Transient {
            message: message.into(),
            retry_after: None,
        }
    }
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Transient { message, .. } | LlmError::Permanent(message) => {
                f.write_str(message)
            }
        }
    }
}

impl From<reqwest::Error> for LlmError {
    fn from(e: reqwest::Error) -> Self {
        // A timed-out generation was probably billed; re-issuing the same
        // request would just bill it again and time out again.
        if e.is_timeout() {
            LlmError::Permanent(format!(
                "{e} (no answer within {}s; the request was not retried)",
                CLOUD_TIMEOUT.as_secs()
            ))
        } else if http::is_transient(&e) {
            LlmError::transient(e.to_string())
        } else {
            LlmError::Permanent(e.to_string())
        }
    }
}

type LlmResult = Result<String, LlmError>;

/// Summarise activities with the configured provider.
///
/// Returns `None` when summarisation is not possible (no key, provider down
/// after retries, empty response); the caller should then fall back to the
/// plain recap. Ollama additionally falls back to Claude when a key is set.
pub async fn summarize(activities: &[Activity], settings: &Settings) -> Option<String> {
    if activities.is_empty() {
        return None;
    }

    let prompt = build_prompt(activities);
    let client = Client::builder().user_agent(USER_AGENT).build().ok()?;

    match run_provider(&client, settings, &prompt).await {
        Ok(summary) if !summary.trim().is_empty() => Some(summary),
        Ok(_) => {
            eprintln!("Summarizer returned an empty response; using plain commit list instead.");
            None
        }
        Err(e) => {
            eprintln!("Summarization failed: {e}");
            eprintln!("Using plain commit list instead.");
            None
        }
    }
}

async fn run_provider(client: &Client, settings: &Settings, prompt: &str) -> LlmResult {
    let max_tokens = settings.max_tokens;
    match settings.summarizer_provider {
        Provider::Claude => run_claude(client, settings, prompt).await,
        Provider::OpenAi => {
            let key = require_key(settings.openai_api_key.as_deref(), "OPENAI_API_KEY")?;
            announce("OpenAI", &settings.openai_model);
            with_retry("OpenAI", || {
                call_openai(client, key, prompt, &settings.openai_model, max_tokens)
            })
            .await
        }
        Provider::Gemini => {
            let key = require_key(settings.gemini_api_key.as_deref(), "GEMINI_API_KEY")?;
            announce("Gemini", &settings.gemini_model);
            with_retry("Gemini", || {
                call_gemini(client, key, prompt, &settings.gemini_model, max_tokens)
            })
            .await
        }
        Provider::Ollama => {
            announce("Ollama", &settings.ollama_model);
            let result = with_retry("Ollama", || {
                call_ollama(
                    client,
                    &settings.ollama_url,
                    &settings.ollama_model,
                    prompt,
                    max_tokens,
                )
            })
            .await;
            match result {
                Err(e) if settings.anthropic_api_key.is_some() => {
                    eprintln!("Ollama failed: {e}");
                    eprintln!("Falling back to Claude...");
                    run_claude(client, settings, prompt).await
                }
                other => other,
            }
        }
    }
}

async fn run_claude(client: &Client, settings: &Settings, prompt: &str) -> LlmResult {
    let key = require_key(settings.anthropic_api_key.as_deref(), "ANTHROPIC_API_KEY")?;
    announce("Claude", &settings.claude_model);
    let model = &settings.claude_model;
    let max_tokens = settings.max_tokens;

    let result = with_retry("Claude", || {
        call_claude(client, key, prompt, model, max_tokens, ClaudeExtras::On)
    })
    .await;

    // Older models reject the effort/fallback fields; retry once without them.
    match result {
        Err(LlmError::Permanent(ref msg)) if rejects_claude_extras(msg) => {
            warn!("{model} rejected output_config/fallbacks; retrying without them");
            with_retry("Claude", || {
                call_claude(client, key, prompt, model, max_tokens, ClaudeExtras::Off)
            })
            .await
        }
        other => other,
    }
}

fn require_key<'a>(key: Option<&'a str>, var: &str) -> Result<&'a str, LlmError> {
    key.ok_or_else(|| LlmError::Permanent(format!("{var} not set")))
}

fn announce(name: &str, model: &str) {
    eprintln!("Summarizing with {name} ({model})...");
}

/// Run `op` up to [`MAX_ATTEMPTS`] times. Between transient failures wait
/// for the server's `Retry-After` when given (giving up if it exceeds
/// [`MAX_RETRY_AFTER`]), otherwise exponential backoff. Never sleeps after
/// the final attempt.
async fn with_retry<F, Fut>(name: &str, mut op: F) -> LlmResult
where
    F: FnMut() -> Fut,
    Fut: Future<Output = LlmResult>,
{
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match op().await {
            Ok(s) => return Ok(s),
            Err(LlmError::Transient {
                message,
                retry_after,
            }) if attempt < MAX_ATTEMPTS => {
                let delay = match retry_after {
                    Some(wait) if wait > MAX_RETRY_AFTER => {
                        return Err(LlmError::Permanent(format!(
                            "{message} (server asked to wait {}s; giving up)",
                            wait.as_secs()
                        )));
                    }
                    Some(wait) => wait,
                    None => http::backoff_delay(attempt),
                };
                warn!(
                    "{name} request failed (attempt {attempt}/{MAX_ATTEMPTS}): {message}; retrying in {}s",
                    delay.as_secs()
                );
                eprintln!("{name} request failed, retrying ({attempt}/{MAX_ATTEMPTS})...");
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Classify a non-success HTTP response. Never sleeps; the retry loop decides.
async fn http_error(provider: &str, response: Response) -> LlmError {
    let status = response.status();
    let retry_after = http::retry_after(&response);
    let body = http::truncate_body(response.text().await.unwrap_or_default());
    let message = format!("{provider} API error {status}: {body}");

    let retryable = status == StatusCode::TOO_MANY_REQUESTS
        || status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::CONFLICT
        || status.is_server_error();
    if retryable {
        LlmError::Transient {
            message,
            retry_after,
        }
    } else {
        LlmError::Permanent(message)
    }
}

// ---------------------------------------------------------------------------
// Claude
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum ClaudeExtras {
    /// Send `output_config.effort` and `fallbacks` (current models).
    On,
    /// Bare request for models that reject those fields.
    Off,
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeBlock>,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClaudeBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(other)]
    Other,
}

async fn call_claude(
    client: &Client,
    api_key: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
    extras: ClaudeExtras,
) -> LlmResult {
    #[derive(Serialize)]
    struct Req<'a> {
        model: &'a str,
        max_tokens: u32,
        /// Summarising a commit list is routine work; keep thinking light.
        #[serde(skip_serializing_if = "Option::is_none")]
        output_config: Option<OutputConfig<'a>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        fallbacks: Option<&'a str>,
        messages: Vec<Msg<'a>>,
    }
    #[derive(Serialize)]
    struct OutputConfig<'a> {
        effort: &'a str,
    }
    #[derive(Serialize)]
    struct Msg<'a> {
        role: &'static str,
        content: &'a str,
    }

    let with_extras = matches!(extras, ClaudeExtras::On);
    let mut request = client
        .post(ANTHROPIC_API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .timeout(CLOUD_TIMEOUT);
    if with_extras {
        request = request.header("anthropic-beta", ANTHROPIC_BETA);
    }

    let response = request
        .json(&Req {
            model,
            max_tokens,
            output_config: with_extras.then_some(OutputConfig { effort: "low" }),
            fallbacks: with_extras.then_some("default"),
            messages: vec![Msg {
                role: "user",
                content: prompt,
            }],
        })
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(http_error("Claude", response).await);
    }
    parse_claude_response(&response.text().await?)
}

/// A 400 (as formatted by [`http_error`]) whose body names one of the
/// optional request fields we send for current models.
fn rejects_claude_extras(message: &str) -> bool {
    message.starts_with("Claude API error 400")
        && (message.contains("output_config") || message.contains("fallbacks"))
}

fn parse_claude_response(body: &str) -> LlmResult {
    let data: ClaudeResponse = serde_json::from_str(body)
        .map_err(|e| LlmError::Permanent(format!("Claude response was not understood: {e}")))?;

    match data.stop_reason.as_deref() {
        Some("refusal") => {
            return Err(LlmError::Permanent(
                "Claude declined to summarise this content".into(),
            ));
        }
        Some("max_tokens") => warn!("Claude hit MAX_TOKENS; the summary may be truncated"),
        _ => {}
    }

    let text = data
        .content
        .into_iter()
        .filter_map(|b| match b {
            ClaudeBlock::Text { text } => Some(text),
            ClaudeBlock::Other => None,
        })
        .collect::<Vec<_>>()
        .join("");
    Ok(text.trim().to_string())
}

// ---------------------------------------------------------------------------
// Ollama
// ---------------------------------------------------------------------------

async fn call_ollama(
    client: &Client,
    url: &str,
    model: &str,
    prompt: &str,
    max_tokens: u32,
) -> LlmResult {
    #[derive(Serialize)]
    struct Req<'a> {
        model: &'a str,
        prompt: &'a str,
        stream: bool,
        /// Reasoning models (qwen3, deepseek-r1) otherwise spend the whole
        /// token budget thinking; older Ollama versions ignore this field.
        think: bool,
        options: Options,
    }
    #[derive(Serialize)]
    struct Options {
        num_predict: u32,
    }
    #[derive(Deserialize)]
    struct Resp {
        response: String,
    }

    // A local server that refuses connections or times out will not do
    // better on a retry, and retrying would delay the Claude fallback by
    // minutes; treat network failures as final.
    let response = client
        .post(format!("{}/api/generate", url.trim_end_matches('/')))
        .timeout(OLLAMA_TIMEOUT)
        .json(&Req {
            model,
            prompt,
            stream: false,
            think: false,
            options: Options {
                num_predict: max_tokens,
            },
        })
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() {
                LlmError::Permanent(format!(
                    "Ollama is not reachable at {url} ({e}); is `ollama serve` running?"
                ))
            } else if e.is_timeout() {
                LlmError::Permanent(format!(
                    "Ollama did not answer within {}s with model {model}",
                    OLLAMA_TIMEOUT.as_secs()
                ))
            } else {
                e.into()
            }
        })?;

    if response.status() == StatusCode::NOT_FOUND {
        let body = http::truncate_body(response.text().await.unwrap_or_default());
        return Err(LlmError::Permanent(format!(
            "Ollama API error 404: {body} (hint: run `ollama pull {model}`)"
        )));
    }
    if !response.status().is_success() {
        return Err(http_error("Ollama", response).await);
    }

    let data: Resp = response.json().await?;
    Ok(strip_think_blocks(&data.response).trim().to_string())
}

/// Remove `<think>...</think>` sections some local models emit inline even
/// when asked not to think.
fn strip_think_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => return out, // unterminated: drop the tail
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// OpenAI
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}
#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}
#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

async fn call_openai(
    client: &Client,
    api_key: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> LlmResult {
    #[derive(Serialize)]
    struct Req<'a> {
        model: &'a str,
        max_completion_tokens: u32,
        messages: Vec<Msg<'a>>,
    }
    #[derive(Serialize)]
    struct Msg<'a> {
        role: &'static str,
        content: &'a str,
    }

    let response = client
        .post(OPENAI_API_URL)
        .bearer_auth(api_key)
        .timeout(CLOUD_TIMEOUT)
        .json(&Req {
            model,
            max_completion_tokens: max_tokens,
            messages: vec![Msg {
                role: "user",
                content: prompt,
            }],
        })
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(http_error("OpenAI", response).await);
    }
    parse_openai_response(&response.text().await?)
}

fn parse_openai_response(body: &str) -> LlmResult {
    let data: OpenAiResponse = serde_json::from_str(body)
        .map_err(|e| LlmError::Permanent(format!("OpenAI response was not understood: {e}")))?;
    let text = data
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_default();
    Ok(text.trim().to_string())
}

// ---------------------------------------------------------------------------
// Gemini
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "promptFeedback")]
    prompt_feedback: Option<GeminiPromptFeedback>,
}
#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}
#[derive(Deserialize)]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}
#[derive(Deserialize)]
struct GeminiPart {
    text: Option<String>,
}
#[derive(Deserialize)]
struct GeminiPromptFeedback {
    #[serde(rename = "blockReason")]
    block_reason: Option<String>,
}

async fn call_gemini(
    client: &Client,
    api_key: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> LlmResult {
    #[derive(Serialize)]
    struct Req<'a> {
        contents: Vec<Content<'a>>,
        #[serde(rename = "generationConfig")]
        generation_config: GenConfig,
    }
    #[derive(Serialize)]
    struct Content<'a> {
        parts: Vec<Part<'a>>,
    }
    #[derive(Serialize)]
    struct Part<'a> {
        text: &'a str,
    }
    #[derive(Serialize)]
    struct GenConfig {
        #[serde(rename = "maxOutputTokens")]
        max_output_tokens: u32,
    }

    let response = client
        .post(format!("{GEMINI_API_BASE}/{model}:generateContent"))
        .header("x-goog-api-key", api_key)
        .timeout(CLOUD_TIMEOUT)
        .json(&Req {
            contents: vec![Content {
                parts: vec![Part { text: prompt }],
            }],
            generation_config: GenConfig {
                max_output_tokens: max_tokens,
            },
        })
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(http_error("Gemini", response).await);
    }
    parse_gemini_response(&response.text().await?)
}

fn parse_gemini_response(body: &str) -> LlmResult {
    let data: GeminiResponse = serde_json::from_str(body)
        .map_err(|e| LlmError::Permanent(format!("Gemini response was not understood: {e}")))?;

    if let Some(reason) = data.prompt_feedback.and_then(|f| f.block_reason) {
        return Err(LlmError::Permanent(format!(
            "Gemini blocked the prompt ({reason})"
        )));
    }

    let text = data
        .candidates
        .into_iter()
        .filter_map(|c| c.content)
        .flat_map(|c| c.parts)
        .filter_map(|p| p.text)
        .collect::<Vec<_>>()
        .join("");
    Ok(text.trim().to_string())
}

// ---------------------------------------------------------------------------
// Prompt
// ---------------------------------------------------------------------------

/// One prompt line per distinct (repo, subject, PR) triple, in first-seen
/// order, with a count when the same commit subject repeats. Small models
/// otherwise turn repeated "checkpoint" commits into repeated bullets.
fn prompt_lines(activities: &[Activity]) -> Vec<String> {
    let mut lines: Vec<(String, usize)> = Vec::new();
    for a in activities {
        let pr = a
            .associated_pr
            .as_ref()
            .map(|p| format!(" (PR #{})", p.number))
            .unwrap_or_default();
        let line = format!(
            "- [{}] {}{pr}",
            a.commit.repository.full_name(),
            a.commit.subject()
        );
        match lines.iter_mut().find(|(l, _)| *l == line) {
            Some((_, n)) => *n += 1,
            None => lines.push((line, 1)),
        }
    }
    lines
        .into_iter()
        .map(|(line, n)| {
            if n > 1 {
                format!("{line} (x{n})")
            } else {
                line
            }
        })
        .collect()
}

fn build_prompt(activities: &[Activity]) -> String {
    let commits = prompt_lines(activities).join("\n");

    debug!("Prompt covers {} commits", activities.len());
    format!(
        "Summarize this developer's daily GitHub activity.\n\
         Group the output by repository. For each repository, output a heading like:\n\
         #### `owner/repo`\n\
         Then list 1-3 bullet points starting with \"- \" summarizing work in that repo.\n\
         Use backticks around repository names, PR references (e.g. `PR #42`), and other code-related terms.\n\
         Focus on WHAT was accomplished, use action verbs. Only describe work shown in the commits below.\n\
         Return ONLY the repository headings and bullet points: no preamble, no closing remarks, no code fences.\n\n{commits}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{CommitInfo, PullRequestInfo, RepoInfo};
    use chrono::Utc;

    fn make_activity(owner: &str, repo: &str, message: &str, pr: Option<u64>) -> Activity {
        Activity {
            commit: CommitInfo {
                sha: "abc123".to_string(),
                message: message.to_string(),
                author: "testuser".to_string(),
                date: Utc::now(),
                repository: RepoInfo {
                    owner: owner.to_string(),
                    name: repo.to_string(),
                },
                html_url: "https://github.com/test".to_string(),
            },
            associated_pr: pr.map(|n| PullRequestInfo {
                number: n,
                title: "Test PR".to_string(),
                state: "closed".to_string(),
                html_url: format!("https://github.com/{owner}/{repo}/pull/{n}"),
            }),
        }
    }

    #[test]
    fn provider_parses_case_insensitively_with_aliases() {
        assert_eq!("OpenAI".parse::<Provider>(), Ok(Provider::OpenAi));
        assert_eq!(" gemini ".parse::<Provider>(), Ok(Provider::Gemini));
        assert_eq!("anthropic".parse::<Provider>(), Ok(Provider::Claude));
        assert_eq!("Google".parse::<Provider>(), Ok(Provider::Gemini));
        let err = "bard".parse::<Provider>().unwrap_err();
        assert!(err.contains("claude, ollama, openai, gemini"), "{err}");
        assert_eq!(Provider::OpenAi.to_string(), "openai");
        assert_eq!(Provider::Claude.to_string(), "claude");
    }

    #[test]
    fn build_prompt_includes_repo_message_and_pr() {
        let activities = vec![
            make_activity("acme", "api", "fix bug\nsecond line", None),
            make_activity("acme", "web", "update ui", Some(10)),
        ];
        let prompt = build_prompt(&activities);
        assert!(prompt.contains("[acme/api] fix bug\n"));
        assert!(!prompt.contains("second line"));
        assert!(prompt.contains("[acme/web] update ui (PR #10)"));
    }

    #[test]
    fn prompt_collapses_repeated_commits_with_a_count() {
        let activities = vec![
            make_activity("acme", "api", "checkpoint", None),
            make_activity("acme", "web", "update ui", None),
            make_activity("acme", "api", "checkpoint", None),
            make_activity("acme", "api", "checkpoint", None),
        ];
        assert_eq!(
            prompt_lines(&activities),
            vec!["- [acme/api] checkpoint (x3)", "- [acme/web] update ui"]
        );
    }

    #[test]
    fn claude_response_joins_text_blocks_and_ignores_others() {
        let body = "{\"content\":[{\"type\":\"thinking\",\"thinking\":\"...\"},{\"type\":\"text\",\"text\":\"#### `a/b`\\n\"},{\"type\":\"text\",\"text\":\"- Did it\"}],\"stop_reason\":\"end_turn\"}";
        assert_eq!(parse_claude_response(body).unwrap(), "#### `a/b`\n- Did it");
    }

    #[test]
    fn claude_refusal_is_permanent_error() {
        let body = r#"{"content":[],"stop_reason":"refusal","stop_details":{"type":"refusal","category":null}}"#;
        assert!(matches!(
            parse_claude_response(body),
            Err(LlmError::Permanent(_))
        ));
    }

    #[test]
    fn detects_rejection_of_optional_claude_fields() {
        assert!(rejects_claude_extras(
            "Claude API error 400 Bad Request: {\"error\":{\"message\":\"output_config: Extra inputs are not permitted\"}}"
        ));
        assert!(rejects_claude_extras(
            "Claude API error 400 Bad Request: fallbacks is not supported for this model"
        ));
        assert!(!rejects_claude_extras(
            "Claude API error 401 Unauthorized: bad key"
        ));
        assert!(!rejects_claude_extras(
            "Claude API error 400 Bad Request: max_tokens must be > 0"
        ));
        // "400" or a field name appearing elsewhere is not enough.
        assert!(!rejects_claude_extras(
            "Claude API error 529 Overloaded: request 400abc fallbacks"
        ));
    }

    #[test]
    fn think_blocks_are_stripped() {
        assert_eq!(
            strip_think_blocks("<think>\nhmm\n</think>\n#### `a/b`\n- x"),
            "\n#### `a/b`\n- x"
        );
        assert_eq!(strip_think_blocks("plain"), "plain");
        assert_eq!(strip_think_blocks("keep <think>drop"), "keep ");
        assert_eq!(
            strip_think_blocks("a<think>1</think>b<think>2</think>c"),
            "abc"
        );
    }

    #[test]
    fn openai_null_content_is_empty_not_error() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":null}}]}"#;
        assert_eq!(parse_openai_response(body).unwrap(), "");
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"  hi  "}}]}"#;
        assert_eq!(parse_openai_response(body).unwrap(), "hi");
    }

    #[test]
    fn gemini_blocked_prompt_is_reported() {
        let body = r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#;
        let err = parse_gemini_response(body).unwrap_err().to_string();
        assert!(err.contains("SAFETY"));
        let body = r#"{"candidates":[{"content":{"parts":[{"text":"a"},{"text":"b"}]}}]}"#;
        assert_eq!(parse_gemini_response(body).unwrap(), "ab");
    }

    #[tokio::test(start_paused = true)]
    async fn retry_gives_up_after_max_attempts_on_transient() {
        let mut calls = 0;
        let result = with_retry("test", || {
            calls += 1;
            async { Err(LlmError::transient("boom")) }
        })
        .await;
        assert!(matches!(result, Err(LlmError::Transient { .. })));
        assert_eq!(calls, MAX_ATTEMPTS);
    }

    #[tokio::test(start_paused = true)]
    async fn retry_stops_immediately_on_permanent() {
        let mut calls = 0;
        let result = with_retry("test", || {
            calls += 1;
            async { Err(LlmError::Permanent("nope".into())) }
        })
        .await;
        assert!(matches!(result, Err(LlmError::Permanent(_))));
        assert_eq!(calls, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn retry_honours_retry_after_and_gives_up_on_long_waits() {
        // Short Retry-After: waits exactly that long, then retries.
        let mut calls = 0;
        let started = tokio::time::Instant::now();
        let result = with_retry("test", || {
            calls += 1;
            let first = calls == 1;
            async move {
                if first {
                    Err(LlmError::Transient {
                        message: "429".into(),
                        retry_after: Some(Duration::from_secs(7)),
                    })
                } else {
                    Ok("ok".into())
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(calls, 2);
        assert_eq!(started.elapsed(), Duration::from_secs(7));

        // Retry-After beyond the cap: give up immediately without sleeping.
        let mut calls = 0;
        let started = tokio::time::Instant::now();
        let result = with_retry("test", || {
            calls += 1;
            async {
                Err(LlmError::Transient {
                    message: "429".into(),
                    retry_after: Some(MAX_RETRY_AFTER + Duration::from_secs(1)),
                })
            }
        })
        .await;
        assert!(matches!(result, Err(LlmError::Permanent(_))));
        assert_eq!(calls, 1);
        assert_eq!(started.elapsed(), Duration::ZERO);
    }
}
