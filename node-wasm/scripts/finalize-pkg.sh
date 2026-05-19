#!/usr/bin/env bash
# Finalize the wasm-pack-produced `rust/haiai-wasm/pkg/` directory into
# a publishable `@haiai/wasm` npm package (HAIAI_WASM_PRD §4.10 /
# Task 031). Mirrors jacs-wasm/scripts/finalize-pkg.sh.
#
# 1. Reads the version from `rust/haiai-wasm/Cargo.toml` so the npm
#    version matches the Rust crate version.
# 2. Merges `node-wasm/package.template.json` into `pkg/package.json`
#    via Python json (avoids a `jq` dependency on dev machines).
# 3. Compiles the hand-written `index.ts` + `worker/*.ts` + `types.ts`
#    to JS + d.ts via the node-wasm/tsconfig.json and copies them into
#    pkg/.
# 4. Copies node-wasm/README.md into pkg/ so npm shows the README.
#
# Idempotent — safe to re-run.

set -euo pipefail

NODE_WASM_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "${NODE_WASM_DIR}/.." && pwd)"
PKG_DIR="${REPO_ROOT}/rust/haiai-wasm/pkg"
TEMPLATE="${NODE_WASM_DIR}/package.template.json"
CARGO_TOML="${REPO_ROOT}/rust/haiai-wasm/Cargo.toml"

if [[ ! -d "${PKG_DIR}" ]]; then
    echo "error: ${PKG_DIR} does not exist. Run 'wasm-pack build --target web --release rust/haiai-wasm' first." >&2
    exit 1
fi

if [[ ! -f "${TEMPLATE}" ]]; then
    echo "error: ${TEMPLATE} missing." >&2
    exit 1
fi

# --- 1. Extract version from Cargo.toml ---
CARGO_VERSION="$(grep -E '^version[[:space:]]*=' "${CARGO_TOML}" | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ -z "${CARGO_VERSION}" ]]; then
    echo "error: could not extract version from ${CARGO_TOML}" >&2
    exit 1
fi
echo "finalize-pkg: version = ${CARGO_VERSION}"

# --- 2. Merge template into pkg/package.json ---
python3 - "${PKG_DIR}/package.json" "${TEMPLATE}" "${CARGO_VERSION}" <<'PY'
import json
import sys

pkg_path, template_path, version = sys.argv[1], sys.argv[2], sys.argv[3]
with open(pkg_path, "r", encoding="utf-8") as fh:
    pkg = json.load(fh)
with open(template_path, "r", encoding="utf-8") as fh:
    template = json.load(fh)

# Template takes precedence on every field it specifies.
pkg.update(template)
pkg["version"] = version

with open(pkg_path, "w", encoding="utf-8") as fh:
    json.dump(pkg, fh, indent=2, sort_keys=False)
    fh.write("\n")
print(f"finalize-pkg: wrote {pkg_path} (version={version})")
PY

# --- 3. Compile the hand-written TS wrapper ---
# Use the node-wasm tsconfig with `--noEmit false --declaration true`
# overrides so we emit both index.js and index.d.ts into pkg/.
if command -v tsc >/dev/null 2>&1; then
    cd "${NODE_WASM_DIR}"
    tsc \
        --project tsconfig.json \
        --noEmit false \
        --declaration true \
        --outDir "${PKG_DIR}" \
        index.ts || {
        echo "finalize-pkg: tsc emit failed for index.ts" >&2
        exit 1
    }
    if [[ -f types.ts ]]; then
        tsc \
            --project tsconfig.json \
            --noEmit false \
            --declaration true \
            --outDir "${PKG_DIR}" \
            types.ts || true
    fi
    cd "${REPO_ROOT}"
    echo "finalize-pkg: compiled TS wrapper into ${PKG_DIR}"
else
    echo "finalize-pkg: tsc not available; skipping TS compile (CI must run with TypeScript installed)" >&2
fi

# --- 4. Copy README ---
if [[ -f "${NODE_WASM_DIR}/README.md" ]]; then
    cp "${NODE_WASM_DIR}/README.md" "${PKG_DIR}/README.md"
    echo "finalize-pkg: copied README.md"
fi

echo "finalize-pkg: ${PKG_DIR} is publishable. Inspect with: cd ${PKG_DIR} && npm pack --dry-run"
