#!/usr/bin/env bash
set -euo pipefail

# Validate that MCP tool and CLI command parity fixtures are structurally sound.
# This runs before Rust compilation as a fast-fail check.

ROOT="${1:-.}"
cd "$ROOT"

status=0

# ── MCP tool contract ────────────────────────────────────────────
MCP_FIXTURE="fixtures/mcp_tool_contract.json"

if [ ! -f "$MCP_FIXTURE" ]; then
  echo "FAIL: $MCP_FIXTURE not found"
  status=1
else
  # Validate JSON
  if ! python3 -c "import json, sys; json.load(open(sys.argv[1]))" "$MCP_FIXTURE" 2>/dev/null; then
    echo "FAIL: $MCP_FIXTURE is not valid JSON"
    status=1
  else
    DECLARED=$(python3 -c "import json; d=json.load(open('$MCP_FIXTURE')); print(d.get('total_tool_count', -1))")
    ACTUAL=$(python3 -c "import json; d=json.load(open('$MCP_FIXTURE')); print(len(d['required_tools']))")

    if [ "$DECLARED" = "-1" ]; then
      echo "FAIL: $MCP_FIXTURE missing total_tool_count field"
      status=1
    elif [ "$DECLARED" != "$ACTUAL" ]; then
      echo "FAIL: $MCP_FIXTURE total_tool_count ($DECLARED) != actual tool count ($ACTUAL)"
      status=1
    else
      echo "OK: $MCP_FIXTURE has $ACTUAL tools (total_tool_count matches)"
    fi
  fi
fi

# ── CLI command contract ─────────────────────────────────────────
CLI_FIXTURE="fixtures/cli_command_parity.json"

if [ ! -f "$CLI_FIXTURE" ]; then
  echo "SKIP: $CLI_FIXTURE not found (optional)"
else
  if ! python3 -c "import json, sys; json.load(open(sys.argv[1]))" "$CLI_FIXTURE" 2>/dev/null; then
    echo "FAIL: $CLI_FIXTURE is not valid JSON"
    status=1
  else
    DECLARED=$(python3 -c "import json; d=json.load(open('$CLI_FIXTURE')); print(d.get('total_command_count', -1))")
    ACTUAL=$(python3 -c "import json; d=json.load(open('$CLI_FIXTURE')); print(len(d['commands']))")

    if [ "$DECLARED" = "-1" ]; then
      echo "FAIL: $CLI_FIXTURE missing total_command_count field"
      status=1
    elif [ "$DECLARED" != "$ACTUAL" ]; then
      echo "FAIL: $CLI_FIXTURE total_command_count ($DECLARED) != actual command count ($ACTUAL)"
      status=1
    else
      echo "OK: $CLI_FIXTURE has $ACTUAL commands (total_command_count matches)"
    fi
  fi
fi

if [ "$status" -ne 0 ]; then
  echo ""
  echo "Parity fixture validation failed. Update the fixture files to match."
  exit 1
fi

echo "Parity fixture validation passed."
