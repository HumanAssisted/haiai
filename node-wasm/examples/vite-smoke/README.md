# @haiai/wasm Vite + Playwright smoke

HAIAI_WASM_PRD §5.5 / Task 036.

Smoke fixture that:

1. Builds with [Vite](https://vitejs.dev/) using the published
   `@haiai/wasm` shape via a `file:` dependency.
2. Renders `BrowserAgent.createEphemeral("ed25519") +
   client.hello()` against a `window.fetch` stub (no production
   network).
3. Asserts the result with Playwright.

## Run locally

```sh
# 1. Build the wasm artifact + finalize the package.
wasm-pack build --target web --release rust/haiai-wasm
bash node-wasm/scripts/finalize-pkg.sh

# 2. Install + run the smoke.
cd node-wasm/examples/vite-smoke
npm install
npx playwright install chromium
npm test
```

CI runs the same flow via the wasm-checks workflow (Task 038).
