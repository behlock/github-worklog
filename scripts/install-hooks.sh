#!/usr/bin/env bash

# Install git hooks for the project

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
HOOKS_DIR="$PROJECT_DIR/.git/hooks"

echo "Installing git hooks..."
mkdir -p "$HOOKS_DIR"

cat > "$HOOKS_DIR/pre-commit" << 'EOF_HOOK'
#!/usr/bin/env bash

# Pre-commit hook: verify formatting, lints and tests.
#
# Check-only on purpose. An auto-fixing hook has to rewrite files and re-stage
# them, which sweeps unstaged hunks of partially staged files into the commit
# and reformats unrelated dirty files behind your back. If a check fails, run
# `just fmt` (or `cargo clippy --fix`), review the result, stage it, and
# commit again.

set -euo pipefail

echo "Checking formatting..."
if ! cargo fmt -- --check >/dev/null 2>&1; then
    echo "Formatting differs. Run 'just fmt', review, stage, and commit again." >&2
    exit 1
fi

echo "Running clippy..."
cargo clippy --all-targets -- -D warnings

echo "Running tests..."
cargo test

echo "All checks passed!"
EOF_HOOK

chmod +x "$HOOKS_DIR/pre-commit"

echo "Git hooks installed successfully!"
