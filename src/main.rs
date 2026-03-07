use chrono::{Local, NaiveDate};
use clap::Parser;
use github_worklog::{
    cli::{Cli, Commands},
    config::Settings,
    error::RecapError,
    github::GitHubClient,
    recap::{file_ops::copy_file, file_ops::prepend_to_file, RecapGenerator},
    summarize,
};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        print_error(&e);
        std::process::exit(1);
    }
}

async fn run() -> github_worklog::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_level.into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    match &cli.command {
        Commands::Generate {
            date,
            preview,
            force,
        } => {
            let settings = load_settings(&cli)?;
            let target_date = parse_date(date.clone())?;
            debug!(
                "Generate command: date={}, preview={}, force={}",
                target_date, preview, force
            );
            run_generate(settings, target_date, *preview, *force).await?;
        }
        Commands::Today { preview, force } => {
            let settings = load_settings(&cli)?;
            let today = Local::now().date_naive();
            debug!(
                "Today command: date={}, preview={}, force={}",
                today, preview, force
            );
            run_generate(settings, today, *preview, *force).await?;
        }
        Commands::Config => {
            show_config(&cli);
        }
        Commands::Init => {
            show_init_instructions();
        }
    }

    Ok(())
}

fn load_settings(cli: &Cli) -> github_worklog::Result<Settings> {
    let settings = Settings::load()?;
    Ok(settings.with_overrides(
        cli.token.clone(),
        cli.username.clone(),
        cli.output.clone(),
        cli.provider,
        cli.ollama_model.clone(),
    ))
}

fn parse_date(date: Option<String>) -> github_worklog::Result<NaiveDate> {
    match date {
        Some(d) => NaiveDate::parse_from_str(&d, "%Y-%m-%d")
            .map_err(|_| RecapError::DateParse { input: d }),
        None => Ok(Local::now().date_naive()),
    }
}

async fn run_generate(
    settings: Settings,
    date: NaiveDate,
    preview: bool,
    force: bool,
) -> github_worklog::Result<()> {
    let client = GitHubClient::new(&settings.github_token, &settings.github_username)?;
    let generator = RecapGenerator::new(&settings.date_format);

    info!(
        "Fetching commits for {} on {}",
        settings.github_username,
        date.format("%Y-%m-%d")
    );
    eprintln!(
        "Fetching commits for {} on {}...",
        settings.github_username,
        date.format("%Y-%m-%d")
    );

    let activities = client.get_activities_for_date(date).await?;
    debug!("Found {} activities", activities.len());

    if activities.is_empty() {
        warn!(
            "No commits found for {} on {}",
            settings.github_username,
            date.format("%Y-%m-%d")
        );
        eprintln!("No commits found for this day.");
        return Ok(());
    }

    info!(
        "Found {} commits for {}",
        activities.len(),
        date.format("%Y-%m-%d")
    );

    // Generate markdown with optional AI summary
    debug!(
        "Generating markdown (summarizer: {:?})",
        settings.summarizer_provider
    );
    let markdown = match summarize(
        &activities,
        settings.summarizer_provider,
        settings.anthropic_api_key.as_deref(),
        &settings.ollama_url,
        &settings.ollama_model,
        &settings.claude_model,
        settings.claude_max_tokens,
    )
    .await
    {
        Some(summary) => {
            debug!("Got AI summary, generating markdown with summary");
            generator.generate_markdown_with_summary(date, &summary)
        }
        None => {
            debug!("No AI summary, generating plain markdown");
            generator.generate_markdown(date, activities.clone())
        }
    };

    if preview {
        info!("Preview mode - not writing to file");
        println!("{}", markdown);
    } else {
        let formatted_date = date.format(&settings.date_format).to_string();
        debug!(
            "Writing to file: {} (date header: '{}', force: {})",
            settings.output_file.display(),
            formatted_date,
            force
        );

        match prepend_to_file(&settings.output_file, &markdown, &formatted_date, force) {
            Ok(_) => {
                info!(
                    "Generated recap with {} activities. Saved to {}",
                    activities.len(),
                    settings.output_file.display()
                );
                eprintln!(
                    "Generated recap with {} activities. Saved to {}",
                    activities.len(),
                    settings.output_file.display()
                );
            }
            Err(RecapError::DuplicateDate { date, path }) => {
                warn!("Duplicate entry detected for date '{}' in {}", date, path);
                eprintln!(
                    "Entry for date '{}' already exists in {}. Use --force to overwrite.",
                    date, path
                );
                return Ok(());
            }
            Err(e) => {
                error!("Failed to write recap: {}", e);
                return Err(e);
            }
        }

        // Copy to Bear if configured
        if let Some(bear_path) = &settings.bear_copy_path {
            debug!("Copying to Bear: {}", bear_path.display());
            copy_file(&settings.output_file, bear_path)?;
            info!("Copied to Bear: {}", bear_path.display());
            eprintln!("Copied to Bear: {}", bear_path.display());
        }
    }

    Ok(())
}

