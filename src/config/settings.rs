use crate::error::{RecapError, Result};
use crate::summarizer::Provider;
use directories::ProjectDirs;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub github_token: String,
    pub github_username: String,
    pub output_file: PathBuf,
    #[serde(default = "default_date_format")]
    pub date_format: String,
    pub bear_copy_path: Option<PathBuf>,
    pub anthropic_api_key: Option<String>,
    pub summarizer_provider: Provider,
    pub ollama_model: String,
    pub ollama_url: String,
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

impl Settings {
    pub fn load() -> Result<Self> {
        // Load .env file if present
        dotenvy::dotenv().ok();

        // Try to get values from environment
        let github_token = std::env::var("GITHUB_TOKEN")
            .or_else(|_| std::env::var("GITHUB_TOKEN"))
            .map_err(|_| RecapError::MissingConfig("GITHUB_TOKEN".to_string()))?;

        let github_username = std::env::var("GITHUB_USERNAME")
            .or_else(|_| std::env::var("GITHUB_USERNAME"))
            .map_err(|_| RecapError::MissingConfig("GITHUB_USERNAME".to_string()))?;

        let output_file = std::env::var("OUTPUT_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("worklog.md")
            });

        let date_format = std::env::var("DATE_FORMAT").unwrap_or_else(|_| default_date_format());

        let bear_copy_path = std::env::var("BEAR_COPY_PATH").ok().map(PathBuf::from);

        let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();

        let summarizer_provider = std::env::var("SUMMARIZER_PROVIDER")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();

        let ollama_model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| default_ollama_model());

        let ollama_url = std::env::var("OLLAMA_URL").unwrap_or_else(|_| default_ollama_url());

        Ok(Settings {
            github_token,
            github_username,
            output_file,
            date_format,
            bear_copy_path,
            anthropic_api_key,
            summarizer_provider,
            ollama_model,
            ollama_url,
        })
    }

    pub fn with_overrides(
        mut self,
        token: Option<String>,
        username: Option<String>,
        output: Option<PathBuf>,
        provider: Option<Provider>,
        ollama_model: Option<String>,
    ) -> Self {
        if let Some(t) = token {
            self.github_token = t;
        }
        if let Some(u) = username {
            self.github_username = u;
        }
        if let Some(o) = output {
            self.output_file = o;
        }
        if let Some(p) = provider {
            self.summarizer_provider = p;
        }
        if let Some(m) = ollama_model {
            self.ollama_model = m;
        }
        self
    }

    pub fn config_dir() -> Option<PathBuf> {
        ProjectDirs::from("com", "github-worklog", "github-worklog")
            .map(|dirs| dirs.config_dir().to_path_buf())
    }

    pub fn expand_path(path: &PathBuf) -> PathBuf {
        let path_str = path.to_string_lossy();
        if path_str.starts_with("~") {
            if let Some(home) = dirs::home_dir() {
                return home.join(&path_str[2..]);
            }
        }
        path.clone()
    }
}
