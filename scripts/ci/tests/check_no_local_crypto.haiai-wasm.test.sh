#!/usr/bin/env bash
#
# Tests for the haiai-wasm / node-wasm extensions in
# scripts/ci/check_no_local_crypto.sh. Creates temporary files under the
# real wasm scan roots, runs the script, and cleans up.
#
# This test does NOT assume the baseline tree is currently clean (the
# pre-existing Node check may flag `node/src/client.ts` independently of
# the wasm work). It captures the baseline output once, runs the script
# again after introducing a wasm-specific violation, and asserts only that
# the wasm violation appears / disappears as expected.
#
# Run:  bash scripts/ci/tests/check_no_local_crypto.haiai-wasm.test.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
SCRIPT="${REPO_ROOT}/scripts/ci/check_no_local_crypto.sh"

if [[ ! -x "${SCRIPT}" ]]; then
  echo "FAIL: ${SCRIPT} not executable" >&2
  exit 1
fi

declare -a CREATED_FILES=()
declare -a CREATED_DIRS=()
cleanup() {
  for path in "${CREATED_FILES[@]}"; do
    rm -f "${path}"
  done
  # Remove directories we created in reverse order, then sweep any empty
  # parents (rust/haiai-wasm/ and node-wasm/) that we may have implicitly
  # created via `mkdir -p`. Only rmdir empty dirs.
  for ((i=${#CREATED_DIRS[@]}-1; i>=0; i--)); do
    rmdir "${CREATED_DIRS[i]}" 2>/dev/null || true
  done
  rmdir "${REPO_ROOT}/rust/haiai-wasm/src" 2>/dev/null || true
  rmdir "${REPO_ROOT}/rust/haiai-wasm"     2>/dev/null || true
  rmdir "${REPO_ROOT}/node-wasm"           2>/dev/null || true
}
trap cleanup EXIT

ensure_dir() {
  local d="$1"
  if [[ ! -d "${d}" ]]; then
    mkdir -p "${d}"
    CREATED_DIRS+=("${d}")
  fi
}

run_script() {
  ( cd "${REPO_ROOT}" && bash "${SCRIPT}" 2>&1 ) || true
}

# Capture baseline output (whatever it is — pass or fail).
BASELINE="$(run_script)"
echo "Captured baseline output ($(printf '%s' "${BASELINE}" | wc -l | tr -d ' ') lines)"

# ---- Case 1: rust/haiai-wasm/src/test_leak.rs with sha2:: must add a new
#       'Rust wasm crate sha2 imports' error to the output.
ensure_dir "${REPO_ROOT}/rust/haiai-wasm/src"
BAD_RS="${REPO_ROOT}/rust/haiai-wasm/src/test_leak_$$.rs"
CREATED_FILES+=("${BAD_RS}")
cat > "${BAD_RS}" <<'EOF'
use sha2::Digest;
fn _smoke() { let _ = sha2::Sha256::new(); }
EOF

OUT="$(run_script)"
if ! printf '%s' "${OUT}" | grep -q 'Rust wasm crate sha2 imports'; then
  echo "FAIL: sha2 import in rust/haiai-wasm/src was not flagged" >&2
  printf '%s\n' "${OUT}" >&2
  exit 1
fi
if ! printf '%s' "${OUT}" | grep -F "${BAD_RS#${REPO_ROOT}/}" > /dev/null; then
  echo "FAIL: output did not name the offending file" >&2
  printf '%s\n' "${OUT}" >&2
  exit 1
fi
echo "PASS: rust/haiai-wasm/src sha2 import flagged"
rm -f "${BAD_RS}"

# After cleanup the baseline should match exactly (no lingering wasm
# errors from this case).
AFTER_RS="$(run_script)"
if [[ "${AFTER_RS}" != "${BASELINE}" ]]; then
  echo "FAIL: removing the test file did not restore baseline output" >&2
  diff <(printf '%s' "${BASELINE}") <(printf '%s' "${AFTER_RS}") >&2 || true
  exit 1
fi
echo "PASS: rust file cleanup restored baseline"

# ---- Case 2: node-wasm/leak.ts with node:crypto must add a new
#       'Node-wasm crypto imports (node-wasm)' error.
ensure_dir "${REPO_ROOT}/node-wasm"
BAD_TS="${REPO_ROOT}/node-wasm/leak_$$.ts"
CREATED_FILES+=("${BAD_TS}")
cat > "${BAD_TS}" <<'EOF'
import { createHash } from "node:crypto";
export const _smoke = (data: string) => createHash("sha256").update(data).digest();
EOF

OUT="$(run_script)"
if ! printf '%s' "${OUT}" | grep -q 'Node-wasm crypto imports (node-wasm)'; then
  echo "FAIL: node:crypto import in node-wasm was not flagged" >&2
  printf '%s\n' "${OUT}" >&2
  exit 1
fi
if ! printf '%s' "${OUT}" | grep -F "${BAD_TS#${REPO_ROOT}/}" > /dev/null; then
  echo "FAIL: output did not name the offending TS file" >&2
  printf '%s\n' "${OUT}" >&2
  exit 1
fi
echo "PASS: node-wasm node:crypto import flagged"
rm -f "${BAD_TS}"

AFTER_TS="$(run_script)"
if [[ "${AFTER_TS}" != "${BASELINE}" ]]; then
  echo "FAIL: removing the TS test file did not restore baseline output" >&2
  diff <(printf '%s' "${BASELINE}") <(printf '%s' "${AFTER_TS}") >&2 || true
  exit 1
fi
echo "PASS: TS file cleanup restored baseline"

# ---- Case 3: a clean wasm-bindgen-only Rust file must NOT add an error.
CLEAN_RS="${REPO_ROOT}/rust/haiai-wasm/src/clean_$$.rs"
CREATED_FILES+=("${CLEAN_RS}")
cat > "${CLEAN_RS}" <<'EOF'
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = noop)]
pub fn _noop() {}
EOF

AFTER_CLEAN="$(run_script)"
if [[ "${AFTER_CLEAN}" != "${BASELINE}" ]]; then
  echo "FAIL: a clean wasm-bindgen file changed the output (should be inert)" >&2
  diff <(printf '%s' "${BASELINE}") <(printf '%s' "${AFTER_CLEAN}") >&2 || true
  exit 1
fi
echo "PASS: clean rust/haiai-wasm/src file accepted (no new errors)"
rm -f "${CLEAN_RS}"

echo "All check_no_local_crypto haiai-wasm tests passed."
