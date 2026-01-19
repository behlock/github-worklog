use crate::summarizer::Provider;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "github-worklog")]
#[command(about = "Generate daily GitHub activity recaps", long_about = None)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// GitHub Personal Access Token
    #[arg(long, env = "GITHUB_TOKEN", global = true)]
    pub token: Option<String>,

    /// GitHub username
    #[arg(long, short = 'u', env = "GITHUB_USERNAME", global = true)]
    pub username: Option<String>,

    /// Output file path
    #[arg(long, short = 'o', env = "OUTPUT_FILE", global = true)]
    pub output: Option<PathBuf>,

    /// Summarizer provider (claude or ollama)
    #[arg(long, short = 'p', env = "SUMMARIZER_PROVIDER", global = true)]
    pub provider: Option<Provider>,

    /// Ollama model to use (e.g., llama3.2:3b, phi3:mini, mistral:7b)
    #[arg(long, env = "OLLAMA_MODEL", global = true)]
    pub ollama_model: Option<String>,

    /// Enable verbose logging
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate recap for a specific date
    Generate {
        /// Date to generate recap for (YYYY-MM-DD format, defaults to today)
        #[arg(short, long)]
        date: Option<String>,

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
