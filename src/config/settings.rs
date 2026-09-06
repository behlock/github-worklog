use crate::cli::Cli;
use crate::error::{RecapError, Result};
use crate::summarizer::Provider;
use chrono::{Local, NaiveDate};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const DEFAULT_OUTPUT_FILE: &str = "./worklog.md";
pub const DEFAULT_DATE_FORMAT: &str = "%d/%m/%y";
pub const DEFAULT_OLLAMA_MODEL: &str = "gemma4:e4b";
pub const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";
pub const DEFAULT_CLAUDE_MODEL: &str = "claude-opus-5";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-4.1-mini";
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
/// Upper bound on the summary response. Thinking tokens count against this on
/// current Claude models, so keep it comfortably above the expected output.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

#[derive(Debug, Clone)]
pub struct Settings {
    pub github_token: String,
    pub github_username: String,
    pub output_file: PathBuf,
    pub date_format: String,

    pub summarizer_provider: Provider,
    pub max_tokens: u32,

    pub anthropic_api_key: Option<String>,
    pub claude_model: String,
    pub ollama_url: String,
    pub ollama_model: String,
    pub openai_api_key: Option<String>,
    pub openai_model: String,
    pub gemini_api_key: Option<String>,
    pub gemini_model: String,
}

impl Settings {
    /// Build settings from parsed CLI arguments plus the process environment.
    ///
    /// Values that have a CLI flag (token, username, models) are resolved by
    /// clap from either the flag or the corresponding env var. Everything
    /// else is read from the environment here. `.env` must be loaded (via
    /// `dotenvy`) before the CLI is parsed for this to see it.
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        Self::build(cli, |name| std::env::var(name).ok())
    }

    /// Like [`from_cli`](Self::from_cli) but with an explicit environment
    /// lookup, so the result does not depend on the caller's real process
    /// environment (used by tests).
    pub fn build(cli: &Cli, env: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let env = |name: &str| non_empty(env(name).as_deref());

        let github_token = required(cli.token.as_deref(), "GITHUB_TOKEN")?;
        let github_username = required(cli.username.as_deref(), "GITHUB_USERNAME")?;

        // These two have no clap `env` attribute on purpose: clap would
        // hard-error on a blank `OUTPUT_FILE=` or a mixed-case provider, even
        // for `config`. Read them leniently here instead.
        let output_file = cli
            .output
            .clone()
            .or_else(|| env("OUTPUT_FILE").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_OUTPUT_FILE));

        let summarizer_provider = match cli.provider {
            Some(p) => p,
            None => provider_from_env(env("SUMMARIZER_PROVIDER"))?,
        };

        let max_tokens = env("MAX_TOKENS")
            .or_else(|| env("CLAUDE_MAX_TOKENS"))
            .map(|raw| match raw.parse::<u32>() {
                Ok(v) if v > 0 => v,
                _ => {
                    tracing::warn!(
                        "Invalid MAX_TOKENS value '{raw}', using default {DEFAULT_MAX_TOKENS}"
                    );
                    DEFAULT_MAX_TOKENS
                }
            })
            .unwrap_or(DEFAULT_MAX_TOKENS);

        let date_format = env("DATE_FORMAT").unwrap_or_else(|| DEFAULT_DATE_FORMAT.to_string());
        validate_date_format(&date_format)?;

        Ok(Settings {
            github_token,
            github_username,
            output_file: expand_tilde(&output_file),
            date_format,

            summarizer_provider,
            max_tokens,

            anthropic_api_key: env("ANTHROPIC_API_KEY"),
            claude_model: non_empty(cli.claude_model.as_deref())
                .unwrap_or_else(|| DEFAULT_CLAUDE_MODEL.to_string()),
            ollama_url: env("OLLAMA_URL").unwrap_or_else(|| DEFAULT_OLLAMA_URL.to_string()),
            ollama_model: non_empty(cli.ollama_model.as_deref())
                .unwrap_or_else(|| DEFAULT_OLLAMA_MODEL.to_string()),
            openai_api_key: env("OPENAI_API_KEY"),
            openai_model: non_empty(cli.openai_model.as_deref())
                .unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_string()),
            gemini_api_key: env("GEMINI_API_KEY"),
            gemini_model: non_empty(cli.gemini_model.as_deref())
                .unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_string()),
        })
    }
}

/// The date format is used both to render entry headers and to recognise
/// them again later, so it must round-trip: render today, parse it back, and
/// get the same day. Formats without a year (`%d %b`) or with invalid
/// specifiers (`%Q`) are rejected up front instead of silently breaking
/// duplicate detection or panicking after the network work is done.
pub fn validate_date_format(format: &str) -> Result<()> {
    let today = Local::now().date_naive();
    let mut rendered = String::new();
    let ok = write!(rendered, "{}", today.format(format)).is_ok()
        && NaiveDate::parse_from_str(rendered.trim(), format) == Ok(today);
    if ok {
        Ok(())
    } else {
        Err(RecapError::MissingConfig(format!(
            "DATE_FORMAT '{format}' must render a date that can be parsed back unambiguously \
             (include day, month and year, e.g. {DEFAULT_DATE_FORMAT} or %Y-%m-%d)"
        )))
    }
}

/// `SUMMARIZER_PROVIDER` from the environment: unset or blank means the
/// default; anything else must name a known provider (case-insensitive).
fn provider_from_env(raw: Option<String>) -> Result<Provider> {
    match raw {
        None => Ok(Provider::default()),
        Some(raw) => raw
            .parse()
            .map_err(|e| RecapError::MissingConfig(format!("SUMMARIZER_PROVIDER: {e}"))),
    }
}

