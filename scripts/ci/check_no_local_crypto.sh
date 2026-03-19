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

# Transitional allowlist. New usage must not be added outside these files.
check_pattern \
  "Python Ed25519 primitive imports" \
  "cryptography\.hazmat\.primitives\.asymmetric\.ed25519" \
  '^(python/src/haiai/(crypt|client|async_client|config|signing)\.py):' || status=1

check_pattern \
  "Node native crypto imports" \
  "from 'node:crypto'" \
  '^(node/src/(crypt|signing|client|hash|mime)\.ts):' || status=1

check_pattern \
  "Go crypto/ed25519 imports" \
  '"crypto/ed25519"' \
  '^(go/(signing|client|auth|crypto_fallback|crypto_jacs|a2a|sign_response_local)\.go|go/.+_test\.go|go/examples/.+):' || status=1

if [[ "$status" -ne 0 ]]; then
  cat <<'MSG'

Policy violation:
  haiai runtime crypto must delegate to JACS functions.
  If this is a temporary migration exception, update the ADR and allowlist intentionally.
MSG
  exit 1
fi

echo "Crypto policy guard passed."
