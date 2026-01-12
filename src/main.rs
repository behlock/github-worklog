use chrono::{Local, NaiveDate};
use clap::Parser;
use github_daily_recap::{
    cli::{Cli, Commands},
    config::Settings,
    error::RecapError,
    github::GitHubClient,
    recap::{append_to_file, file_ops::copy_file, RecapGenerator},
    summarize,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        print_error(&e);
        std::process::exit(1);
    }
}

async fn run() -> github_daily_recap::Result<()> {
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
        Commands::Generate { date, preview } => {
            let settings = load_settings(&cli)?;
            let target_date = parse_date(date.clone())?;
            run_generate(settings, target_date, *preview).await?;
        }
        Commands::Today { preview } => {
            let settings = load_settings(&cli)?;
            let today = Local::now().date_naive();
            run_generate(settings, today, *preview).await?;
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

fn load_settings(cli: &Cli) -> github_daily_recap::Result<Settings> {
    let settings = Settings::load()?;
    Ok(settings.with_overrides(
        cli.token.clone(),
        cli.username.clone(),
        cli.output.clone(),
        cli.provider,
        cli.ollama_model.clone(),
    ))
}

fn parse_date(date: Option<String>) -> github_daily_recap::Result<NaiveDate> {
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
) -> github_daily_recap::Result<()> {
    let client = GitHubClient::new(&settings.github_token, &settings.github_username)?;
    let generator = RecapGenerator::new(&settings.date_format);

    eprintln!(
        "Fetching commits for {} on {}...",
        settings.github_username,
        date.format("%Y-%m-%d")
    );

    let activities = client.get_activities_for_date(date).await?;

    if activities.is_empty() {
        eprintln!("No commits found for this day.");
        return Ok(());
    }

    // Generate markdown with optional AI summary
    let markdown = match summarize(
        &activities,
        settings.summarizer_provider,
        settings.anthropic_api_key.as_deref(),
        &settings.ollama_url,
        &settings.ollama_model,
    )
    .await
    {
        Some(summary) => generator.generate_markdown_with_summary(date, &summary),
        None => generator.generate_markdown(date, activities.clone()),
    };

    if preview {
        println!("{}", markdown);
    } else {
        append_to_file(&settings.output_file, &markdown)?;
        eprintln!(
            "Generated recap with {} activities. Saved to {}",
            activities.len(),
            settings.output_file.display()
        );

        // Copy to Bear if configured
        if let Some(bear_path) = &settings.bear_copy_path {
            copy_file(&settings.output_file, bear_path)?;
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
    println!("   export OUTPUT_FILE=\"~/daily-recap.md\"");
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
    println!("   github-daily-recap today           # Generate recap for today");
    println!("   github-daily-recap today --preview # Preview without saving");
    println!("   github-daily-recap today --provider ollama  # Use Ollama");
    println!("   github-daily-recap generate --date 2026-01-07");
}

fn print_error(e: &RecapError) {
    match e {
        RecapError::InvalidToken => {
            eprintln!("Error: Invalid GitHub token.");
            eprintln!("Please set GITHUB_TOKEN or GITHUB_TOKEN environment variable.");
            eprintln!("Generate a token at: https://github.com/settings/tokens");
        }
        RecapError::MissingConfig(field) => {
            eprintln!("Error: Missing required configuration '{}'", field);
            eprintln!("Run 'github-daily-recap init' for setup instructions.");
        }
        RecapError::RateLimitExceeded { retry_after } => {
            eprintln!("Error: GitHub API rate limit exceeded.");
            eprintln!("Please wait {} seconds before trying again.", retry_after);
        }
        RecapError::NoCommitsFound { username, date } => {
            eprintln!("No commits found for {} on {}", username, date);
        }
        _ => {
            eprintln!("Error: {}", e);
        }
    }
}
