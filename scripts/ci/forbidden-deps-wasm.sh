#!/usr/bin/env bash
#
# scripts/ci/forbidden-deps-wasm.sh
#
# Fail closed if any forbidden dependency appears in the wasm32 dep tree
# of the requested crate. See HAIAI_WASM_PRD.md §4.9 + §4.2.1.
#
# Usage:
#   scripts/ci/forbidden-deps-wasm.sh                    # default: haiai-wasm crate
#   scripts/ci/forbidden-deps-wasm.sh haiai "--no-default-features --features wasm"
#
# Environment overrides:
#   WASM_TARGET       — default `wasm32-unknown-unknown`
#   WASM_TREE_FIXTURE — path to a file with the output of `cargo tree`; if set,
#                       the script reads from this file instead of invoking
#                       cargo. Used by the unit test
#                       (`scripts/ci/tests/forbidden-deps-wasm.test.sh`).
#
# Forbidden list rationale (PRD §4.9 / §4.2.1):
#   tokio_tungstenite hickory-resolver rustls native-tls ring keyring
#   rusqlite duckdb surrealdb sqlx mio tokio-rustls hyper-tls tempfile
#   image jacs-media
# Task 007 (audit) may extend this list once we know whether bm25 +
# html5ever are wasm-compatible.
set -euo pipefail

CRATE="${1:-haiai-wasm}"
FEATURES="${2:-}"
WASM_TARGET="${WASM_TARGET:-wasm32-unknown-unknown}"

FORBIDDEN=(
  tokio_tungstenite
  tokio-tungstenite
  hickory-resolver
  rustls
  native-tls
  ring
  keyring
  rusqlite
  duckdb
  surrealdb
  sqlx
  mio
  tokio-rustls
  hyper-tls
  tempfile
  image
  jacs-media
)

if [[ -n "${WASM_TREE_FIXTURE:-}" ]]; then
  if [[ ! -f "${WASM_TREE_FIXTURE}" ]]; then
    echo "ERROR: WASM_TREE_FIXTURE='${WASM_TREE_FIXTURE}' does not exist" >&2
    exit 2
  fi
  TREE="$(cat "${WASM_TREE_FIXTURE}")"
else
  # When the override is unset we shell out to cargo. Use `read -ra` so the
  # feature string is split safely. `${FEATURE_ARGS[@]+"${FEATURE_ARGS[@]}"}`
  # expands to nothing when the array is empty (FEATURES was unset/empty),
  # which is required under `set -u`.
  FEATURE_ARGS=()
  if [[ -n "${FEATURES}" ]]; then
    read -ra FEATURE_ARGS <<< "${FEATURES}"
  fi
  # Check the production runtime dependency graph only. `cargo tree`
  # includes dev-dependencies by default, and build-dependencies are
  # host-side (Linux/macOS) rather than wasm-shipped code.
  if ! TREE="$(cargo tree -p "${CRATE}" --target "${WASM_TARGET}" --edges normal ${FEATURE_ARGS[@]+"${FEATURE_ARGS[@]}"} 2>&1)"; then
    echo "ERROR: cargo tree -p ${CRATE} --target ${WASM_TARGET} --edges normal ${FEATURES} failed:" >&2
    echo "${TREE}" >&2
    exit 2
  fi
fi

status=0
seen=""
for crate in "${FORBIDDEN[@]}"; do
  # Match only real cargo-tree package rows:
  #   crate v...
  #   ├── crate v...
  #   │   └── crate v...
  # Do not match cargo warning/help text that merely mentions `crate v...`.
  if printf '%s\n' "${TREE}" | grep -E "^(${crate} v|[[:space:]│]*[├└]── ${crate} v)" >/dev/null 2>&1; then
    echo "FORBIDDEN: '${crate}' appears in ${CRATE} ${WASM_TARGET} tree" >&2
    seen="${seen} ${crate}"
    status=1
  fi
done

if [[ "${status}" -ne 0 ]]; then
  if [[ -z "${WASM_TREE_FIXTURE:-}" ]]; then
    echo "" >&2
    echo "Reverse dependency paths for offenders:" >&2
    for crate in ${seen}; do
      echo "--- ${crate} ---" >&2
      cargo tree -p "${CRATE}" --target "${WASM_TARGET}" --edges normal -i "${crate}" \
        ${FEATURE_ARGS[@]+"${FEATURE_ARGS[@]}"} >&2 || true
    done
  fi

  cat >&2 <<MSG

Policy violation: the listed crate(s) are not wasm-portable and must not
appear in '${CRATE}' (target ${WASM_TARGET}). See HAIAI_WASM_PRD.md §4.9
and §4.2.1. Gate the offending module behind
\`cfg(not(target_arch = "wasm32"))\` or feature-flag it out of the wasm build.

Offenders:${seen}
MSG
  exit 1
fi

echo "forbidden-deps-wasm: ${CRATE} (${WASM_TARGET}) is clean."
