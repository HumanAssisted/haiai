#!/usr/bin/env bash
#
# generate_knowledge.sh — Copies jacsbook chapters, SDK READMEs, and JACS README
# into rust/haiai/docs/knowledge/ and generates rust/haiai/src/self_knowledge_data.rs
# with include_str!() references for compile-time embedding.
#
# Usage: ./scripts/generate_knowledge.sh
#
# Run from the haisdk repo root. Requires sibling ../JACS repo for jacsbook source.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JACS_ROOT="$(cd "$REPO_ROOT/../JACS" && pwd)"
JACSBOOK_SRC="$JACS_ROOT/jacs/docs/jacsbook/src"
SUMMARY="$JACSBOOK_SRC/SUMMARY.md"
KNOWLEDGE_DIR="$REPO_ROOT/rust/haiai/docs/knowledge"
DATA_FILE="$REPO_ROOT/rust/haiai/src/self_knowledge_data.rs"

if [ ! -f "$SUMMARY" ]; then
    echo "ERROR: SUMMARY.md not found at $SUMMARY" >&2
    echo "Ensure the JACS repo is at $JACS_ROOT" >&2
    exit 1
fi

# Clean and recreate knowledge directory
rm -rf "$KNOWLEDGE_DIR"
mkdir -p "$KNOWLEDGE_DIR/jacsbook"
mkdir -p "$KNOWLEDGE_DIR/haiai-sdk"
mkdir -p "$KNOWLEDGE_DIR/jacs"

# ── Parse SUMMARY.md and copy jacsbook files ──────────────────────

declare -a JACSBOOK_PATHS=()
declare -a JACSBOOK_TITLES=()

re='\[([^]]+)\]\(([^)]+\.md)\)'
while IFS= read -r line; do
    # Match lines like: - [Title](path.md) or   - [Title](dir/path.md)
    if [[ "$line" =~ $re ]]; then
        title="${BASH_REMATCH[1]}"
        rel_path="${BASH_REMATCH[2]}"

        src_file="$JACSBOOK_SRC/$rel_path"
        if [ -f "$src_file" ]; then
            # Create destination directory structure
            dest_file="$KNOWLEDGE_DIR/jacsbook/$rel_path"
            mkdir -p "$(dirname "$dest_file")"
            cp "$src_file" "$dest_file"

            JACSBOOK_PATHS+=("$rel_path")
            JACSBOOK_TITLES+=("$title")
        else
            echo "WARN: jacsbook file not found: $src_file (skipping)" >&2
        fi
    fi
done < "$SUMMARY"

echo "Copied ${#JACSBOOK_PATHS[@]} jacsbook chapters"

# ── Copy SDK READMEs ──────────────────────────────────────────────

declare -a README_SRCS=(
    "$REPO_ROOT/README.md"
    "$REPO_ROOT/python/README.md"
    "$REPO_ROOT/node/README.md"
    "$REPO_ROOT/go/README.md"
    "$REPO_ROOT/rust/haiai/README.md"
    "$REPO_ROOT/rust/haiai-cli/README.md"
    "$REPO_ROOT/rust/hai-mcp/README.md"
    "$REPO_ROOT/fixtures/README.md"
)

declare -a README_NAMES=(
    "root.md"
    "python.md"
    "node.md"
    "go.md"
    "haiai.md"
    "haiai-cli.md"
    "hai-mcp.md"
    "fixtures.md"
)

declare -a README_TITLES=(
    "HAIAI SDK"
    "HAIAI Python SDK"
    "HAIAI Node SDK"
    "HAIAI Go SDK"
    "HAIAI Rust Library"
    "HAIAI CLI"
    "HAI MCP Server"
    "HAIAI Fixtures"
)

# ── Copy project guides ──────────────────────────────────────────

declare -a GUIDE_SRCS=(
    "$REPO_ROOT/DEVELOPMENT.md"
    "$REPO_ROOT/AGENTS.md"
    "$REPO_ROOT/CLAUDE.md"
    "$REPO_ROOT/docs/HAIAI_LANGUAGE_SYNC_GUIDE.md"
    "$REPO_ROOT/docs/haisdk/PARITY_MAP.md"
    "$REPO_ROOT/docs/adr/0001-crypto-delegation-to-jacs.md"
    "$REPO_ROOT/docs/A2A_INTEGRATION_ROADMAP.md"
    "$REPO_ROOT/docs/CLI_PARITY_AUDIT.md"
    "$REPO_ROOT/skills/jacs/SKILL.md"
)

declare -a GUIDE_NAMES=(
    "development.md"
    "agents.md"
    "claude.md"
    "language-sync-guide.md"
    "parity-map.md"
    "adr-crypto-delegation.md"
    "a2a-integration-roadmap.md"
    "cli-parity-audit.md"
    "skill-definition.md"
)

declare -a GUIDE_TITLES=(
    "Development Guide"
    "Agent Rules"
    "Project Instructions"
    "Cross-Language Sync Guide"
    "JACS Parity Map"
    "ADR: Crypto Delegation to JACS"
    "A2A Integration Roadmap"
    "CLI Parity Audit"
    "MCP Skill Definition"
)

