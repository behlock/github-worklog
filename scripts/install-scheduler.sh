#!/bin/bash

# Install script for GitHub Daily Recap scheduler
# Sets up launchd to run the recap daily at midnight

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PLIST_NAME="com.github-daily-recap.plist"
LAUNCH_AGENTS_DIR="$HOME/Library/LaunchAgents"
LOG_DIR="$HOME/.local/log"

echo "GitHub Daily Recap - Scheduler Installation"
echo "============================================"
echo ""
echo "Project directory: $PROJECT_DIR"
echo ""

# Check if binary exists
if [[ ! -f "$PROJECT_DIR/target/release/github-daily-recap" ]]; then
    echo "Error: Binary not found. Please run 'cargo build --release' first."
    exit 1
fi

# Check if .env exists
if [[ ! -f "$PROJECT_DIR/.env" ]]; then
    echo "Error: .env file not found. Please copy .env.example to .env and configure it."
    exit 1
fi

# Create log directory
mkdir -p "$LOG_DIR"

# Create LaunchAgents directory if it doesn't exist
mkdir -p "$LAUNCH_AGENTS_DIR"

# Generate plist from template
SCRIPT_PATH="$SCRIPT_DIR/daily-recap-cron.sh"
sed -e "s|{{SCRIPT_PATH}}|$SCRIPT_PATH|g" \
    -e "s|{{LOG_DIR}}|$LOG_DIR|g" \
    "$SCRIPT_DIR/com.github-daily-recap.plist.template" > "$LAUNCH_AGENTS_DIR/$PLIST_NAME"

echo "Generated launchd plist at: $LAUNCH_AGENTS_DIR/$PLIST_NAME"

# Unload if already loaded
launchctl unload "$LAUNCH_AGENTS_DIR/$PLIST_NAME" 2>/dev/null || true

# Load the new plist
launchctl load "$LAUNCH_AGENTS_DIR/$PLIST_NAME"

echo ""
echo "Installation complete!"
echo ""
echo "The recap will run automatically every day at midnight."
echo ""
echo "Useful commands:"
echo "  Check status:    launchctl list | grep daily-recap"
echo "  View logs:       cat $LOG_DIR/daily-recap-stdout.log"
echo "  Run manually:    $SCRIPT_DIR/daily-recap-cron.sh"
echo "  Uninstall:       launchctl unload $LAUNCH_AGENTS_DIR/$PLIST_NAME"
echo ""
