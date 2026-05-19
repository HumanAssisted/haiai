#!/usr/bin/env bash
#
# scripts/ci/check_wasm_surface.sh
#
# Enforce HAIAI_WASM_PRD §3.2 / §5.6 wasm browser surface contract.
#
# For every method in fixtures/wasm_browser_surface.json, assert:
#
#   1. kind == "http" or "stream" or "local" with rust_fn set →
#        `pub (async )?fn <rust_fn>` exists in rust/haiai/src/client.rs
#        (the wasm wrapper must wrap an existing HaiClient method —
#        PRD §9 forbids new HaiClient methods).
#
#   2. (optional, only if node-wasm/ exists) the js_name appears as a
#        method definition in node-wasm/index.ts. The TS wrapper lands
#        in Tasks 031-034 — until then the check skips silently when
#        node-wasm/index.ts is absent.
#
# Exit codes:
#   0 — all entries satisfied
#   1 — at least one drift detected
#   2 — script error (missing fixture, missing client.rs, etc.)
set -euo pipefail

ROOT="${1:-.}"
cd "${ROOT}"

FIXTURE="fixtures/wasm_browser_surface.json"
CLIENT_RS="rust/haiai/src/client.rs"
# Stream-kind entries (connectSse/connectWs) live on the wasm
# `BrowserAgentHandle`, not on the native HaiClient — verify them
# against the wasm crate instead so a missing wasm export trips the
# gate even when HaiClient has matching native methods (Issue 005).
BROWSER_AGENT_RS="rust/haiai-wasm/src/browser_agent.rs"
NODE_WASM_INDEX="node-wasm/index.ts"

if [[ ! -f "${FIXTURE}" ]]; then
  echo "ERROR: fixture not found at ${FIXTURE}" >&2
  exit 2
fi
if [[ ! -f "${CLIENT_RS}" ]]; then
  echo "ERROR: ${CLIENT_RS} not found" >&2
  exit 2
fi
if [[ ! -f "${BROWSER_AGENT_RS}" ]]; then
  echo "ERROR: ${BROWSER_AGENT_RS} not found" >&2
  exit 2
fi

# Use python (always available) to walk the JSON; portable across
# macOS bash 3.2 + Linux bash 5.x. Read via a tempfile so we can avoid
# bash 3.2's missing `mapfile`.
ENTRIES_FILE="$(mktemp -t wasm_surface_entries.XXXXXX)"
trap 'rm -f "${ENTRIES_FILE}"' EXIT
# Pipe-separated so bash `read -d` doesn't collapse consecutive empty
# fields the way it does for whitespace-class separators (tab/space).
python3 -c '
import json, sys
with open(sys.argv[1]) as f:
    data = json.load(f)
for m in data["methods"]:
    js = m["js_name"]
    rust = m.get("rust_fn") or ""
    kind = m["kind"]
    print(f"{js}|{rust}|{kind}")
' "${FIXTURE}" > "${ENTRIES_FILE}"

status=0
missing_rust=()
missing_ts=()

while IFS='|' read -r JS_NAME RUST_FN KIND; do
  [[ -z "${JS_NAME}" ]] && continue

  # 1) Rust method exists when rust_fn is set. Stream entries live on
  # the wasm-bindgen handle (`BrowserAgentHandle`), everything else on
  # the native HaiClient. Local helpers may be sync (`pub fn`) so we
  # accept both `pub async fn` and `pub fn`.
  if [[ -n "${RUST_FN}" ]]; then
    if [[ "${KIND}" == "stream" ]]; then
      TARGET_FILE="${BROWSER_AGENT_RS}"
    else
      TARGET_FILE="${CLIENT_RS}"
    fi
    if ! grep -qE "^[[:space:]]*pub (async )?fn ${RUST_FN}\b" "${TARGET_FILE}"; then
      missing_rust+=("${JS_NAME} (rust_fn=${RUST_FN}, kind=${KIND}, file=${TARGET_FILE})")
      status=1
    fi
  fi

  # 2) TS wrapper check — only when node-wasm/index.ts exists AND is
  # not still the Task 031 skeleton. Tasks 031-034 stage the TS
  # wrapper in waves; the skeleton intentionally ships only
  # initHaiaiWasm/version/about, with BrowserAgent.* throwing
  # NotImplemented. The skeleton self-identifies via the marker comment
  # below so this enforcer can skip the TS check until Task 033 lands.
  if [[ -f "${NODE_WASM_INDEX}" ]] && ! grep -q "Skeleton only — Tasks 032-034 land the" "${NODE_WASM_INDEX}"; then
    # Match `<js_name>(` or `<js_name>?(` (optional methods) anywhere
    # in the file. We're not parsing TS — a substring match catches
    # both `methodName(` declarations and `methodName({` calls; either
    # confirms the surface is present.
    if ! grep -qE "\b${JS_NAME}\s*[?]?\(" "${NODE_WASM_INDEX}"; then
      missing_ts+=("${JS_NAME}")
      status=1
    fi
  fi
done < "${ENTRIES_FILE}"

if (( ${#missing_rust[@]} > 0 )); then
  echo "ERROR: ${#missing_rust[@]} fixture entries have no matching Rust method:" >&2
  for m in "${missing_rust[@]}"; do
    echo "  - ${m}" >&2
  done
fi

if (( ${#missing_ts[@]} > 0 )); then
  echo "ERROR: ${#missing_ts[@]} fixture entries have no matching TS wrapper in ${NODE_WASM_INDEX}:" >&2
  for m in "${missing_ts[@]}"; do
    echo "  - ${m}" >&2
  done
fi

if [[ "${status}" -ne 0 ]]; then
  cat >&2 <<MSG

Fixture <-> code drift detected. Update either the fixture or the
implementation so both sides match. The fixture is the single source
of truth for the wasm browser surface (HAIAI_WASM_PRD §3.2).
MSG
  exit 1
fi

echo "check_wasm_surface: ${FIXTURE} <-> ${CLIENT_RS} + ${BROWSER_AGENT_RS}$([[ -f \"${NODE_WASM_INDEX}\" ]] && echo \" + ${NODE_WASM_INDEX}\") is in sync."
