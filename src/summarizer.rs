use crate::github::Activity;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: String,
}

pub struct Summarizer {
    client: Client,
    api_key: String,
}

impl Summarizer {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
        }
    }

    pub async fn summarize_activities(&self, activities: &[Activity]) -> Result<String, String> {
        if activities.is_empty() {
            return Ok("No activities to summarize.".to_string());
        }

        // Build a description of all commits for Claude
        let mut commits_text = String::new();
        for activity in activities {
            let repo = format!(
                "{}/{}",
                activity.commit.repository.owner, activity.commit.repository.name
            );
            let message = &activity.commit.message;
            let pr_info = match &activity.associated_pr {
                Some(pr) => format!(" (PR #{}: {})", pr.number, pr.title),
                None => String::new(),
            };
            commits_text.push_str(&format!(
                "- [{}] {}{}\n",
                repo,
                message.lines().next().unwrap_or(""),
                pr_info
            ));
        }

        let prompt = format!(
            r#"You are summarizing a developer's daily GitHub activity for their personal work log.

Here are the commits and PRs from today:

{}

Please create a concise summary with 2-5 bullet points that:
1. Group related commits into meaningful features or project areas
2. Use clear, action-oriented language (e.g., "Implemented...", "Fixed...", "Updated...")
3. Focus on WHAT was accomplished, not individual commit messages
4. Mention the project/repo name when relevant
5. Skip merge commits and focus on actual work done

Format: Return ONLY the bullet points, one per line, starting with "- ". No headers or extra text."#,
            commits_text
        );

        let request = ClaudeRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 500,
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt,
            }],
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("Claude API error {}: {}", status, body));
        }

        let claude_response: ClaudeResponse = response
            .json()
            .await
            .map_err(|e| format!("JSON parse error: {}", e))?;

        let summary = claude_response
            .content
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_else(|| "No summary generated.".to_string());

        Ok(summary)
    }
}
