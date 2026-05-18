#!/usr/bin/env bash
#
# scripts/ci/check_no_local_crypto.sh
#
# Crypto-policy denylist. Fails if any direct local-crypto import appears
# outside the narrow per-language allowlist. All real signing /
# verification / hashing must delegate to JACS (see CLAUDE.md Rule 1).
#
# Scans:
#   * python/src       (Python SDK)
#   * node/src         (Node SDK)
#   * go               (Go SDK)
#   * rust/haiai-wasm/src (browser-side Rust wrapper, when present)
#   * node-wasm        (browser-side TS wrapper, when present)
#
# The wasm scan paths are added per HAIAI_WASM_PRD.md §4.9 so the very
# first source file added to either directory trips the check if it
# imports `sha2::`, `ed25519`, `aes-gcm`, `argon2`, or pulls
# `node:crypto`. The paths are silently skipped if they do not yet exist.
set -euo pipefail

ROOT="${1:-.}"
cd "$ROOT"

# Helper that resolves a list of candidate roots and skips any that do
# not exist yet. Returns the roots that exist, one per line.
existing_roots() {
  local r
  for r in "$@"; do
    if [[ -d "${r}" ]]; then
      printf '%s\n' "${r}"
    fi
  done
}

# Default scan roots for the legacy SDK checks (Python / Node / Go).
# Use plain array append for bash 3.2 compatibility (macOS default).
LEGACY_ROOTS=()
while IFS= read -r r; do
  [[ -n "${r}" ]] && LEGACY_ROOTS+=("${r}")
done < <(existing_roots python/src node/src go)

# WASM-specific scan roots — silently skipped if the dirs do not exist yet,
# so the script is safe to run before rust/haiai-wasm/ and node-wasm/ land.
WASM_ROOTS=()
while IFS= read -r r; do
  [[ -n "${r}" ]] && WASM_ROOTS+=("${r}")
done < <(existing_roots rust/haiai-wasm/src node-wasm)

check_pattern() {
  local label="$1"
  local pattern="$2"
  local allow_regex="$3"
  shift 3
  local roots=("$@")

  if [[ "${#roots[@]}" -eq 0 ]]; then
    return 0
  fi

  local matches
  matches="$(rg -n --no-heading "$pattern" "${roots[@]}" || true)"
  if [[ -z "$matches" ]]; then
    return 0
  fi

  local filtered
  filtered="$(printf '%s\n' "$matches" | grep -Ev "$allow_regex" || true)"
  if [[ -n "$filtered" ]]; then
    echo "ERROR: disallowed direct crypto usage detected for ${label}:"
    printf '%s\n' "$filtered"
    return 1
  fi
  return 0
}

status=0

# Allowlist: Only JACS delegation and config files may use crypto directly.
# client.py, async_client.py, and auth.go are REMOVED from the allowlist —
# after FFI migration, they delegate HTTP + auth to Rust via hai-binding-core.
check_pattern \
  "Python Ed25519 primitive imports" \
  "cryptography\.hazmat\.primitives\.asymmetric\.ed25519" \
  '^(python/src/haiai/(crypt|config|signing)\.py):' \
  ${LEGACY_ROOTS[@]+"${LEGACY_ROOTS[@]}"} || status=1

# Node allowlist (post-crypto-elimination):
#   signing.ts -- uses randomUUID from node:crypto (not signing, acceptable)
#   hash.ts    -- uses createHash from node:crypto (deterministic hashing, acceptable)
#   crypt.ts   -- JACS crypto delegation layer
#   mime.ts    -- MIME handling
# client.ts is REMOVED from allowlist -- fromCredentials no longer uses node:crypto.
check_pattern \
  "Node native crypto imports" \
  "from 'node:crypto'" \
  '^(node/src/(crypt|signing|hash|mime)\.ts):' \
  ${LEGACY_ROOTS[@]+"${LEGACY_ROOTS[@]}"} || status=1

# Go allowlist (post-crypto-elimination):
#   _test.go   -- test files may use ed25519 directly for test fixtures
#   examples/  -- example code may demonstrate key usage
# signing.go, client.go, crypto_jacs.go, a2a.go are REMOVED from allowlist --
# all local crypto has been eliminated from production Go code.
check_pattern \
  "Go crypto/ed25519 imports" \
  '"crypto/ed25519"' \
  '^(go/.+_test\.go|go/examples/.+):' \
  ${LEGACY_ROOTS[@]+"${LEGACY_ROOTS[@]}"} || status=1

# ---- WASM wrappers (rust/haiai-wasm/src/**.rs + node-wasm/**.ts) ----------
#
# Per HAIAI_WASM_PRD.md §4.9: no local crypto in the browser wrapper.
# Every sign / verify / canonicalize call must delegate to jacs-core or
# jacs-wasm. The allowlist for each pattern below is intentionally empty
# (`^$` matches nothing in `grep -Ev`). Scoped to WASM_ROOTS so the new
# rules do not flag pre-existing imports in node/src, python/src, or go.
check_pattern \
  "Rust wasm crate sha2 imports (rust/haiai-wasm/src)" \
  '^[[:space:]]*use[[:space:]]+sha2(::|;)' \
  '^$' \
  ${WASM_ROOTS[@]+"${WASM_ROOTS[@]}"} || status=1

check_pattern \
  "Rust wasm crate ed25519 / aes-gcm / argon2 / ring imports (rust/haiai-wasm/src)" \
  '^[[:space:]]*use[[:space:]]+(ed25519|ed25519_dalek|aes_gcm|argon2|ring)(::|;)' \
  '^$' \
  ${WASM_ROOTS[@]+"${WASM_ROOTS[@]}"} || status=1

check_pattern \
  "Node-wasm crypto imports (node-wasm)" \
  "from ['\"]node:crypto['\"]" \
  '^$' \
  ${WASM_ROOTS[@]+"${WASM_ROOTS[@]}"} || status=1

check_pattern \
  "Node-wasm WebCrypto / SubtleCrypto direct calls (node-wasm)" \
  "(crypto\.subtle|globalThis\.crypto\.subtle|window\.crypto\.subtle)\.(sign|verify|digest|deriveBits|deriveKey|importKey|generateKey)" \
  '^$' \
  ${WASM_ROOTS[@]+"${WASM_ROOTS[@]}"} || status=1

if [[ "$status" -ne 0 ]]; then
  cat <<'MSG'

Policy violation:
  haiai runtime crypto must delegate to JACS functions.
  If this is a temporary migration exception, update the ADR and allowlist intentionally.
MSG
  exit 1
fi

echo "Crypto policy guard passed."
