#!/bin/bash

# Daily GitHub Recap - Automated Script
# Runs the recap tool and saves to iCloud Drive

set -e

# Get the directory where this script lives, then go to project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

# Load environment variables
if [[ -f .env ]]; then
    export $(grep -v '^#' .env | xargs)
fi

# Run the recap tool for today
./target/release/github-worklog today

echo "$(date): Recap generated and saved to $OUTPUT_FILE"
