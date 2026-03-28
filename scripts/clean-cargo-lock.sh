#!/usr/bin/env bash
#
# clean-cargo-lock.sh — Strip [[patch.unused]] entries from rust/Cargo.lock.
#
# These entries are added by cargo when [patch.crates-io] points to local JACS
# paths during development. They cause merge conflicts on pull because the remote
# Cargo.lock doesn't have them.
#
# Usage:
#   ./scripts/clean-cargo-lock.sh           # clean in-place
#   git checkout -- rust/Cargo.lock         # alternative: just reset

set -euo pipefail

LOCKFILE="${1:-rust/Cargo.lock}"

if [ ! -f "$LOCKFILE" ]; then
    exit 0
fi

# Remove [[patch.unused]] blocks (each block is 3 lines: header, name, version,
# optionally followed by a blank line)
sed -i '' '/^\[\[patch\.unused\]\]/,/^$/d' "$LOCKFILE"

# Remove any trailing blank lines left behind
sed -i '' -e :a -e '/^\n*$/{$d;N;ba' -e '}' "$LOCKFILE"
