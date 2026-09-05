use crate::summarizer::Provider;
use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "github-worklog")]
#[command(about = "Generate daily GitHub activity recaps", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// GitHub personal access token
    #[arg(long, env = "GITHUB_TOKEN", global = true, hide_env_values = true)]
    pub token: Option<String>,

    /// GitHub username
    #[arg(long, short = 'u', env = "GITHUB_USERNAME", global = true)]
    pub username: Option<String>,

    /// Output file path [env: OUTPUT_FILE]
    #[arg(long, short = 'o', global = true)]
    pub output: Option<PathBuf>,

    /// Summarizer provider [env: SUMMARIZER_PROVIDER]
    #[arg(long, short = 'p', global = true, ignore_case = true)]
    pub provider: Option<Provider>,

    /// Claude model to use (e.g., claude-opus-5, claude-sonnet-5)
    #[arg(long, env = "CLAUDE_MODEL", global = true)]
    pub claude_model: Option<String>,

    /// Ollama model to use (e.g., gemma4:e4b, gemma3:4b, llama3.2:3b)
    #[arg(long, env = "OLLAMA_MODEL", global = true)]
    pub ollama_model: Option<String>,

    /// OpenAI model to use (e.g., gpt-4.1-mini, gpt-4.1-nano)
    #[arg(long, env = "OPENAI_MODEL", global = true)]
    pub openai_model: Option<String>,

    /// Gemini model to use (e.g., gemini-2.5-flash, gemini-2.5-pro)
    #[arg(long, env = "GEMINI_MODEL", global = true)]
    pub gemini_model: Option<String>,

    /// Enable verbose logging
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Generate recap for a specific date
    Generate {
        /// Date to generate recap for (YYYY-MM-DD, defaults to today)
        #[arg(short, long, value_parser = parse_date)]
        date: Option<NaiveDate>,

        /// Preview without writing to file
        #[arg(long)]
        preview: bool,

        /// Force overwrite if entry for this date already exists
        #[arg(short, long)]
        force: bool,
    },

    /// Generate recap for today
    Today {
        /// Preview without writing to file
        #[arg(long)]
        preview: bool,

        /// Force overwrite if entry for this date already exists
        #[arg(short, long)]
        force: bool,
    },

    /// Show current configuration
    Config,

    /// Initialize configuration (shows required environment variables)
    Init,
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| format!("expected YYYY-MM-DD format, got '{s}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_date() {
        let cli = Cli::parse_from(["test", "generate", "--date", "2026-03-18"]);
        match cli.command {
            Commands::Generate { date, .. } => {
                assert_eq!(date, NaiveDate::from_ymd_opt(2026, 3, 18));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn provider_flag_is_case_insensitive_and_accepts_aliases() {
        let cli = Cli::parse_from(["test", "--provider", "OpenAI", "config"]);
        assert_eq!(cli.provider, Some(Provider::OpenAi));
        let cli = Cli::parse_from(["test", "--provider", "anthropic", "config"]);
        assert_eq!(cli.provider, Some(Provider::Claude));
        let cli = Cli::parse_from(["test", "--provider", "google", "config"]);
        assert_eq!(cli.provider, Some(Provider::Gemini));
    }

    #[test]
    fn rejects_invalid_date() {
        let result = Cli::try_parse_from(["test", "generate", "--date", "18/03/2026"]);
        assert!(result.is_err());
    }
}
