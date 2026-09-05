#!/usr/bin/env bash

# Install script for the GitHub Worklog scheduler.
# Sets up a launchd agent that runs the recap for the previous day at midnight.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
LABEL="com.github-worklog"
PLIST_NAME="$LABEL.plist"
LAUNCH_AGENTS_DIR="$HOME/Library/LaunchAgents"
PLIST_PATH="$LAUNCH_AGENTS_DIR/$PLIST_NAME"
LOG_DIR="$HOME/.local/log"
DOMAIN="gui/$(id -u)"

echo "GitHub Worklog - Scheduler Installation"
echo "========================================"
echo ""
echo "Project directory: $PROJECT_DIR"
echo ""

if [[ ! -x "$PROJECT_DIR/target/release/github-worklog" ]]; then
    echo "Error: Binary not found. Please run 'just build' first." >&2
    exit 1
fi

if [[ ! -f "$PROJECT_DIR/.env" ]]; then
    echo "Error: .env file not found. Please copy .env.example to .env and configure it." >&2
    exit 1
fi

mkdir -p "$LOG_DIR" "$LAUNCH_AGENTS_DIR"

# Generate plist from template
SCRIPT_PATH="$SCRIPT_DIR/worklog-cron.sh"
sed -e "s|{{SCRIPT_PATH}}|$SCRIPT_PATH|g" \
    -e "s|{{LOG_DIR}}|$LOG_DIR|g" \
    "$SCRIPT_DIR/$PLIST_NAME.template" > "$PLIST_PATH"

echo "Generated launchd plist at: $PLIST_PATH"

# Replace any previously loaded copy, then load the new one.
launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true
launchctl bootstrap "$DOMAIN" "$PLIST_PATH"

echo ""
echo "Installation complete!"
echo ""
echo "The recap for the previous day will run automatically every day at midnight."
echo ""
echo "Useful commands:"
echo "  Check status:    launchctl print $DOMAIN/$LABEL"
echo "  View logs:       cat $LOG_DIR/worklog-stdout.log"
echo "  Run manually:    $SCRIPT_PATH"
echo "  Uninstall:       just uninstall-scheduler"
echo ""
