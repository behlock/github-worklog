use crate::cli::Cli;
use crate::error::{RecapError, Result};
use crate::summarizer::Provider;
use directories::ProjectDirs;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub github_token: String,
    pub github_username: String,
    pub output_file: PathBuf,
    #[serde(default = "default_date_format")]
    pub date_format: String,

    pub anthropic_api_key: Option<String>,
    pub summarizer_provider: Provider,
    pub ollama_model: String,
    pub ollama_url: String,
    pub claude_model: String,
    pub max_tokens: u32,
    pub openai_api_key: Option<String>,
    pub openai_model: String,
    pub gemini_api_key: Option<String>,
    pub gemini_model: String,
}

fn default_date_format() -> String {
    "%d/%m/%y".to_string()
}

fn default_ollama_model() -> String {
    "llama3.2:3b".to_string()
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_claude_model() -> String {
    "claude-sonnet-4-6".to_string()
}

fn default_openai_model() -> String {
    "gpt-4.1-mini".to_string()
}

fn default_gemini_model() -> String {
    "gemini-2.5-flash".to_string()
}

const DEFAULT_MAX_TOKENS: u32 = 500;

impl Settings {
    pub fn load() -> Result<Self> {
        // Load .env file if present
        dotenvy::dotenv().ok();

        // Try to get values from environment
        let github_token = std::env::var("GITHUB_TOKEN")
            .map_err(|_| RecapError::MissingConfig("GITHUB_TOKEN".to_string()))?;

        let github_username = std::env::var("GITHUB_USERNAME")
            .map_err(|_| RecapError::MissingConfig("GITHUB_USERNAME".to_string()))?;

        let output_file = std::env::var("OUTPUT_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./worklog.md"));

        let date_format = std::env::var("DATE_FORMAT").unwrap_or_else(|_| default_date_format());

        let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();

        let summarizer_provider = std::env::var("SUMMARIZER_PROVIDER")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();

        let ollama_model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| default_ollama_model());

        let ollama_url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| default_ollama_url());

        let claude_model = std::env::var("CLAUDE_MODEL").unwrap_or_else(|_| default_claude_model());

        let max_tokens =
            match std::env::var("MAX_TOKENS").or_else(|_| std::env::var("CLAUDE_MAX_TOKENS")) {
                Ok(s) => match s.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        tracing::warn!(
                            "Invalid MAX_TOKENS value '{}', using default {}",
                            s,
                            DEFAULT_MAX_TOKENS
                        );
                        DEFAULT_MAX_TOKENS
                    }
                },
                Err(_) => DEFAULT_MAX_TOKENS,
            };

        let openai_api_key = std::env::var("OPENAI_API_KEY").ok();
        let openai_model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| default_openai_model());
        let gemini_api_key = std::env::var("GEMINI_API_KEY").ok();
        let gemini_model = std::env::var("GEMINI_MODEL").unwrap_or_else(|_| default_gemini_model());

        Ok(Settings {
            github_token,
            github_username,
            output_file,
            date_format,

            anthropic_api_key,
            summarizer_provider,
            ollama_model,
            ollama_url,
            claude_model,
            max_tokens,
            openai_api_key,
            openai_model,
            gemini_api_key,
            gemini_model,
        })
    }

    pub fn with_overrides(mut self, cli: &Cli) -> Self {
        if let Some(t) = cli.token.clone() {
            self.github_token = t;
        }
        if let Some(u) = cli.username.clone() {
            self.github_username = u;
        }
        if let Some(o) = cli.output.clone() {
            self.output_file = o;
        }
        if let Some(p) = cli.provider {
            self.summarizer_provider = p;
        }
        if let Some(m) = cli.ollama_model.clone() {
            self.ollama_model = m;
        }
        if let Some(m) = cli.openai_model.clone() {
            self.openai_model = m;
        }
        if let Some(m) = cli.gemini_model.clone() {
            self.gemini_model = m;
        }
        self
    }

    pub fn config_dir() -> Option<PathBuf> {
        ProjectDirs::from("com", "github-worklog", "github-worklog")
            .map(|dirs| dirs.config_dir().to_path_buf())
    }

    pub fn expand_path(path: &Path) -> PathBuf {
        let path_str = path.to_string_lossy();
        if path_str == "~" {
            if let Some(home) = dirs::home_dir() {
                return home;
            }
        } else if let Some(suffix) = path_str.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(suffix);
            }
        }
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::Parser;

    #[test]
    fn expand_path_bare_tilde() {
        let path = PathBuf::from("~");
        let expanded = Settings::expand_path(&path);
        assert_eq!(expanded, dirs::home_dir().unwrap());
    }

    #[test]
    fn expand_path_tilde_slash() {
        let path = PathBuf::from("~/worklog.md");
        let expanded = Settings::expand_path(&path);
        let expected = dirs::home_dir().unwrap().join("worklog.md");
        assert_eq!(expanded, expected);
    }

    #[test]
    fn expand_path_absolute_unchanged() {
        let path = PathBuf::from("/tmp/worklog.md");
        let expanded = Settings::expand_path(&path);
        assert_eq!(expanded, path);
    }

    #[test]
    fn expand_path_relative_unchanged() {
        let path = PathBuf::from("relative/path.md");
        let expanded = Settings::expand_path(&path);
        assert_eq!(expanded, path);
    }

    #[test]
    fn with_overrides_applies_values() {
        let settings = Settings {
            github_token: "original".to_string(),
            github_username: "user".to_string(),
            output_file: PathBuf::from("/tmp/out.md"),
            date_format: default_date_format(),

            anthropic_api_key: None,
            summarizer_provider: Provider::Claude,
            ollama_model: default_ollama_model(),
            ollama_url: default_ollama_url(),
            claude_model: default_claude_model(),
            max_tokens: DEFAULT_MAX_TOKENS,
            openai_api_key: None,
            openai_model: default_openai_model(),
            gemini_api_key: None,
            gemini_model: default_gemini_model(),
        };

        let cli = Cli::parse_from([
            "test",
            "--token",
            "new-token",
            "--username",
            "new-user",
            "--provider",
            "ollama",
            "config",
        ]);

        let updated = settings.with_overrides(&cli);

        assert_eq!(updated.github_token, "new-token");
        assert_eq!(updated.github_username, "new-user");
        assert_eq!(updated.output_file, PathBuf::from("/tmp/out.md"));
        assert_eq!(updated.summarizer_provider, Provider::Ollama);
    }

    #[test]
    fn default_claude_settings() {
        assert_eq!(default_claude_model(), "claude-sonnet-4-6");
        assert_eq!(DEFAULT_MAX_TOKENS, 500);
    }
}
