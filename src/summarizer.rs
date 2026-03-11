use crate::config::Settings;
use crate::github::Activity;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Claude,
    Ollama,
    OpenAi,
    Gemini,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Provider::Claude => "claude",
                Provider::Ollama => "ollama",
                Provider::OpenAi => "openai",
                Provider::Gemini => "gemini",
            }
        )
    }
}

impl std::str::FromStr for Provider {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "claude" => Ok(Provider::Claude),
            "ollama" => Ok(Provider::Ollama),
            "openai" => Ok(Provider::OpenAi),
            "gemini" => Ok(Provider::Gemini),
            _ => Err(format!("unknown provider: {}", s)),
        }
    }
}

/// Summarize activities using the configured provider (with fallback to Claude if Ollama fails)
pub async fn summarize(activities: &[Activity], settings: &Settings) -> Option<String> {
    if activities.is_empty() {
        return None;
    }

    let prompt = build_prompt(activities);
    let client = Client::new();

    match settings.summarizer_provider {
        Provider::Ollama => {
            eprintln!("Summarizing with Ollama ({})...", settings.ollama_model);
            match call_ollama(
                &client,
                &settings.ollama_url,
                &settings.ollama_model,
                &prompt,
            )
            .await
            {
                Ok(s) => return Some(s),
                Err(e) => eprintln!("Ollama failed: {}", e),
            }
            // Fallback to Claude
            if let Some(key) = settings.anthropic_api_key.as_deref() {
                eprintln!("Falling back to Claude...");
                call_claude(
                    &client,
                    key,
                    &prompt,
                    &settings.claude_model,
                    settings.max_tokens,
                )
                .await
                .ok()
            } else {
                None
            }
        }
        Provider::Claude => {
            if let Some(key) = settings.anthropic_api_key.as_deref() {
                eprintln!("Summarizing with Claude...");
                call_claude(
                    &client,
                    key,
                    &prompt,
                    &settings.claude_model,
                    settings.max_tokens,
                )
                .await
                .ok()
            } else {
                eprintln!("ANTHROPIC_API_KEY not set, skipping summarization");
                None
            }
        }
        Provider::OpenAi => {
            if let Some(key) = settings.openai_api_key.as_deref() {
                eprintln!("Summarizing with OpenAI ({})...", settings.openai_model);
                call_openai(
                    &client,
                    key,
                    &prompt,
                    &settings.openai_model,
                    settings.max_tokens,
                )
                .await
                .ok()
            } else {
                eprintln!("OPENAI_API_KEY not set, skipping summarization");
                None
            }
        }
        Provider::Gemini => {
            if let Some(key) = settings.gemini_api_key.as_deref() {
                eprintln!("Summarizing with Gemini ({})...", settings.gemini_model);
                call_gemini(
                    &client,
                    key,
                    &prompt,
                    &settings.gemini_model,
                    settings.max_tokens,
                )
                .await
                .ok()
            } else {
                eprintln!("GEMINI_API_KEY not set, skipping summarization");
                None
            }
        }
    }
}

async fn call_claude(
    client: &Client,
    api_key: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> Result<String, String> {
    #[derive(Serialize)]
    struct Req<'a> {
        model: &'a str,
        max_tokens: u32,
        messages: Vec<Msg>,
    }
    #[derive(Serialize)]
    struct Msg {
        role: &'static str,
        content: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        content: Vec<Block>,
    }
    #[derive(Deserialize)]
    struct Block {
        text: String,
    }

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(Duration::from_secs(30))
        .json(&Req {
            model,
            max_tokens,
            messages: vec![Msg {
                role: "user",
                content: prompt.to_string(),
            }],
        })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Claude API error {}: {}", status, body));
    }

    let data: Resp = resp.json().await.map_err(|e| e.to_string())?;
    Ok(data
        .content
        .first()
        .map(|b| b.text.clone())
        .unwrap_or_default())
}

async fn call_ollama(
    client: &Client,
    url: &str,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    #[derive(Serialize)]
    struct Req {
        model: String,
        prompt: String,
        stream: bool,
    }
    #[derive(Deserialize)]
    struct Resp {
        response: String,
    }

    let resp = client
        .post(format!("{}/api/generate", url.trim_end_matches('/')))
        .timeout(Duration::from_secs(120))
        .json(&Req {
            model: model.to_string(),
            prompt: prompt.to_string(),
            stream: false,
        })
        .send()
        .await
        .map_err(|e| format!("Ollama request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Ollama API error {}: {} (hint: run `ollama pull {}`)",
            status, body, model
        ));
    }

    let data: Resp = resp.json().await.map_err(|e| e.to_string())?;
    Ok(data.response.trim().to_string())
}

