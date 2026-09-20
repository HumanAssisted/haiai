#!/usr/bin/env bash
#
# check_knowledge_freshness.sh — Verify embedded content and its index are current.
#
# Regenerates knowledge and compares with the working tree, restoring it on exit.
# Exits non-zero if either content or index differs after a source doc change
# without re-running ./scripts/generate_knowledge.sh.
#
# Usage:
#   ./scripts/check_knowledge_freshness.sh          # local check
#   make check-knowledge                             # via Makefile
#
# Requirements:
#   - JACS_ROOT or sibling ../JACS repo (same as generate_knowledge.sh)
#
# What to do if this fails:
#   1. Run: ./scripts/generate_knowledge.sh
#   2. Review the diff in rust/haiai/src/self_knowledge_data.rs
#   3. Commit the updated file

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_FILE="$REPO_ROOT/rust/haiai/src/self_knowledge_data.rs"
KNOWLEDGE_DIR="$REPO_ROOT/rust/haiai/docs/knowledge"

if [ ! -f "$DATA_FILE" ]; then
    echo "ERROR: $DATA_FILE does not exist. Run ./scripts/generate_knowledge.sh first." >&2
    exit 1
fi

# Save and restore current state even if generation fails.
SNAPSHOT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/haiai-knowledge.XXXXXX")"
cp "$DATA_FILE" "$SNAPSHOT_DIR/self_knowledge_data.rs"
cp -R "$KNOWLEDGE_DIR" "$SNAPSHOT_DIR/knowledge"
restore_knowledge() {
    cp "$SNAPSHOT_DIR/self_knowledge_data.rs" "$DATA_FILE"
    rm -rf "$KNOWLEDGE_DIR"
    cp -R "$SNAPSHOT_DIR/knowledge" "$KNOWLEDGE_DIR"
    rm -rf "$SNAPSHOT_DIR"
}
trap restore_knowledge EXIT

# Regenerate
"$REPO_ROOT/scripts/generate_knowledge.sh" > /dev/null

# Compare
if diff -q "$SNAPSHOT_DIR/self_knowledge_data.rs" "$DATA_FILE" > /dev/null 2>&1 &&
   diff -qr "$SNAPSHOT_DIR/knowledge" "$KNOWLEDGE_DIR" > /dev/null 2>&1; then
    echo "Embedded knowledge content and index are up to date."
    exit 0
else
    echo "ERROR: embedded knowledge content or index is stale." >&2
    echo "" >&2
    echo "Diff (first 40 lines):" >&2
    diff -u "$SNAPSHOT_DIR/self_knowledge_data.rs" "$DATA_FILE" | head -40 >&2 || true
    diff -qr "$SNAPSHOT_DIR/knowledge" "$KNOWLEDGE_DIR" | head -40 >&2 || true
    echo "" >&2
    echo "Fix: run ./scripts/generate_knowledge.sh and commit the result." >&2
    exit 1
fi
