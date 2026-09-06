# Default recipe - show available commands
default:
    @just --list

# Build release binary
build:
    cargo build --release

# Run tests
test:
    cargo test

# Preview today's recap without saving
preview:
    ./target/release/github-worklog today --preview

# Generate today's recap
today:
    ./target/release/github-worklog today

# Generate recap for a specific date (usage: just generate 2026-01-07)
generate date:
    ./target/release/github-worklog generate --date {{ date }}

# Generate recaps for the past week
week:
    #!/usr/bin/env bash
    for i in {0..6}; do
        ./target/release/github-worklog generate --date $(date -v-${i}d +%Y-%m-%d)
    done

# Show current configuration
config:
    ./target/release/github-worklog config

# Show setup instructions
init:
    ./target/release/github-worklog init

# Install the daily scheduler (macOS launchd)
install-scheduler:
    ./scripts/install-scheduler.sh

# Uninstall the daily scheduler
uninstall-scheduler:
    launchctl bootout gui/$(id -u)/com.github-worklog 2>/dev/null || true
    rm -f ~/Library/LaunchAgents/com.github-worklog.plist

# Check if scheduler is running
scheduler-status:
    @launchctl print gui/$(id -u)/com.github-worklog >/dev/null 2>&1 && echo "Scheduler loaded" || echo "Scheduler not running"

# View scheduler logs
logs:
    @cat ~/.local/log/worklog-stdout.log 2>/dev/null || echo "No logs found"

# Run the scheduled job manually (generates yesterday's recap with retries)
cron *args:
    ./scripts/worklog-cron.sh {{ args }}

# Pull the configured Ollama model (reads OLLAMA_MODEL from .env, default gemma4:e4b)
ollama-pull:
    #!/usr/bin/env bash
    set -a; [[ -f .env ]] && source .env; set +a
    ollama pull "${OLLAMA_MODEL:-gemma4:e4b}"

# Clean build artifacts
clean:
    cargo clean

# Run clippy linter
lint:
    cargo clippy --all-targets -- -D warnings

# Format code
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check

# Run all checks (format, lint, tests)
check-all:
    just fmt-check
    just lint
    just test

# Install git hooks
install-hooks:
    ./scripts/install-hooks.sh
