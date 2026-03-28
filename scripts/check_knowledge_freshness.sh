#!/usr/bin/env bash
#
# check_knowledge_freshness.sh — Verify that self_knowledge_data.rs is up to date.
#
# Regenerates the knowledge data into a temp file and diffs against the committed
# version. Exits non-zero if they differ, meaning someone changed a source doc
# without re-running ./scripts/generate_knowledge.sh.
#
# Usage:
#   ./scripts/check_knowledge_freshness.sh          # local check
#   make check-knowledge                             # via Makefile
#
# Requirements:
#   - Sibling ../JACS repo (same as generate_knowledge.sh)
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

# Save current state
cp "$DATA_FILE" "$DATA_FILE.bak"
cp -r "$KNOWLEDGE_DIR" "$KNOWLEDGE_DIR.bak"

# Regenerate
"$REPO_ROOT/scripts/generate_knowledge.sh" > /dev/null 2>&1

# Compare
if diff -q "$DATA_FILE.bak" "$DATA_FILE" > /dev/null 2>&1; then
    echo "self_knowledge_data.rs is up to date."
    rm -f "$DATA_FILE.bak"
    rm -rf "$KNOWLEDGE_DIR.bak"
    exit 0
else
    echo "ERROR: self_knowledge_data.rs is stale." >&2
    echo "" >&2
    echo "Diff (first 40 lines):" >&2
    diff -u "$DATA_FILE.bak" "$DATA_FILE" | head -40 >&2 || true
    echo "" >&2
    echo "Fix: run ./scripts/generate_knowledge.sh and commit the result." >&2
    # Restore original so working tree stays clean
    mv "$DATA_FILE.bak" "$DATA_FILE"
    rm -rf "$KNOWLEDGE_DIR"
    mv "$KNOWLEDGE_DIR.bak" "$KNOWLEDGE_DIR"
    exit 1
fi
