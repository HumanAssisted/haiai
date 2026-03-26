#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/bump-version.sh [major|minor|patch]
# Bumps the HAIAI SDK version across all packages.
# See CLAUDE.md for the full list of files that must stay in sync.

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

BUMP_TYPE="${1:-}"
if [[ ! "$BUMP_TYPE" =~ ^(major|minor|patch)$ ]]; then
  echo "Usage: $0 [major|minor|patch]"
  echo ""
  echo "  major  — X.0.0  (breaking changes)"
  echo "  minor  — 0.X.0  (new features)"
  echo "  patch  — 0.0.X  (bug fixes)"
  exit 1
fi

# --- Read current version from canonical source ---

CURRENT=$(grep '^version' rust/haiai/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

case "$BUMP_TYPE" in
  major) NEW_VERSION="$((MAJOR + 1)).0.0" ;;
  minor) NEW_VERSION="${MAJOR}.$((MINOR + 1)).0" ;;
  patch) NEW_VERSION="${MAJOR}.${MINOR}.$((PATCH + 1))" ;;
esac

echo "HAIAI SDK: $CURRENT -> $NEW_VERSION"
echo ""

# --- Rust crate package versions ---

echo "Rust crates:"

RUST_CARGO_FILES=(
  rust/haiai/Cargo.toml
  rust/haiai-cli/Cargo.toml
  rust/hai-mcp/Cargo.toml
  rust/hai-binding-core/Cargo.toml
  rust/haiinpm/Cargo.toml
  rust/haiipy/Cargo.toml
  rust/haiigo/Cargo.toml
)

for f in "${RUST_CARGO_FILES[@]}"; do
  sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW_VERSION\"/" "$f"
  echo "  $f: package version"
done

# --- Rust inter-crate dependency versions (pinned =X.Y.Z) ---

echo ""
echo "Rust inter-crate dependencies:"

RUST_DEP_FILES=(
  rust/haiai-cli/Cargo.toml
  rust/hai-mcp/Cargo.toml
)

for f in "${RUST_DEP_FILES[@]}"; do
  sed -i '' "s/version = \"=$CURRENT\"/version = \"=$NEW_VERSION\"/g" "$f"
  echo "  $f: pinned deps"
done

# --- Python ---

echo ""
echo "Python:"
sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW_VERSION\"/" python/pyproject.toml
sed -i '' "s/\"haiipy>=.*\"/\"haiipy>=$NEW_VERSION\"/" python/pyproject.toml
echo "  python/pyproject.toml"
# Safety net: update __init__.py fallback version if it contains a hardcoded version string
sed -i '' "s/__version__ = \"$CURRENT\"/__version__ = \"$NEW_VERSION\"/" python/src/haiai/__init__.py 2>/dev/null || true
echo "  python/src/haiai/__init__.py (fallback version)"

# --- haiinpm package.json (napi-rs native binding) ---

echo ""
echo "haiinpm:"
sed -i '' "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW_VERSION\"/" rust/haiinpm/package.json
echo "  rust/haiinpm/package.json: version"

# --- haiipy pyproject.toml (PyO3 native binding) ---

echo ""
echo "haiipy:"
sed -i '' "s/^version = \"$CURRENT\"/version = \"$NEW_VERSION\"/" rust/haiipy/pyproject.toml
echo "  rust/haiipy/pyproject.toml: version"

# --- Node main package ---

echo ""
echo "Node:"
sed -i '' "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW_VERSION\"/" node/package.json
echo "  node/package.json: version"

# --- Node haiinpm dependency version ---

sed -i '' "s/\"haiinpm\": \"$CURRENT\"/\"haiinpm\": \"$NEW_VERSION\"/" node/package.json
echo "  node/package.json: haiinpm dependency"

# --- Node optionalDependencies (CLI platform binary refs in main package.json) ---

sed -i '' "s/\"@haiai\/cli-\(.*\)\": \"$CURRENT\"/\"@haiai\/cli-\1\": \"$NEW_VERSION\"/g" node/package.json
echo "  node/package.json: optionalDependencies"

# --- Node CLI platform binary packages ---
# These are easy to forget! They live under node/npm/@haiai/cli-*/package.json

NODE_PLATFORM_PACKAGES=(
  node/npm/@haiai/cli-darwin-arm64/package.json
  node/npm/@haiai/cli-darwin-x64/package.json
  node/npm/@haiai/cli-linux-arm64/package.json
  node/npm/@haiai/cli-linux-x64/package.json
  node/npm/@haiai/cli-win32-x64/package.json
)

for f in "${NODE_PLATFORM_PACKAGES[@]}"; do
  if [ -f "$f" ]; then
    sed -i '' "s/\"version\": \"[0-9]*\.[0-9]*\.[0-9]*\"/\"version\": \"$NEW_VERSION\"/" "$f"
    echo "  $f"
  fi
done

# --- Claude plugin ---

echo ""
echo "Plugin:"
sed -i '' "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW_VERSION\"/" .claude-plugin/plugin.json
echo "  .claude-plugin/plugin.json"

# --- Regenerate lockfiles ---

echo ""
echo "Regenerating Cargo.lock..."
(cd rust && cargo generate-lockfile 2>/dev/null) || echo "  (skipped — cargo not available or crate not yet published)"

echo ""
echo "Regenerating package-lock.json..."
(cd node && npm install --package-lock-only 2>/dev/null) || echo "  (skipped — npm not available or package not yet published)"

# --- Verify ---

echo ""
echo "Verifying..."
make check-versions

echo ""
echo "Done! All versions bumped to $NEW_VERSION."
echo ""
echo "NOTE: Node platform binary packages (node/npm/@haiai/cli-*) were also bumped."
echo "      These are not checked by 'make check-versions' but must match."
echo ""
echo "Next steps:"
echo "  1. git add -A && git commit -m 'Bump version to $NEW_VERSION'"
echo "  2. git push"
echo "  3. make release-all"
