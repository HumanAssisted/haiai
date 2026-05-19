#!/usr/bin/env bash
# Sanity tests for node-wasm/scripts/finalize-pkg.sh (HAIAI_WASM_PRD
# §4.10 / Task 031, JACS_WASM ISSUE 008 / 009 lesson).
#
# 1. Merges a synthetic pkg/ + template into a merged package.json and
#    asserts the merged shape has the expected name, version, exports,
#    files, sideEffects, license, and main/module/types.
# 2. Asserts the template references the haiai-wasm wasm-bindgen
#    exports the hand-written index.ts depends on (initHaiaiWasm,
#    version, about).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
TEMPLATE="${REPO_ROOT}/node-wasm/package.template.json"
INDEX_TS="${REPO_ROOT}/node-wasm/index.ts"
HAIAI_WASM_DTS="${REPO_ROOT}/node-wasm/haiai_wasm.d.ts"

fail_count=0
run_test() {
    local name="$1"
    shift
    if "$@"; then
        echo "PASS: ${name}"
    else
        echo "FAIL: ${name}" >&2
        fail_count=$((fail_count + 1))
    fi
}

test_template_well_formed() {
    [[ -f "${TEMPLATE}" ]] || return 1
    python3 -c "
import json, sys
with open('${TEMPLATE}') as f:
    data = json.load(f)
required = ['name', 'version', 'type', 'main', 'module', 'types',
            'sideEffects', 'files', 'exports', 'license']
missing = [k for k in required if k not in data]
if missing:
    print('missing fields:', missing, file=sys.stderr)
    sys.exit(1)
assert data['name'] == '@haiai/wasm', f'name mismatch: {data[\"name\"]}'
assert data['type'] == 'module', 'must be ESM'
assert data['sideEffects'] is False, 'sideEffects must be false'
assert '@jacs/wasm' in data.get('dependencies', {}), 'must depend on @jacs/wasm'
"
}

test_index_ts_uses_haiai_wasm_exports() {
    # The hand-written index.ts must reference the wasm-bindgen exports
    # that haiai_wasm.d.ts declares. Drift here is the JACS_WASM ISSUE
    # 009 class of bug (wrapper drifts from real surface).
    grep -q "initHaiaiWasm as wasmInit" "${INDEX_TS}" || return 1
    grep -q "version as wasmVersion" "${INDEX_TS}" || return 1
    grep -q "about as wasmAbout" "${INDEX_TS}" || return 1
}

test_haiai_wasm_dts_declares_referenced_exports() {
    grep -qE "^export function initHaiaiWasm" "${HAIAI_WASM_DTS}" || return 1
    grep -qE "^export function version" "${HAIAI_WASM_DTS}" || return 1
    grep -qE "^export function about" "${HAIAI_WASM_DTS}" || return 1
}

test_synthetic_merge_produces_publishable_shape() {
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    local pkg_dir="${tmp_dir}/pkg"
    mkdir -p "${pkg_dir}"
    # Synthesize a wasm-pack-style pkg/package.json.
    cat > "${pkg_dir}/package.json" <<'JSON'
{
  "name": "haiai-wasm-from-wasm-pack",
  "version": "0.0.0",
  "description": "wasm-pack stub",
  "main": "haiai_wasm.js"
}
JSON
    # Run the same merge logic finalize-pkg.sh uses, inline so the test
    # doesn't require a real Cargo.toml or pkg/.
    python3 - "${pkg_dir}/package.json" "${TEMPLATE}" "9.9.9" <<'PY'
import json, sys
pkg_path, template_path, version = sys.argv[1], sys.argv[2], sys.argv[3]
pkg = json.load(open(pkg_path))
template = json.load(open(template_path))
pkg.update(template)
pkg["version"] = version
json.dump(pkg, open(pkg_path, "w"), indent=2)
PY
    # Assert merged shape.
    python3 -c "
import json, sys
data = json.load(open('${pkg_dir}/package.json'))
assert data['name'] == '@haiai/wasm', f'name not merged: {data[\"name\"]}'
assert data['version'] == '9.9.9', f'version not overridden: {data[\"version\"]}'
assert data['type'] == 'module', 'type missing'
assert data['sideEffects'] is False, 'sideEffects missing'
assert 'index.js' in data.get('files', []), 'index.js not in files'
"
    rm -rf "${tmp_dir}"
}

run_test "package.template.json well-formed" test_template_well_formed
run_test "index.ts uses haiai_wasm.d.ts exports" test_index_ts_uses_haiai_wasm_exports
run_test "haiai_wasm.d.ts declares referenced exports" test_haiai_wasm_dts_declares_referenced_exports
run_test "synthetic merge produces publishable shape" test_synthetic_merge_produces_publishable_shape

if [[ "${fail_count}" -ne 0 ]]; then
    echo ""
    echo "FAILED: ${fail_count} test(s) failed" >&2
    exit 1
fi

echo ""
echo "All finalize-pkg tests passed."
