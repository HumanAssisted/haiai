import { defineConfig } from "@playwright/test";

// HAIAI_WASM_PRD §5.5 — Playwright drives Chromium against the Vite
// dev server, asserts `#hello-result` renders.
export default defineConfig({
  testDir: "./tests",
  use: {
    baseURL: "http://localhost:5173",
    headless: true,
  },
  webServer: {
    command: "npm run dev",
    port: 5173,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
