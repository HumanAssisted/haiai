#!/usr/bin/env bash
#
# scripts/ci/tests/check_wasm_surface.test.sh
#
# Drift-detection sanity tests for check_wasm_surface.sh.
# Verifies:
#   1. enforcer exits 0 against the current tree
#   2. enforcer fails when a fixture entry has no matching HaiClient method
#   3. enforcer fails when (TODO) the TS wrapper drops a method —
#      only meaningful after node-wasm/ lands (Task 031).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SCRIPT="${REPO_ROOT}/scripts/ci/check_wasm_surface.sh"
FIXTURE="${REPO_ROOT}/fixtures/wasm_browser_surface.json"

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

test_current_tree_passes() {
    bash "${SCRIPT}" "${REPO_ROOT}" >/dev/null 2>&1
}

test_drift_rust_missing_fails() {
    # Inject a fake method into a temp fixture and run against it.
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    cp -R "${REPO_ROOT}/fixtures" "${tmp_dir}/fixtures"
    cp -R "${REPO_ROOT}/rust" "${tmp_dir}/rust"
    # Inject an entry pointing at a Rust fn that does not exist.
    python3 -c '
import json, sys
p = sys.argv[1]
with open(p) as f:
    data = json.load(f)
data["methods"].append({
    "js_name": "neverImplementedXyz",
    "rust_fn": "never_implemented_xyz_method",
    "kind": "http",
})
with open(p, "w") as f:
    json.dump(data, f)
' "${tmp_dir}/fixtures/wasm_browser_surface.json"
    # Expect non-zero exit
    if bash "${SCRIPT}" "${tmp_dir}" >/dev/null 2>&1; then
        rm -rf "${tmp_dir}"
        return 1  # should have failed
    fi
    rm -rf "${tmp_dir}"
    return 0
}

run_test "current tree passes" test_current_tree_passes
run_test "drift (rust method missing) is rejected" test_drift_rust_missing_fails

if [[ "${fail_count}" -ne 0 ]]; then
    echo ""
    echo "FAILED: ${fail_count} test(s) failed" >&2
    exit 1
fi

echo ""
echo "All check_wasm_surface tests passed."
