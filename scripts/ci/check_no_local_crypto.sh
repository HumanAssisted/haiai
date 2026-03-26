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

# Node allowlist (post-crypto-elimination):
#   signing.ts -- uses randomUUID from node:crypto (not signing, acceptable)
#   hash.ts    -- uses createHash from node:crypto (deterministic hashing, acceptable)
#   crypt.ts   -- JACS crypto delegation layer
#   mime.ts    -- MIME handling
# client.ts is REMOVED from allowlist -- fromCredentials no longer uses node:crypto.
check_pattern \
  "Node native crypto imports" \
  "from 'node:crypto'" \
  '^(node/src/(crypt|signing|hash|mime)\.ts):' || status=1

# Go allowlist (post-crypto-elimination):
#   _test.go   -- test files may use ed25519 directly for test fixtures
#   examples/  -- example code may demonstrate key usage
# signing.go, client.go, crypto_jacs.go, a2a.go are REMOVED from allowlist --
# all local crypto has been eliminated from production Go code.
check_pattern \
  "Go crypto/ed25519 imports" \
  '"crypto/ed25519"' \
  '^(go/.+_test\.go|go/examples/.+):' || status=1

if [[ "$status" -ne 0 ]]; then
  cat <<'MSG'

Policy violation:
  haiai runtime crypto must delegate to JACS functions.
  If this is a temporary migration exception, update the ADR and allowlist intentionally.
MSG
  exit 1
fi

echo "Crypto policy guard passed."
