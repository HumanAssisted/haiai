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
NODE_WASM_INDEX="node-wasm/index.ts"

if [[ ! -f "${FIXTURE}" ]]; then
  echo "ERROR: fixture not found at ${FIXTURE}" >&2
  exit 2
fi
if [[ ! -f "${CLIENT_RS}" ]]; then
  echo "ERROR: ${CLIENT_RS} not found" >&2
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

  # 1) Rust HaiClient method exists when rust_fn is set.
  if [[ -n "${RUST_FN}" ]]; then
    # Accept either `pub async fn <name>` or `pub fn <name>` (local
    # helpers like build_auth_header / canonical_json are sync).
    if ! grep -qE "^[[:space:]]*pub (async )?fn ${RUST_FN}\b" "${CLIENT_RS}"; then
      missing_rust+=("${JS_NAME} (rust_fn=${RUST_FN})")
      status=1
    fi
  fi

  # 2) TS wrapper check — only when node-wasm/index.ts exists. Tasks
  # 031-034 land the TS wrapper; until then this is silently skipped
  # so the enforcer can run green during the Rust-only landing waves.
  if [[ -f "${NODE_WASM_INDEX}" ]]; then
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
  echo "ERROR: ${#missing_rust[@]} fixture entries have no matching HaiClient method in ${CLIENT_RS}:" >&2
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

echo "check_wasm_surface: ${FIXTURE} <-> ${CLIENT_RS}$([[ -f \"${NODE_WASM_INDEX}\" ]] && echo \" + ${NODE_WASM_INDEX}\") is in sync."
