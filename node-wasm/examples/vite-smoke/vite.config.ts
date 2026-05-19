import { defineConfig } from "vite";

// HAIAI_WASM_PRD §5.5: Vite is the smoke-tested bundler. The
// `optimizeDeps.exclude` keeps the `@haiai/wasm` package out of Vite's
// pre-bundling (it ships its own `.wasm` artifact that must be loaded
// via the wasm-pack glue at runtime, not pre-bundled).
export default defineConfig({
  optimizeDeps: {
    exclude: ["@haiai/wasm"],
  },
  server: {
    fs: {
      // Allow reading the parent (..) so file: dependency resolves.
      allow: [".."],
    },
  },
  build: {
    target: "es2020",
  },
});
