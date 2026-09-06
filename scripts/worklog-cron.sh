#!/usr/bin/env bash

# GitHub Worklog - scheduled run.
# Generates the recap for the previous day and prepends it to OUTPUT_FILE.
# Invoked by launchd (see install-scheduler.sh); safe to run by hand.
# Extra arguments are passed to the binary, e.g. `worklog-cron.sh --preview`
# or `worklog-cron.sh --date 2026-01-07` (which then replaces "yesterday").
#
# When the provider is ollama and no Ollama server is reachable at OLLAMA_URL
# (a loopback address), one is started for the duration of the run and
# stopped again afterwards, so the midnight job works even if the Ollama app
# is not running.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR" || exit 1

# The binary loads .env itself; this is so the values are visible to this
# script. Variables already set in the environment win over .env, matching
# the binary's behaviour, so `GITHUB_TOKEN=... worklog-cron.sh` works.
if [[ -f .env ]]; then
    ENV_BEFORE="$(export -p)"
    # .env may reference variables that launchd does not set; that is fine
    # (the binary treats them as empty too), so relax -u while sourcing.
    set -a +u
    # shellcheck disable=SC1091
    source .env
    set +a -u
    eval "$ENV_BEFORE"
fi

# Flags forwarded to the binary that this script also needs to know about.
PROVIDER="${SUMMARIZER_PROVIDER:-claude}"
OUTPUT="${OUTPUT_FILE:-./worklog.md}"
USER_DATE=""
prev=""
for arg in "$@"; do
    case "$prev" in
        --provider|-p) PROVIDER="$arg" ;;
        --output|-o)   OUTPUT="$arg" ;;
        --date|-d)     USER_DATE="$arg" ;;
    esac
    case "$arg" in
        --provider=*) PROVIDER="${arg#*=}" ;;
        --output=*)   OUTPUT="${arg#*=}" ;;
        --date=*)     USER_DATE="${arg#*=}" ;;
    esac
    prev="$arg"
done
PROVIDER="$(printf '%s' "$PROVIDER" | tr '[:upper:]' '[:lower:]')"

MAX_RETRIES=3
RETRY_DELAY=30
OLLAMA_URL="${OLLAMA_URL:-http://localhost:11434}"
OLLAMA_PID=""

# Only add our own --date when the caller did not supply one.
DATE_ARGS=()
if [[ -n "$USER_DATE" ]]; then
    TARGET_DATE="$USER_DATE"
else
    TARGET_DATE="$(date -v-1d +%Y-%m-%d)"
    DATE_ARGS=(--date "$TARGET_DATE")
fi

log() { echo "$(date): $*"; }
err() { echo "$(date): $*" >&2; }

ollama_up() { curl -sf --max-time 2 "$OLLAMA_URL/api/tags" >/dev/null 2>&1; }

# host:port from OLLAMA_URL, e.g. http://localhost:11500/ -> localhost:11500
ollama_hostport() {
    local hp="${OLLAMA_URL#*://}"
    hp="${hp%%/*}"
    printf '%s' "$hp"
}

ollama_is_local() {
    case "$(ollama_hostport)" in
        localhost*|127.0.0.1*|0.0.0.0*|\[::1\]*) return 0 ;;
        *) return 1 ;;
    esac
}

start_ollama_if_needed() {
    [[ "$PROVIDER" == "ollama" ]] || return 0
    ollama_up && return 0
    if ! ollama_is_local; then
        err "WARNING - Ollama at $OLLAMA_URL is not reachable and is not local, so it cannot be started here"
        return 0
    fi
    if ! command -v ollama >/dev/null; then
        err "WARNING - SUMMARIZER_PROVIDER=ollama but the ollama binary is not on PATH; summaries will be skipped"
        return 0
    fi
    log "Ollama not running; starting a temporary server on $(ollama_hostport)"
    OLLAMA_HOST="$(ollama_hostport)" ollama serve >/dev/null 2>&1 &
    OLLAMA_PID=$!
    for _ in $(seq 1 30); do
        ollama_up && { log "Ollama ready (pid $OLLAMA_PID)"; return 0; }
        sleep 1
    done
    err "WARNING - Ollama did not become ready in 30s; continuing without it"
}

stop_ollama_if_started() {
    if [[ -n "$OLLAMA_PID" ]]; then
        log "Stopping temporary Ollama server (pid $OLLAMA_PID)"
        kill "$OLLAMA_PID" 2>/dev/null || true
        wait "$OLLAMA_PID" 2>/dev/null || true
    fi
}
trap stop_ollama_if_started EXIT

start_ollama_if_needed

for attempt in $(seq 1 "$MAX_RETRIES"); do
    ./target/release/github-worklog generate ${DATE_ARGS[@]+"${DATE_ARGS[@]}"} "$@"
    EXIT_CODE=$?
    if (( EXIT_CODE == 0 )); then
        if [[ " $* " == *" --preview "* ]]; then
            log "Preview for $TARGET_DATE complete (nothing written)"
        else
            log "Recap for $TARGET_DATE saved to $OUTPUT"
        fi
        exit 0
    fi
    # Exit code 2 means configuration, credentials or usage: retrying cannot help.
    if (( EXIT_CODE == 2 )); then
        err "ERROR - Configuration or usage error (exit code 2) for date $TARGET_DATE; not retrying"
        exit 2
    fi
    err "ERROR - Attempt $attempt/$MAX_RETRIES failed with exit code $EXIT_CODE for date $TARGET_DATE"
    if (( attempt < MAX_RETRIES )); then
        err "Retrying in ${RETRY_DELAY}s..."
        sleep "$RETRY_DELAY"
    fi
done

err "FAILED - All $MAX_RETRIES attempts failed for date $TARGET_DATE"
exit 1
