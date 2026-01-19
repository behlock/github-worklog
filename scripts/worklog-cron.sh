#!/bin/bash

# Daily GitHub Recap - Automated Script
# Runs the recap tool and saves to iCloud Drive

set -e

# Get the directory where this script lives, then go to project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

# Load environment variables (handles quoted values with spaces)
if [[ -f .env ]]; then
    set -a
    source .env
    set +a
fi

# Run the recap tool for yesterday (since this runs at midnight)
./target/release/github-worklog generate --date "$(date -v-1d +%Y-%m-%d)"

echo "$(date): Recap generated and saved to $OUTPUT_FILE"
