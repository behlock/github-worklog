#!/bin/bash

# Daily GitHub Recap - Automated Script
# Runs the recap tool and saves to iCloud Drive

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

# Retry logic for transient API failures
MAX_RETRIES=3
RETRY_DELAY=30
TARGET_DATE="$(date -v-1d +%Y-%m-%d)"

for attempt in $(seq 1 $MAX_RETRIES); do
    if ./target/release/github-worklog generate --date "$TARGET_DATE"; then
        echo "$(date): Recap generated and saved to $OUTPUT_FILE"
        exit 0
    else
        EXIT_CODE=$?
        echo "$(date): ERROR - Attempt $attempt/$MAX_RETRIES failed with exit code $EXIT_CODE for date $TARGET_DATE" >&2
        if [[ $attempt -lt $MAX_RETRIES ]]; then
            echo "$(date): Retrying in ${RETRY_DELAY}s..." >&2
            sleep $RETRY_DELAY
        fi
    fi
done

echo "$(date): FAILED - All $MAX_RETRIES attempts failed for date $TARGET_DATE" >&2
exit 1
