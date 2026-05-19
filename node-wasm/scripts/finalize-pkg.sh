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
#    to JS + d.ts from a staged `tsconfig.json` and copies them into
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
STAGE_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "${STAGE_DIR}"
}
trap cleanup EXIT

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
# Stage the sources next to the real wasm-pack declarations so `tsc`
# validates the published wrapper against the generated wasm-bindgen API,
# without mixing `--project` and explicit source files (TS5042).
mkdir -p "${STAGE_DIR}/worker"
cp "${NODE_WASM_DIR}/index.ts" "${STAGE_DIR}/index.ts"
cp "${NODE_WASM_DIR}/types.ts" "${STAGE_DIR}/types.ts"
cp "${NODE_WASM_DIR}/worker/index.ts" "${STAGE_DIR}/worker/index.ts"
cp "${NODE_WASM_DIR}/jacs_wasm_stub.d.ts" "${STAGE_DIR}/jacs_wasm_stub.d.ts"

if [[ ! -f "${PKG_DIR}/haiai_wasm.d.ts" ]]; then
    echo "error: ${PKG_DIR}/haiai_wasm.d.ts missing. Run wasm-pack build first." >&2
    exit 1
fi
cp "${PKG_DIR}/haiai_wasm.d.ts" "${STAGE_DIR}/haiai_wasm.d.ts"

cat > "${STAGE_DIR}/tsconfig.json" <<'JSON'
{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ES2020",
    "moduleResolution": "bundler",
    "lib": ["ES2020", "DOM", "DOM.Iterable", "WebWorker"],
    "strict": true,
    "noImplicitAny": true,
    "strictNullChecks": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true,
    "isolatedModules": true,
    "declaration": true,
    "noEmitOnError": false,
    "outDir": "./out",
    "rootDir": "."
  },
  "include": [
    "./index.ts",
    "./types.ts",
    "./worker/*.ts",
    "./haiai_wasm.d.ts",
    "./jacs_wasm_stub.d.ts"
  ]
}
JSON

if command -v tsc >/dev/null 2>&1; then
    echo "finalize-pkg: tsc -p ${STAGE_DIR}/tsconfig.json"
    (cd "${STAGE_DIR}" && tsc -p tsconfig.json)
elif command -v npx >/dev/null 2>&1; then
    echo "finalize-pkg: npx tsc -p ${STAGE_DIR}/tsconfig.json"
    (cd "${STAGE_DIR}" && npx --yes -p typescript@5 tsc -p tsconfig.json)
else
    echo "error: tsc not available; cannot finalize TypeScript wrappers." >&2
    echo "       install Node + typescript and re-run finalize-pkg.sh." >&2
    exit 1
fi

# Copy the compiled outputs back into pkg/. Skip the staged copy of
# `haiai_wasm.d.ts` so the real wasm-pack declaration remains in place.
mkdir -p "${PKG_DIR}/worker"
if [[ -d "${STAGE_DIR}/out" ]]; then
    cp "${STAGE_DIR}/out/index.js" "${PKG_DIR}/index.js"
    cp "${STAGE_DIR}/out/index.d.ts" "${PKG_DIR}/index.d.ts"
    cp "${STAGE_DIR}/out/types.js" "${PKG_DIR}/types.js"
    cp "${STAGE_DIR}/out/types.d.ts" "${PKG_DIR}/types.d.ts"
    cp "${STAGE_DIR}/out/worker/index.js" "${PKG_DIR}/worker/index.js"
    cp "${STAGE_DIR}/out/worker/index.d.ts" "${PKG_DIR}/worker/index.d.ts"
fi
cp "${NODE_WASM_DIR}/worker/haiai-worker.js" "${PKG_DIR}/worker/haiai-worker.js"
echo "finalize-pkg: compiled TS wrapper into ${PKG_DIR}"

# --- 4. Copy README ---
if [[ -f "${NODE_WASM_DIR}/README.md" ]]; then
    cp "${NODE_WASM_DIR}/README.md" "${PKG_DIR}/README.md"
    echo "finalize-pkg: copied README.md"
fi

echo "finalize-pkg: ${PKG_DIR} is publishable. Inspect with: cd ${PKG_DIR} && npm pack --dry-run"
