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
    ./target/release/github-worklog generate --date {{date}}

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
    launchctl unload ~/Library/LaunchAgents/com.github-worklog.plist

# Check if scheduler is running
scheduler-status:
    @launchctl list | grep worklog || echo "Scheduler not running"

# View scheduler logs
logs:
    @cat ~/.local/log/worklog-stdout.log 2>/dev/null || echo "No logs found"

# Run the cron script manually (generates + commits + pushes)
cron:
    ./scripts/worklog-cron.sh

# Clean build artifacts
clean:
    cargo clean

# Run clippy linter
lint:
    cargo clippy -- -D warnings

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
