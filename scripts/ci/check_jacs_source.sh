#!/usr/bin/env bash
# Check ref/version configuration and, when supplied, the actual fetched source.
set -euo pipefail

source_ref="${1:-}"
expected_version="${2:-}"
source_dir="${3:-}"
# Preserve three-argument callers for a source whose versions are still equal.
# An explicitly supplied empty core version is invalid, catching config drift.
expected_core_version="${4-$expected_version}"
expected_npm_version="${5-$expected_version}"
fail() { echo "ERROR: $*" >&2; exit 1; }

[[ "$expected_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "invalid expected JACS version: $expected_version"
[[ "$expected_core_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "invalid expected JACS core version: $expected_core_version"
[[ "$expected_npm_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "invalid expected JACS npm version: $expected_npm_version"
if [[ "$source_ref" =~ ^[0-9a-f]{40}$ ]]; then
  ref_kind=commit
elif [[ "$source_ref" == "crate/v$expected_version" || "$source_ref" == "v$expected_version" ]]; then
  ref_kind=tag
else
  fail "JACS_REF must be a full commit SHA or a release tag matching JACS_VERSION=$expected_version"
fi

if [[ -z "$source_dir" ]]; then
  echo "JACS ref/version configuration valid; fetched source is checked in each checkout step."
  exit 0
fi

actual_commit="$(git -C "$source_dir" rev-parse --verify HEAD)"
if [[ "$ref_kind" == commit ]]; then
  expected_commit="$source_ref"
else
  expected_commit="$(git -C "$source_dir" rev-parse --verify "refs/tags/$source_ref^{commit}")"
fi
[[ "$actual_commit" == "$expected_commit" ]] || fail "JACS source HEAD $actual_commit does not match $source_ref"

# Native manifests used by the SDK's Rust, Python, Node and Go source builds.
# Portable core stays at the root with its independently pinned version. The
# SDK consumes the retained native adapters, including the archived MCP.
for crate in jacs-core archive/native/{jacs-media,jacs,binding-core,jacs-mcp,jacs-cli,jacspy,jacsnpm,jacsgo/lib}; do
  manifest="$source_dir/$crate/Cargo.toml"
  [[ -f "$manifest" ]] || fail "missing JACS source manifest: $manifest"
  actual_version="$(awk -F '"' '
    /^\[package\][[:space:]]*$/ { in_package=1; next }
    /^\[/ { in_package=0 }
    in_package && /^[[:space:]]*version[[:space:]]*=/ { print $2; exit }
  ' "$manifest")"
  crate_version="$expected_version"
  [[ "$crate" != jacs-core ]] || crate_version="$expected_core_version"
  [[ "$actual_version" == "$crate_version" ]] || fail "$crate version $actual_version does not match expected JACS $crate_version"
done
python3 - "$source_dir/archive/native/jacsnpm/package.json" "$expected_npm_version" <<'PY'
import json
from pathlib import Path
import sys

try:
    package = json.loads(Path(sys.argv[1]).read_text())
    if package.get("name") != "@hai.ai/jacs":
        raise ValueError("expected package @hai.ai/jacs")
    if package.get("version") != sys.argv[2]:
        raise ValueError(f"@hai.ai/jacs version {package.get('version')} does not match expected JACS npm {sys.argv[2]}")
except (OSError, ValueError, TypeError, AttributeError) as error:
    raise SystemExit(f"ERROR: native npm source metadata: {error}") from None
PY
echo "JACS source $actual_commit: core matches $expected_core_version; npm matches $expected_npm_version; all 8 native manifests match $expected_version."
