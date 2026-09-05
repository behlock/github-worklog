use chrono::{Local, NaiveDate};
use clap::Parser;
use github_worklog::{
    cli::{Cli, Commands},
    config::Settings,
    error::RecapError,
    github::GitHubClient,
    recap::{HeaderMatcher, RecapGenerator, WriteResult, prepend_to_file},
    summarize,
};
use std::process::ExitCode;
use tracing::{debug, info};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> ExitCode {
    // Load `.env` before clap parses so `env = "..."` fallbacks see it too.
    // A parse error (an unquoted value with spaces, say) leaves every later
    // line unloaded, which is far too easy to miss; say so loudly.
    let dotenv = dotenvy::dotenv();
    let cli = Cli::parse();
    init_logging(cli.verbose);
    match &dotenv {
        Ok(p) => debug!("Loaded environment from {}", p.display()),
        Err(e) if e.not_found() => debug!("No .env file found"),
        Err(e) => eprintln!(
            "Warning: could not fully load .env ({e}); values after the bad line are ignored. \
             Quote values that contain spaces."
        ),
    }

    match run(&cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            print_error(&e);
            ExitCode::from(e.exit_code())
        }
    }
}

/// Logs go to stderr so `--preview` output on stdout stays clean markdown.
fn init_logging(verbose: bool) {
    // Verbose enables our own debug output only; hyper/h2 internals stay quiet
    // unless RUST_LOG asks for them explicitly.
    let default_level = if verbose {
        "warn,github_worklog=debug"
    } else {
        "warn"
    };
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| default_level.into()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr),
        )
        .init();
}

async fn run(cli: &Cli) -> github_worklog::Result<()> {
    match &cli.command {
        Commands::Generate {
            date,
            preview,
            force,
        } => {
            let settings = Settings::from_cli(cli)?;
            let date = date.unwrap_or_else(|| Local::now().date_naive());
            run_generate(&settings, date, *preview, *force).await
        }
        Commands::Today { preview, force } => {
            let settings = Settings::from_cli(cli)?;
            run_generate(&settings, Local::now().date_naive(), *preview, *force).await
        }
        Commands::Config => {
            show_config(cli);
            Ok(())
        }
        Commands::Init => {
            show_init_instructions();
            Ok(())
        }
    }
}

async fn run_generate(
    settings: &Settings,
    date: NaiveDate,
    preview: bool,
    force: bool,
) -> github_worklog::Result<()> {
    debug!("generate: date={date}, preview={preview}, force={force}");
    let client = GitHubClient::new(&settings.github_token, &settings.github_username)?;
    let generator = RecapGenerator::new(&settings.date_format);

    eprintln!(
        "Fetching commits for {} on {date}...",
        settings.github_username
    );
    let activities = client.get_activities_for_date(date).await?;
    info!("Found {} commits for {date}", activities.len());

    if activities.is_empty() {
        eprintln!("No commits found for this day.");
        return Ok(());
    }

    debug!("Summarizer provider: {}", settings.summarizer_provider);
    let markdown = match summarize(&activities, settings).await {
        Some(summary) => match generator.generate_markdown_with_summary(date, &summary) {
            Some(md) => md,
            None => {
                eprintln!("Summary contained no usable content; using plain commit list instead.");
                generator.generate_markdown(date, &activities)
            }
        },
        None => generator.generate_markdown(date, &activities),
    };

    if preview {
        println!("{markdown}");
        return Ok(());
    }

    let header = generator.date_header(date);
    let output = &settings.output_file;
    let matcher = HeaderMatcher::new(&settings.date_format);
    match prepend_to_file(output, &markdown, date, matcher, force)? {
        WriteResult::Written => eprintln!(
            "Generated recap with {} activities. Saved to {}",
            activities.len(),
            output.display()
        ),
        WriteResult::Skipped => eprintln!(
            "Entry {header} already exists in {}. Use --force to overwrite.",
            output.display()
        ),
    }
    Ok(())
}

/// Show enough of a secret to recognise it without leaking most of it.
fn mask(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() >= 12 {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}****{tail}")
    } else {
        "****".to_string()
    }
}

fn mask_opt(secret: Option<&str>) -> String {
    secret.map(mask).unwrap_or_else(|| "(not set)".to_string())
}

fn show_config(cli: &Cli) {
    println!("GitHub Worklog configuration\n");
    match Settings::from_cli(cli) {
        Ok(s) => {
            println!("  GITHUB_TOKEN:        {}", mask(&s.github_token));
            println!("  GITHUB_USERNAME:     {}", s.github_username);
            println!("  OUTPUT_FILE:         {}", s.output_file.display());
            println!("  DATE_FORMAT:         {}", s.date_format);
            println!("  SUMMARIZER_PROVIDER: {}", s.summarizer_provider);
            println!("  MAX_TOKENS:          {}", s.max_tokens);
            println!();
            println!(
                "  ANTHROPIC_API_KEY:   {}",
                mask_opt(s.anthropic_api_key.as_deref())
            );
            println!("  CLAUDE_MODEL:        {}", s.claude_model);
            println!(
                "  OPENAI_API_KEY:      {}",
                mask_opt(s.openai_api_key.as_deref())
            );
            println!("  OPENAI_MODEL:        {}", s.openai_model);
            println!(
                "  GEMINI_API_KEY:      {}",
                mask_opt(s.gemini_api_key.as_deref())
            );
            println!("  GEMINI_MODEL:        {}", s.gemini_model);
            println!("  OLLAMA_URL:          {}", s.ollama_url);
            println!("  OLLAMA_MODEL:        {}", s.ollama_model);
        }
        Err(e) => {
            println!("  Error loading settings: {e}");
            println!("  Run 'github-worklog init' for setup instructions.");
        }
    }
}

fn show_init_instructions() {
    print!(
        "\
Setup:
  1. cp .env.example .env
  2. Edit .env (GITHUB_TOKEN + GITHUB_USERNAME required)
     Get a token at: https://github.com/settings/tokens

Usage:
  github-worklog today              # Generate recap for today
  github-worklog today --preview    # Preview without saving
  github-worklog generate -d 2026-01-07
  github-worklog generate -d 2026-01-07 --force   # Replace an existing entry
  github-worklog config             # Show current settings
"
    );
}

fn print_error(e: &RecapError) {
    match e {
        RecapError::InvalidToken => {
            eprintln!("Error: GitHub rejected the token.");
            eprintln!("Check GITHUB_TOKEN (or --token) and that it has the 'repo' scope.");
            eprintln!("Generate a token at: https://github.com/settings/tokens");
        }
        RecapError::MissingConfig(field) => {
            eprintln!("Error: Missing required configuration '{field}'");
            eprintln!("Run 'github-worklog init' for setup instructions.");
        }
        RecapError::RateLimitExceeded { retry_after } => {
            eprintln!("Error: GitHub API rate limit exceeded.");
            eprintln!("Please wait {retry_after} seconds before trying again.");
        }
        _ => eprintln!("Error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::mask;

    #[test]
    fn mask_keeps_only_edges() {
        assert_eq!(mask("ghp_abcdefghijklmnop"), "ghp_****mnop");
        assert_eq!(mask("short"), "****");
        assert_eq!(mask(""), "****");
    }
}