readme_count=0
for i in "${!README_SRCS[@]}"; do
    src="${README_SRCS[$i]}"
    name="${README_NAMES[$i]}"
    if [ -f "$src" ]; then
        cp "$src" "$KNOWLEDGE_DIR/haiai-sdk/$name"
        ((readme_count++))
    else
        echo "WARN: README not found: $src (skipping)" >&2
    fi
done

echo "Copied $readme_count SDK READMEs"

# ── Copy project guides ──────────────────────────────────────────

mkdir -p "$KNOWLEDGE_DIR/haiai-guides"
guide_count=0
for i in "${!GUIDE_SRCS[@]}"; do
    src="${GUIDE_SRCS[$i]}"
    name="${GUIDE_NAMES[$i]}"
    if [ -f "$src" ]; then
        cp "$src" "$KNOWLEDGE_DIR/haiai-guides/$name"
        ((guide_count++))
    else
        echo "WARN: Guide not found: $src (skipping)" >&2
    fi
done

echo "Copied $guide_count project guides"

# ── Copy JSON schemas ────────────────────────────────────────────

mkdir -p "$KNOWLEDGE_DIR/schemas"
schema_count=0
for schema_file in "$REPO_ROOT"/schemas/*.json; do
    if [ -f "$schema_file" ]; then
        cp "$schema_file" "$KNOWLEDGE_DIR/schemas/"
        ((schema_count++))
    fi
done

echo "Copied $schema_count JSON schemas"

# ── Copy JACS root README ────────────────────────────────────────

if [ -f "$JACS_ROOT/README.md" ]; then
    cp "$JACS_ROOT/README.md" "$KNOWLEDGE_DIR/jacs/README.md"
    echo "Copied JACS root README"
else
    echo "WARN: JACS README not found at $JACS_ROOT/README.md (skipping)" >&2
fi

# ── Generate self_knowledge_data.rs ──────────────────────────────

cat > "$DATA_FILE" << 'HEADER'
//! Auto-generated by scripts/generate_knowledge.sh -- do not edit.
//! Regenerate: ./scripts/generate_knowledge.sh

/// (relative_path, title, content_via_include_str)
pub static CHAPTERS: &[(&str, &str, &str)] = &[
HEADER

# Helper to escape strings for Rust (double quotes and backslashes)
escape_rust_str() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

# Jacsbook entries
for i in "${!JACSBOOK_PATHS[@]}"; do
    path="${JACSBOOK_PATHS[$i]}"
    title="$(escape_rust_str "${JACSBOOK_TITLES[$i]}")"
    cat >> "$DATA_FILE" << ENTRY
    ("jacsbook/$path", "$title",
     include_str!("../docs/knowledge/jacsbook/$path")),
ENTRY
done

# SDK README entries
for i in "${!README_NAMES[@]}"; do
    name="${README_NAMES[$i]}"
    title="$(escape_rust_str "${README_TITLES[$i]}")"
    dest="$KNOWLEDGE_DIR/haiai-sdk/$name"
    if [ -f "$dest" ]; then
        cat >> "$DATA_FILE" << ENTRY
    ("haiai-sdk/$name", "$title",
     include_str!("../docs/knowledge/haiai-sdk/$name")),
ENTRY
    fi
done

# Project guide entries
for i in "${!GUIDE_NAMES[@]}"; do
    name="${GUIDE_NAMES[$i]}"
    title="$(escape_rust_str "${GUIDE_TITLES[$i]}")"
    dest="$KNOWLEDGE_DIR/haiai-guides/$name"
    if [ -f "$dest" ]; then
        cat >> "$DATA_FILE" << ENTRY
    ("haiai-guides/$name", "$title",
     include_str!("../docs/knowledge/haiai-guides/$name")),
ENTRY
    fi
done

# JSON schema entries
for schema_file in "$KNOWLEDGE_DIR"/schemas/*.json; do
    if [ -f "$schema_file" ]; then
        basename="$(basename "$schema_file")"
        # Derive title from filename: AgentEvent.json -> "Schema: AgentEvent"
        title_part="${basename%.json}"
        cat >> "$DATA_FILE" << ENTRY
    ("schemas/$basename", "Schema: $title_part",
     include_str!("../docs/knowledge/schemas/$basename")),
ENTRY
    fi
done

# JACS root README
if [ -f "$KNOWLEDGE_DIR/jacs/README.md" ]; then
    cat >> "$DATA_FILE" << 'ENTRY'
    ("jacs/README.md", "JACS",
     include_str!("../docs/knowledge/jacs/README.md")),
ENTRY
fi

# Close the array
echo "];" >> "$DATA_FILE"

total=$((${#JACSBOOK_PATHS[@]} + readme_count + guide_count + schema_count))
if [ -f "$KNOWLEDGE_DIR/jacs/README.md" ]; then
    ((total++))
fi

echo "Generated $DATA_FILE with $total entries"
echo "Done."