fn show_config(cli: &Cli) {
    // Load .env file first
    dotenvy::dotenv().ok();

    println!("GitHub Daily Recap Configuration\n");
    println!("Loaded settings:");

    match Settings::load() {
        Ok(settings) => {
            let settings = settings.with_overrides(
                cli.token.clone(),
                cli.username.clone(),
                cli.output.clone(),
                cli.provider,
                cli.ollama_model.clone(),
            );
            println!(
                "  GITHUB_TOKEN: {}****",
                &settings.github_token[..8.min(settings.github_token.len())]
            );
            println!("  GITHUB_USERNAME: {}", settings.github_username);
            println!("  OUTPUT_FILE: {}", settings.output_file.display());
            println!("  DATE_FORMAT: {}", settings.date_format);
            match &settings.bear_copy_path {
                Some(path) => println!("  BEAR_COPY_PATH: {}", path.display()),
                None => println!("  BEAR_COPY_PATH: (not set)"),
            }
            println!("  SUMMARIZER_PROVIDER: {}", settings.summarizer_provider);
            match &settings.anthropic_api_key {
                Some(key) => println!("  ANTHROPIC_API_KEY: {}****", &key[..8.min(key.len())]),
                None => println!("  ANTHROPIC_API_KEY: (not set)"),
            }
            println!("  OLLAMA_URL: {}", settings.ollama_url);
            println!("  OLLAMA_MODEL: {}", settings.ollama_model);
        }
        Err(e) => {
            println!("  Error loading settings: {}", e);
            println!("\nRaw environment variables:");
            if let Ok(token) = std::env::var("GITHUB_TOKEN") {
                println!("  GITHUB_TOKEN: {}****", &token[..8.min(token.len())]);
            } else {
                println!("  GITHUB_TOKEN: (not set)");
            }
            if let Ok(username) = std::env::var("GITHUB_USERNAME") {
                println!("  GITHUB_USERNAME: {}", username);
            } else {
                println!("  GITHUB_USERNAME: (not set)");
            }
        }
    }

    println!("\nCLI Overrides:");
    println!("  --token: {:?}", cli.token.as_ref().map(|_| "****"));
    println!("  --username: {:?}", cli.username);
    println!("  --output: {:?}", cli.output);
    println!("  --provider: {:?}", cli.provider);
    println!("  --ollama-model: {:?}", cli.ollama_model);
}

fn show_init_instructions() {
    println!("GitHub Daily Recap - Setup Instructions\n");
    println!("1. Create a GitHub Personal Access Token:");
    println!("   https://github.com/settings/tokens\n");
    println!("   Required scopes: repo (for private repos) or public_repo (for public only)\n");
    println!("2. Set the following environment variables:\n");
    println!("   # Required");
    println!("   export GITHUB_TOKEN=\"ghp_your_token_here\"");
    println!("   export GITHUB_USERNAME=\"your-github-username\"\n");
    println!("   # Optional - Output");
    println!("   export OUTPUT_FILE=\"~/worklog.md\"");
    println!("   export DATE_FORMAT=\"%d/%m/%y\"\n");
    println!("   # Optional - Summarization (choose one provider)");
    println!("   export SUMMARIZER_PROVIDER=\"claude\"  # or \"ollama\"");
    println!("   export ANTHROPIC_API_KEY=\"sk-ant-...\"  # for Claude");
    println!("   export OLLAMA_URL=\"http://localhost:11434\"  # for Ollama");
    println!("   export OLLAMA_MODEL=\"llama3.2:3b\"  # for Ollama\n");
    println!("3. For Ollama (local LLM):");
    println!("   brew install ollama");
    println!("   ollama serve");
    println!("   ollama pull llama3.2:3b\n");
    println!("4. Add to your shell profile (~/.bashrc, ~/.zshrc, etc.) to persist.\n");
    println!("5. Usage:");
    println!("   github-worklog today           # Generate recap for today");
    println!("   github-worklog today --preview # Preview without saving");
    println!("   github-worklog today --provider ollama  # Use Ollama");
    println!("   github-worklog generate --date 2026-01-07");
}

fn print_error(e: &RecapError) {
    error!("Error occurred: {}", e);
    match e {
        RecapError::InvalidToken => {
            eprintln!("Error: Invalid GitHub token.");
            eprintln!("Please set GITHUB_TOKEN or GITHUB_TOKEN environment variable.");
            eprintln!("Generate a token at: https://github.com/settings/tokens");
        }
        RecapError::MissingConfig(field) => {
            eprintln!("Error: Missing required configuration '{}'", field);
            eprintln!("Run 'github-worklog init' for setup instructions.");
        }
        RecapError::RateLimitExceeded { retry_after } => {
            eprintln!("Error: GitHub API rate limit exceeded.");
            eprintln!("Please wait {} seconds before trying again.", retry_after);
        }
        RecapError::NoCommitsFound { username, date } => {
            eprintln!("No commits found for {} on {}", username, date);
        }
        RecapError::DuplicateDate { date, path } => {
            eprintln!("Entry for date '{}' already exists in {}", date, path);
            eprintln!("Use --force to overwrite the existing entry.");
        }
        _ => {
            eprintln!("Error: {}", e);
        }
    }
}
