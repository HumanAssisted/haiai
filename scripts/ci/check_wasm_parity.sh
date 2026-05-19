#!/usr/bin/env bash
#
# scripts/ci/check_wasm_parity.sh
#
# HAIAI_WASM_PRD §5.2 / §6 / Task 041 / Issue 007 — cross-compat parity gate.
#
# Runs the native + wasm cross-compat tests and diffs their output JSONs.
# Any byte-level disagreement on auth header, canonical register body,
# canonical send body, SSE event parse, or WS frame parse fails CI.
#
# Native side (Rust `cargo test`):
#   rust/haiai/tests/wasm_cross_compat_native.rs writes
#   rust/target/parity/native.json
#
# Wasm side (`wasm-pack test --headless --chrome rust/haiai-wasm`):
#   rust/haiai-wasm/tests/parity_snapshot.rs prints the snapshot JSON
#   between sentinel markers __WASM_PARITY_JSON_BEGIN__ ... __WASM_PARITY_JSON_END__.
#   This script extracts that JSON and writes it to
#   rust/target/parity/wasm.json.
#
# Exit codes:
#   0 — snapshots match
#   1 — drift detected, or wasm.json missing/unparseable
#   2 — toolchain error (cargo test or wasm-pack test failure)
#
# Environment overrides:
#   WASM_PARITY_SKIP_WASM=1 — skip the wasm-pack run (useful for local
#                              development on machines without Chrome /
#                              chromedriver). The wasm.json snapshot must
#                              still already exist on disk; otherwise the
#                              script fails.
#   HAIAI_WASM_RUSTFLAGS     — rustflags for wasm-pack only. Defaults to
#                              warnings-as-errors plus an explicit wasm stack
#                              size large enough for pq2025 browser signing.
set -euo pipefail

ROOT="${1:-.}"
cd "${ROOT}"

NATIVE_SNAPSHOT="rust/target/parity/native.json"
WASM_SNAPSHOT="rust/target/parity/wasm.json"
WASM_RUSTFLAGS="${HAIAI_WASM_RUSTFLAGS:--D warnings -C link-arg=-zstack-size=8388608}"

# ── (1) Regenerate native snapshot ──────────────────────────────────
echo "check_wasm_parity: regenerating native snapshot..."
(cd rust && cargo test -p haiai --test wasm_cross_compat_native -- --nocapture 2>&1 | tail -5)

if [[ ! -f "${NATIVE_SNAPSHOT}" ]]; then
  echo "ERROR: ${NATIVE_SNAPSHOT} missing after native test" >&2
  exit 2
fi
echo "check_wasm_parity: native snapshot at ${NATIVE_SNAPSHOT}"

# ── (2) Regenerate wasm snapshot (unless skipped) ───────────────────
if [[ "${WASM_PARITY_SKIP_WASM:-0}" == "1" ]]; then
  echo "check_wasm_parity: WASM_PARITY_SKIP_WASM=1 — skipping wasm-pack run"
else
  echo "check_wasm_parity: regenerating wasm snapshot via wasm-pack..."
  WASM_LOG="$(mktemp -t wasm_parity_log.XXXXXX)"
  trap 'rm -f "${WASM_LOG}"' EXIT
  if ! PATH="${HOME}/.cargo/bin:${PATH}" RUSTFLAGS="${WASM_RUSTFLAGS}" \
    wasm-pack test --headless --chrome rust/haiai-wasm --test parity_snapshot 2>&1 | tee "${WASM_LOG}"; then
    echo "ERROR: wasm-pack test --test parity_snapshot failed" >&2
    exit 2
  fi

  # Extract the JSON snapshot between sentinel markers. The markers
  # appear on a single line per `println!` in parity_snapshot.rs.
  # `tr` strips ANSI colors that browser-actions/setup-chrome can
  # inject into wasm-pack's stdout.
  PARITY_JSON="$(tr -d '\033' < "${WASM_LOG}" \
    | grep -oE '__WASM_PARITY_JSON_BEGIN__.*__WASM_PARITY_JSON_END__' \
    | head -n1 \
    | sed -E 's/^__WASM_PARITY_JSON_BEGIN__//; s/__WASM_PARITY_JSON_END__$//' || true)"

  if [[ -z "${PARITY_JSON}" ]]; then
    echo "ERROR: wasm test ran but produced no __WASM_PARITY_JSON_*__ block" >&2
    echo "       This means parity_snapshot.rs failed before emitting the snapshot." >&2
    exit 1
  fi

  mkdir -p rust/target/parity
  # Reformat through `python3 -m json.tool` so wasm.json has the same
  # pretty-printed layout as native.json (which is produced via
  # serde_json::to_string_pretty + serde_json's 2-space indent). Both
  # sides round-trip through the same pretty-printer for byte-identical
  # output.
  printf '%s' "${PARITY_JSON}" | python3 -m json.tool --no-ensure-ascii > "${WASM_SNAPSHOT}"
  echo "check_wasm_parity: wasm snapshot at ${WASM_SNAPSHOT}"
fi

if [[ ! -f "${WASM_SNAPSHOT}" ]]; then
  echo "ERROR: ${WASM_SNAPSHOT} missing — the wasm-pack step did not produce a snapshot" >&2
  echo "       (and WASM_PARITY_SKIP_WASM was set, so we cannot diff)" >&2
  exit 1
fi

# ── (3) Diff native vs wasm. Reformat both through the same Python
# pretty-printer first so we compare semantic content, not whitespace
# emitted by two different to_string_pretty impls.
NATIVE_NORM="$(mktemp -t native_norm.XXXXXX)"
WASM_NORM="$(mktemp -t wasm_norm.XXXXXX)"
trap 'rm -f "${NATIVE_NORM}" "${WASM_NORM}" "${WASM_LOG:-}"' EXIT
python3 -m json.tool --no-ensure-ascii < "${NATIVE_SNAPSHOT}" > "${NATIVE_NORM}"
python3 -m json.tool --no-ensure-ascii < "${WASM_SNAPSHOT}" > "${WASM_NORM}"

if diff -q "${NATIVE_NORM}" "${WASM_NORM}" >/dev/null; then
  echo "check_wasm_parity: native and wasm snapshots are byte-identical (post-normalization)."
else
  echo "ERROR: native and wasm parity snapshots diverge:" >&2
  diff -u "${NATIVE_NORM}" "${WASM_NORM}" || true
  exit 1
fi

echo "check_wasm_parity: OK"