async fn call_openai(
    client: &Client,
    api_key: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> Result<String, String> {
    #[derive(Serialize)]
    struct Req<'a> {
        model: &'a str,
        #[serde(rename = "max_completion_tokens")]
        max_tokens: u32,
        messages: Vec<Msg>,
    }
    #[derive(Serialize)]
    struct Msg {
        role: &'static str,
        content: String,
    }
    #[derive(Deserialize)]
    struct Resp {
        choices: Vec<Choice>,
    }
    #[derive(Deserialize)]
    struct Choice {
        message: RespMsg,
    }
    #[derive(Deserialize)]
    struct RespMsg {
        content: String,
    }

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(Duration::from_secs(30))
        .json(&Req {
            model,
            max_tokens,
            messages: vec![Msg {
                role: "user",
                content: prompt.to_string(),
            }],
        })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("OpenAI API error {}: {}", status, body));
    }

    let data: Resp = resp.json().await.map_err(|e| e.to_string())?;
    Ok(data
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default())
}

async fn call_gemini(
    client: &Client,
    api_key: &str,
    prompt: &str,
    model: &str,
    max_tokens: u32,
) -> Result<String, String> {
    #[derive(Serialize)]
    struct Req {
        contents: Vec<Content>,
        #[serde(rename = "generationConfig")]
        generation_config: GenConfig,
    }
    #[derive(Serialize)]
    struct Content {
        parts: Vec<Part>,
    }
    #[derive(Serialize)]
    struct Part {
        text: String,
    }
    #[derive(Serialize)]
    struct GenConfig {
        #[serde(rename = "maxOutputTokens")]
        max_output_tokens: u32,
    }
    #[derive(Deserialize)]
    struct Resp {
        candidates: Vec<Candidate>,
    }
    #[derive(Deserialize)]
    struct Candidate {
        content: RespContent,
    }
    #[derive(Deserialize)]
    struct RespContent {
        parts: Vec<RespPart>,
    }
    #[derive(Deserialize)]
    struct RespPart {
        text: String,
    }

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );

    let resp = client
        .post(&url)
        .header("x-goog-api-key", api_key)
        .timeout(Duration::from_secs(30))
        .json(&Req {
            contents: vec![Content {
                parts: vec![Part {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: GenConfig {
                max_output_tokens: max_tokens,
            },
        })
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Gemini API error {}: {}", status, body));
    }

    let data: Resp = resp.json().await.map_err(|e| e.to_string())?;
    Ok(data
        .candidates
        .first()
        .and_then(|c| c.content.parts.first())
        .map(|p| p.text.clone())
        .unwrap_or_default())
}

fn build_prompt(activities: &[Activity]) -> String {
    let commits: String = activities
        .iter()
        .map(|a| {
            let repo = format!("{}/{}", a.commit.repository.owner, a.commit.repository.name);
            let msg = a.commit.message.lines().next().unwrap_or("");
            let pr = a
                .associated_pr
                .as_ref()
                .map(|p| format!(" (PR #{})", p.number))
                .unwrap_or_default();
            format!("- [{}] {}{}\n", repo, msg, pr)
        })
        .collect();

    format!(
        "Summarize this developer's daily GitHub activity.\n\
         Group the output by repository. For each repository, output a heading like:\n\
         #### `owner/repo`\n\
         Then list 1-3 bullet points starting with \"- \" summarizing work in that repo.\n\
         Use backticks around repository names, PR references (e.g. `PR #42`), and other code-related terms.\n\
         Focus on WHAT was accomplished, use action verbs.\n\
         Return ONLY the repository headings and bullet points, nothing else.\n\n{}",
        commits
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
                state: "merged".to_string(),
                html_url: format!("https://github.com/{}/{}/pull/{}", owner, repo, n),
            }),
        }
    }

    #[test]
    fn build_prompt_includes_repo_and_message() {
        let activities = vec![make_activity("acme", "api", "fix auth bug", None)];
        let prompt = build_prompt(&activities);

        assert!(prompt.contains("[acme/api]"));
        assert!(prompt.contains("fix auth bug"));
        assert!(prompt.contains("Summarize"));
    }

    #[test]
    fn build_prompt_includes_pr_number() {
        let activities = vec![make_activity("acme", "api", "add feature", Some(42))];
        let prompt = build_prompt(&activities);

        assert!(prompt.contains("(PR #42)"));
    }

    #[test]
    fn build_prompt_handles_multiline_commit_message() {
        let activities = vec![make_activity(
            "acme",
            "api",
            "first line\nsecond line\nthird line",
            None,
        )];
        let prompt = build_prompt(&activities);

        assert!(prompt.contains("first line"));
        assert!(!prompt.contains("second line"));
    }

    #[test]
    fn build_prompt_multiple_activities() {
        let activities = vec![
            make_activity("acme", "api", "fix bug", None),
            make_activity("acme", "web", "update ui", Some(10)),
        ];
        let prompt = build_prompt(&activities);

        assert!(prompt.contains("[acme/api] fix bug"));
        assert!(prompt.contains("[acme/web] update ui (PR #10)"));
    }
}
