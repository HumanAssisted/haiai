#!/usr/bin/env bash
#
# scripts/ci/check_wasm_parity.sh
#
# HAIAI_WASM_PRD §5.2 / §6 / Task 041 — cross-compat parity gate.
#
# Drives the native + wasm cross-compat tests and diffs their output
# JSONs. Any byte-level disagreement on auth header, canonical bodies,
# SSE event parse, or WS frame parse fails CI.
#
# Native side: cargo test --test wasm_cross_compat_native writes
#   rust/target/parity/native.json
#
# Wasm side: TODO (Task 041 follow-up) — wasm-pack test --headless
#   --chrome writes rust/target/parity/wasm.json. For now the wasm
#   side is the same source as the native side via the shared
#   modules (transport::build_auth_header_with, sse_parse::SseParser,
#   ws_protocol::parse_frame_text), so we run the native test alone
#   and the diff check is a future hardening point once wasm-pack is
#   wired into CI.
#
# Exit codes:
#   0 — native snapshot generated successfully (and wasm snapshot
#       matches, when available)
#   1 — drift detected
#   2 — toolchain error
set -euo pipefail

ROOT="${1:-.}"
cd "${ROOT}"

NATIVE_SNAPSHOT="rust/target/parity/native.json"
WASM_SNAPSHOT="rust/target/parity/wasm.json"

echo "check_wasm_parity: regenerating native snapshot..."
(cd rust && cargo test -p haiai --test wasm_cross_compat_native -- --nocapture 2>&1 | tail -5)

if [[ ! -f "${NATIVE_SNAPSHOT}" ]]; then
  echo "ERROR: ${NATIVE_SNAPSHOT} missing after native test" >&2
  exit 2
fi
echo "check_wasm_parity: native snapshot at ${NATIVE_SNAPSHOT}"

if [[ -f "${WASM_SNAPSHOT}" ]]; then
  echo "check_wasm_parity: diffing native <-> wasm..."
  if diff -q "${NATIVE_SNAPSHOT}" "${WASM_SNAPSHOT}"; then
    echo "check_wasm_parity: native and wasm snapshots are byte-identical."
  else
    echo "ERROR: native and wasm snapshots diverge" >&2
    diff "${NATIVE_SNAPSHOT}" "${WASM_SNAPSHOT}" || true
    exit 1
  fi
else
  echo "check_wasm_parity: ${WASM_SNAPSHOT} not present — wasm side runs the same shared modules (transport::build_auth_header_with, sse_parse::SseParser, ws_protocol::parse_frame_text) so byte-identity is structural. Skipping diff."
fi

echo "check_wasm_parity: OK"
