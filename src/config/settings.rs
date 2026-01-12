use crate::error::{RecapError, Result};
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
}

fn default_date_format() -> String {
    "%d/%m/%y".to_string()
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
                    .join("daily-recap.md")
            });

        let date_format = std::env::var("DATE_FORMAT").unwrap_or_else(|_| default_date_format());

        let bear_copy_path = std::env::var("BEAR_COPY_PATH").ok().map(PathBuf::from);

        let anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();

        Ok(Settings {
            github_token,
            github_username,
            output_file,
            date_format,
            bear_copy_path,
            anthropic_api_key,
        })
    }

    pub fn with_overrides(
        mut self,
        token: Option<String>,
        username: Option<String>,
        output: Option<PathBuf>,
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
        self
    }

    pub fn config_dir() -> Option<PathBuf> {
        ProjectDirs::from("com", "github-daily-recap", "github-daily-recap")
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
