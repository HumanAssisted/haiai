#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:-.}"
cd "$ROOT"

check_pattern() {
  local label="$1"
  local pattern="$2"
  local allow_regex="$3"

  local matches
  matches="$(rg -n --no-heading "$pattern" python/src node/src go || true)"
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
  '^(python/src/haiai/(crypt|config|signing)\.py):' || status=1

# NOTE: client.ts still uses node:crypto for fromCredentials() credential signing.
# This will be removed once fromCredentials delegates to FFI binding.
# TODO(DRY_FFI): Remove client.ts from allowlist after full FFI migration.
check_pattern \
  "Node native crypto imports" \
  "from 'node:crypto'" \
  '^(node/src/(crypt|signing|client|hash|mime)\.ts):' || status=1

# Go allowlist rationale (post-FFI migration):
#   signing.go     -- key parsing (LoadPrivateKey, ParsePublicKey) uses ed25519 types
#   client.go      -- Client.privateKey field is ed25519.PrivateKey (kept for JACS init)
#   crypto_jacs.go -- GenerateKeyPair uses local ed25519 (jacsgo lacks keygen FFI)
#   a2a.go         -- references ed25519 types for key handling
#   _test.go       -- test files may use ed25519 directly
#   examples/      -- example code may demonstrate key usage
# NOTE: auth.go is REMOVED from allowlist — auth header construction is now in Rust.
check_pattern \
  "Go crypto/ed25519 imports" \
  '"crypto/ed25519"' \
  '^(go/(signing|client|crypto_jacs|a2a)\.go|go/.+_test\.go|go/examples/.+):' || status=1

if [[ "$status" -ne 0 ]]; then
  cat <<'MSG'

Policy violation:
  haiai runtime crypto must delegate to JACS functions.
  If this is a temporary migration exception, update the ADR and allowlist intentionally.
MSG
  exit 1
fi

echo "Crypto policy guard passed."
