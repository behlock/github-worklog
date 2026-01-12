#!/bin/bash

# Install git hooks for the project

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
HOOKS_DIR="$PROJECT_DIR/.git/hooks"

echo "Installing git hooks..."

# Create pre-commit hook
cat > "$HOOKS_DIR/pre-commit" << 'EOF'
#!/bin/bash

# Pre-commit hook: fixes lint issues and runs tests

set -e

echo "Running pre-commit checks..."

echo "Formatting code..."
cargo fmt

echo "Fixing clippy warnings..."
cargo clippy --fix --allow-dirty --allow-staged -- -D warnings

echo "Re-adding formatted files..."
git add -u

echo "Running tests..."
cargo test

echo "All checks passed!"
EOF

chmod +x "$HOOKS_DIR/pre-commit"

echo "Git hooks installed successfully!"