/// Blank values (e.g. `OLLAMA_MODEL=` in `.env`) count as unset.
fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn required(value: Option<&str>, name: &str) -> Result<String> {
    non_empty(value).ok_or_else(|| RecapError::MissingConfig(name.to_string()))
}

/// Expand a leading `~` or `~/` to the user's home directory.
/// Any other path (including `~user` forms) is returned unchanged.
pub fn expand_tilde(path: &Path) -> PathBuf {
    let Some(path_str) = path.to_str() else {
        return path.to_path_buf();
    };
    let Some(home) = dirs::home_dir() else {
        return path.to_path_buf();
    };
    match path_str {
        "~" => home,
        s => match s.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => path.to_path_buf(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::collections::HashMap;

    /// A CLI with every clap-env-backed flag given explicitly, so the parse
    /// does not depend on the real process environment.
    fn cli(extra: &[&str]) -> Cli {
        let mut args = vec![
            "test",
            "--token",
            "tok",
            "--username",
            "user",
            "--claude-model",
            "c-model",
            "--ollama-model",
            "o-model",
            "--openai-model",
            "oa-model",
            "--gemini-model",
            "g-model",
        ];
        args.extend_from_slice(extra);
        args.push("config");
        Cli::parse_from(args)
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    #[test]
    fn expand_tilde_bare() {
        assert_eq!(expand_tilde(Path::new("~")), dirs::home_dir().unwrap());
    }

    #[test]
    fn expand_tilde_slash() {
        let expected = dirs::home_dir().unwrap().join("worklog.md");
        assert_eq!(expand_tilde(Path::new("~/worklog.md")), expected);
    }

    #[test]
    fn expand_tilde_leaves_other_paths_alone() {
        for p in [
            "/tmp/worklog.md",
            "relative/path.md",
            "~user/file.md",
            "a~b",
        ] {
            assert_eq!(expand_tilde(Path::new(p)), PathBuf::from(p));
        }
    }

    #[test]
    fn build_applies_flags_env_and_defaults() {
        let settings = Settings::build(
            &cli(&["--provider", "ollama", "--output", "/tmp/out.md"]),
            env(&[("OLLAMA_URL", "http://box:11434"), ("MAX_TOKENS", "999")]),
        )
        .unwrap();
        assert_eq!(settings.github_token, "tok");
        assert_eq!(settings.github_username, "user");
        assert_eq!(settings.output_file, PathBuf::from("/tmp/out.md"));
        assert_eq!(settings.summarizer_provider, Provider::Ollama);
        assert_eq!(settings.claude_model, "c-model");
        assert_eq!(settings.ollama_url, "http://box:11434");
        assert_eq!(settings.max_tokens, 999);
        assert_eq!(settings.date_format, DEFAULT_DATE_FORMAT);
        assert!(settings.anthropic_api_key.is_none());
    }

    #[test]
    fn env_fallbacks_are_lenient() {
        let settings = Settings::build(
            &cli(&[]),
            env(&[
                ("OUTPUT_FILE", "  "),
                ("SUMMARIZER_PROVIDER", "Gemini"),
                ("MAX_TOKENS", "0"),
                ("GEMINI_API_KEY", "k"),
            ]),
        )
        .unwrap();
        assert_eq!(settings.output_file, PathBuf::from(DEFAULT_OUTPUT_FILE));
        assert_eq!(settings.summarizer_provider, Provider::Gemini);
        assert_eq!(settings.max_tokens, DEFAULT_MAX_TOKENS);
        assert_eq!(settings.gemini_api_key.as_deref(), Some("k"));
    }

    #[test]
    fn blank_model_flag_falls_back_to_default() {
        let c = Cli::parse_from([
            "test",
            "--token",
            "t",
            "--username",
            "u",
            "--claude-model",
            "  ",
            "config",
        ]);
        let settings = Settings::build(&c, env(&[])).unwrap();
        assert_eq!(settings.claude_model, DEFAULT_CLAUDE_MODEL);
    }

    #[test]
    fn bad_provider_and_date_format_are_reported() {
        let err = Settings::build(&cli(&[]), env(&[("SUMMARIZER_PROVIDER", "bard")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("SUMMARIZER_PROVIDER"), "{err}");

        let err = Settings::build(&cli(&[]), env(&[("DATE_FORMAT", "%d %b")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("DATE_FORMAT"), "{err}");
    }

    #[test]
    fn missing_token_is_reported() {
        let c = Cli::parse_from(["test", "--username", "u", "config"]);
        if c.token.is_none() {
            let err = Settings::build(&c, env(&[])).unwrap_err().to_string();
            assert_eq!(err, "Missing required configuration: GITHUB_TOKEN");
        }
    }

    #[test]
    fn date_format_must_round_trip() {
        for ok in [
            "%d/%m/%y",
            "%Y-%m-%d",
            "%d %b %Y",
            "%A %d %B %Y",
            "%m/%d/%Y",
        ] {
            assert!(validate_date_format(ok).is_ok(), "{ok}");
        }
        for bad in ["%d %b", "%d/%m", "%A", "%Q", "", "plain text"] {
            assert!(validate_date_format(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn provider_env_is_lenient_about_blank_and_case() {
        assert_eq!(provider_from_env(None).unwrap(), Provider::Claude);
        assert_eq!(
            provider_from_env(Some("Ollama".into())).unwrap(),
            Provider::Ollama
        );
        assert_eq!(
            provider_from_env(Some("anthropic".into())).unwrap(),
            Provider::Claude
        );
        assert!(provider_from_env(Some("bard".into())).is_err());
    }
}
