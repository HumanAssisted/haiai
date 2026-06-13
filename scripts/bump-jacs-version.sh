#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/bump-jacs-version.sh <version>
# Bumps the JACS dependency version across all SDK packages.
# Affects: rust/haiai, rust/haiai-cli, rust/hai-mcp, python, node, CI JACS ref

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

NEW_VERSION="${1:-}"
if [[ -z "$NEW_VERSION" ]] || [[ ! "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Usage: $0 <version>"
  echo ""
  echo "  Example: $0 0.9.10"
  echo ""
  echo "  Bumps the JACS dependency across all SDK packages."
  exit 1
fi

# --- Read current version from canonical source ---

CURRENT=$(grep '^jacs ' rust/haiai/Cargo.toml | sed 's/.*"\(=*[0-9][^"]*\)".*/\1/' | sed 's/^=//')

if [ "$CURRENT" = "$NEW_VERSION" ]; then
  echo "JACS dependency is already at $NEW_VERSION — nothing to do."
  exit 0
fi

echo "JACS dependency: $CURRENT -> $NEW_VERSION"
echo ""

# --- Rust crates (caret floor X.Y.Z for jacs, jacs-mcp, jacs-binding-core, jacs-media) ---

echo "Rust crates:"

RUST_JACS_FILES=(
  rust/haiai/Cargo.toml
  rust/haiai-cli/Cargo.toml
  rust/hai-mcp/Cargo.toml
)

for f in "${RUST_JACS_FILES[@]}"; do
  # Inline-table form: jacs* = { version = "X.Y.Z", ... }
  sed -i '' -E "s|^(jacs[a-z-]* = \{ version = )\"$CURRENT\"|\1\"$NEW_VERSION\"|" "$f"
  # Plain form: jacs* = "X.Y.Z"
  sed -i '' -E "s|^(jacs[a-z-]* = )\"$CURRENT\"|\1\"$NEW_VERSION\"|" "$f"
  echo "  $f"
done

# --- Python ---

echo ""
echo "Python:"
# Handles both pinned (jacs==X.Y.Z) and range floor (jacs>=X.Y.Z,<X.Y) forms.
sed -i '' -E "s/jacs([>=])=$CURRENT/jacs\1=$NEW_VERSION/" python/pyproject.toml
echo "  python/pyproject.toml"

# --- Node ---

echo ""
echo "Node:"
CURRENT_NODE=$(grep '@hai.ai/jacs' node/package.json | head -1 | sed 's/.*: *"\(.*\)".*/\1/')
case "$CURRENT_NODE" in
  file:*)
    echo "  node/package.json: skipped (using local path: $CURRENT_NODE)"
    ;;
  *)
    sed -i '' "s/\"@hai.ai\/jacs\": \"$CURRENT\"/\"@hai.ai\/jacs\": \"$NEW_VERSION\"/" node/package.json
    echo "  node/package.json"
    ;;
esac

# --- CI JACS checkout ref ---

echo ""
echo "CI:"
sed -i '' -E "s|JACS_REF: ([^ ]*/)?v$CURRENT|JACS_REF: crate/v$NEW_VERSION|" .github/workflows/test.yml
echo "  .github/workflows/test.yml"

# --- Regenerate lockfiles ---

echo ""
echo "Regenerating Cargo.lock..."
(cd rust && cargo generate-lockfile 2>/dev/null) || echo "  (skipped — cargo not available or crate not yet published)"

echo ""
echo "Regenerating package-lock.json..."
(cd node && npm install --package-lock-only 2>/dev/null) || echo "  (skipped — npm not available or package not yet published)"

echo ""
echo "Regenerating uv.lock..."
(cd python && uv lock 2>/dev/null) || echo "  (skipped — uv not available or package not yet published)"

# --- Verify ---

echo ""
echo "Verifying..."
make check-jacs-versions

echo ""
echo "Done! JACS dependency bumped to $NEW_VERSION."
echo ""
echo "Next steps:"
echo "  1. git add -A && git commit -m 'Bump JACS dependency to $NEW_VERSION'"
echo "  2. git push"
