import path from "node:path";
import { defineConfig } from "vite";

const haiaiWasmPkg = path.resolve(__dirname, "../../../rust/haiai-wasm/pkg");
const jacsWasmPkg = path.resolve(__dirname, "../../../../../wasm/jacs-wasm/pkg");

// HAIAI_WASM_PRD §5.5: Vite is the smoke-tested bundler. The
// `optimizeDeps.exclude` keeps the `@haiai/wasm` package out of Vite's
// pre-bundling (it ships its own `.wasm` artifact that must be loaded
// via the wasm-pack glue at runtime, not pre-bundled).
export default defineConfig({
  resolve: {
    alias: [
      { find: /^@haiai\/wasm$/, replacement: path.join(haiaiWasmPkg, "index.js") },
      { find: /^@haiai\/wasm\/types$/, replacement: path.join(haiaiWasmPkg, "types.js") },
      { find: /^@haiai\/wasm\/worker$/, replacement: path.join(haiaiWasmPkg, "worker/index.js") },
      { find: /^@jacs\/wasm$/, replacement: path.join(jacsWasmPkg, "index.js") },
      { find: /^@jacs\/wasm\/worker$/, replacement: path.join(jacsWasmPkg, "worker/index.js") },
    ],
  },
  optimizeDeps: {
    exclude: ["@haiai/wasm", "@jacs/wasm"],
  },
  server: {
    fs: {
      // Allow both sibling local packages used by the file: deps above.
      allow: [path.resolve(__dirname, "../../.."), path.resolve(__dirname, "../../../../../wasm")],
    },
  },
  build: {
    target: "es2020",
  },
});
