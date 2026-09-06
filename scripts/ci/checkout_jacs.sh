#!/usr/bin/env bash
# Fetch one exact JACS source into a new sibling directory. Never replace a checkout.
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "Usage: $0 <full-commit-sha|crate/vX.Y.Z> <new-destination> [repository]" >&2
  exit 1
fi
ref="$1"
destination="$2"
repository="${3:-https://github.com/HumanAssisted/JACS.git}"
if [[ "$ref" =~ ^[0-9a-f]{40}$ ]]; then
  fetch_ref="$ref"
elif [[ "$ref" =~ ^crate/v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  fetch_ref="refs/tags/$ref"
else
  echo 'JACS source must be a full lowercase commit SHA or canonical crate/vX.Y.Z tag' >&2
  exit 1
fi
if [[ -z "$destination" || -e "$destination" || -L "$destination" ]]; then
  echo 'JACS checkout destination must not already exist' >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/../.." && pwd)"
expected_version="$(awk '/^jacs = / {split($0, fields, "\""); sub(/^=/, "", fields[2]); print fields[2]}' "$root/rust/haiai/Cargo.toml")"
if [[ ! "$expected_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo 'Cannot determine the canonical SDK JACS dependency version' >&2
  exit 1
fi
if [[ "$ref" == crate/* && "$ref" != "crate/v$expected_version" ]]; then
  echo 'JACS release tag does not match the SDK dependency' >&2
  exit 1
fi

# mkdir is the atomic no-replacement guard. A failed fetch stays visible for
# diagnosis; this helper never recursively deletes a caller-selected path.
mkdir -- "$destination"
git -C "$destination" init --quiet
git -C "$destination" remote add origin "$repository"
git -C "$destination" fetch --quiet --depth=1 origin "$fetch_ref"
commit="$(git -C "$destination" rev-parse --verify 'FETCH_HEAD^{commit}')"
if [[ "$ref" =~ ^[0-9a-f]{40}$ && "$commit" != "$ref" ]]; then
  echo 'Fetched JACS commit does not match the exact source pin' >&2
  exit 1
fi
git -C "$destination" checkout --quiet --detach "$commit"
if [[ "$(git -C "$destination" rev-parse --verify HEAD)" != "$commit" ]] ||
   git -C "$destination" symbolic-ref -q HEAD >/dev/null; then
  echo 'JACS checkout did not produce the exact detached commit' >&2
  exit 1
fi
actual_version="$(awk '/^\[package\]$/ {package=1; next} /^\[/ {package=0} package && /^version = / {split($0, fields, "\""); print fields[2]}' "$destination/jacs/Cargo.toml")"
if [[ "$actual_version" != "$expected_version" ]]; then
  echo 'JACS source version does not match the SDK dependency' >&2
  exit 1
fi
printf 'JACS checkout verified: %s (version %s)\n' "$commit" "$actual_version"
