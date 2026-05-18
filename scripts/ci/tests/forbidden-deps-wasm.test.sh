#!/usr/bin/env bash
#
# Unit test for scripts/ci/forbidden-deps-wasm.sh.
# Uses WASM_TREE_FIXTURE so it does not require cargo or a wasm32 toolchain.
#
# Run:  bash scripts/ci/tests/forbidden-deps-wasm.test.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
SCRIPT="${REPO_ROOT}/scripts/ci/forbidden-deps-wasm.sh"

if [[ ! -x "${SCRIPT}" ]]; then
  echo "FAIL: ${SCRIPT} not executable" >&2
  exit 1
fi

TMPDIR_T="$(mktemp -d)"
trap 'rm -rf "${TMPDIR_T}"' EXIT

# ---- Case 1: clean tree (no forbidden crates) exits 0 ----------------------
CLEAN="${TMPDIR_T}/clean.txt"
cat > "${CLEAN}" <<'EOF'
haiai-wasm v0.4.1 (/repo/rust/haiai-wasm)
├── wasm-bindgen v0.2.99
├── js-sys v0.3.76
└── web-sys v0.3.76
    └── wasm-bindgen v0.2.99
EOF

if WASM_TREE_FIXTURE="${CLEAN}" "${SCRIPT}" haiai-wasm > "${TMPDIR_T}/clean.out" 2>&1; then
  if ! grep -q "is clean" "${TMPDIR_T}/clean.out"; then
    echo "FAIL: clean tree did not print 'is clean'" >&2
    cat "${TMPDIR_T}/clean.out" >&2
    exit 1
  fi
  echo "PASS: clean tree exits 0"
else
  echo "FAIL: clean tree should exit 0" >&2
  cat "${TMPDIR_T}/clean.out" >&2
  exit 1
fi

# ---- Case 2: tree with forbidden crate exits 1 -----------------------------
DIRTY="${TMPDIR_T}/dirty.txt"
cat > "${DIRTY}" <<'EOF'
haiai-wasm v0.4.1 (/repo/rust/haiai-wasm)
├── wasm-bindgen v0.2.99
├── tokio-tungstenite v0.28.0
│   └── mio v0.8.10
└── tempfile v3.10.0
EOF

if WASM_TREE_FIXTURE="${DIRTY}" "${SCRIPT}" haiai-wasm > "${TMPDIR_T}/dirty.out" 2>&1; then
  echo "FAIL: dirty tree should exit non-zero" >&2
  cat "${TMPDIR_T}/dirty.out" >&2
  exit 1
fi

for crate in tokio-tungstenite mio tempfile; do
  if ! grep -q "FORBIDDEN: '${crate}'" "${TMPDIR_T}/dirty.out"; then
    echo "FAIL: dirty tree did not flag ${crate}" >&2
    cat "${TMPDIR_T}/dirty.out" >&2
    exit 1
  fi
done
echo "PASS: dirty tree exits 1 and lists each forbidden crate"

# ---- Case 3: WASM_TREE_FIXTURE pointing at a nonexistent file errors ------
if WASM_TREE_FIXTURE="${TMPDIR_T}/does-not-exist" "${SCRIPT}" haiai-wasm \
    > "${TMPDIR_T}/missing.out" 2>&1; then
  echo "FAIL: missing fixture should exit non-zero" >&2
  cat "${TMPDIR_T}/missing.out" >&2
  exit 1
fi
if ! grep -q "does not exist" "${TMPDIR_T}/missing.out"; then
  echo "FAIL: missing fixture error message missing" >&2
  cat "${TMPDIR_T}/missing.out" >&2
  exit 1
fi
echo "PASS: missing fixture exits with clear error"

echo "All forbidden-deps-wasm tests passed."
