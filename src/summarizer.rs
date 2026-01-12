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
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Provider::Claude => "claude",
                Provider::Ollama => "ollama",
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
            _ => Err(format!("unknown provider: {}", s)),
        }
    }
}

/// Summarize activities using the configured provider (with fallback to Claude if Ollama fails)
pub async fn summarize(
    activities: &[Activity],
    provider: Provider,
    anthropic_key: Option<&str>,
    ollama_url: &str,
    ollama_model: &str,
) -> Option<String> {
    if activities.is_empty() {
        return None;
    }

    let prompt = build_prompt(activities);
    let client = Client::new();

    match provider {
        Provider::Ollama => {
            eprintln!("Summarizing with Ollama ({})...", ollama_model);
            match call_ollama(&client, ollama_url, ollama_model, &prompt).await {
                Ok(s) => return Some(s),
                Err(e) => eprintln!("Ollama failed: {}", e),
            }
            // Fallback to Claude
            if let Some(key) = anthropic_key {
                eprintln!("Falling back to Claude...");
                call_claude(&client, key, &prompt).await.ok()
            } else {
                None
            }
        }
        Provider::Claude => {
            if let Some(key) = anthropic_key {
                eprintln!("Summarizing with Claude...");
                call_claude(&client, key, &prompt).await.ok()
            } else {
                None
            }
        }
    }
}

async fn call_claude(client: &Client, api_key: &str, prompt: &str) -> Result<String, String> {
    #[derive(Serialize)]
    struct Req {
        model: &'static str,
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
        .json(&Req {
            model: "claude-sonnet-4-20250514",
            max_tokens: 500,
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
        "Summarize this developer's daily GitHub activity into 2-5 bullet points.\n\
         Focus on WHAT was accomplished, group related work, use action verbs.\n\
         Return ONLY bullet points starting with \"- \".\n\n{}",
        commits
    )
}
