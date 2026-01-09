#!/bin/bash

# Daily GitHub Recap - Automated Script
# Runs the recap tool and commits changes to git

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
./target/release/github-daily-recap today

# Check if there are changes to commit
if [[ -n $(git status --porcelain daily-recap.md 2>/dev/null) ]]; then
    git add daily-recap.md
    git commit -m "Daily recap for $(date +%Y-%m-%d)"
    git push origin main
    echo "$(date): Recap generated and pushed to GitHub"
else
    echo "$(date): No changes to commit"
fi
