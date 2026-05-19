# @haiai/wasm

Browser-only HAI API client (WASM) on top of [`@jacs/wasm`](https://www.npmjs.com/package/@jacs/wasm).

## Status

**Skeleton.** The published surface lands across Tasks 031–036 of
[HAIAI_WASM_PRD.md](../docs/jacs/HAIAI_WASM_PRD.md):

- Task 031 — TS wrapper skeleton (this commit)
- Task 032 — `BrowserAgent` lifecycle + local-crypto class
- Task 033 — `BrowserHaiClient` HTTP wrappers + `AsyncIterableIterator` event stream
- Task 034 — `@haiai/wasm/types` subpath with shared TS types
- Task 035 — `@haiai/wasm/worker` Web Worker subpath
- Task 036 — Vite + Playwright smoke fixture

Until those land, the only callable surface is:

- `initHaiaiWasm()` — async init, idempotent
- `version()` — package version string
- `about()` — one-line build descriptor
- `HaiaiWasmError` — typed error with `.code` discriminator

`BrowserAgent.*` is a sentinel that throws `HaiaiWasmError { code: "NotImplemented" }`.

## Install (when published)

```sh
npm install @haiai/wasm @jacs/wasm
```

```ts
import { initHaiaiWasm, BrowserAgent } from "@haiai/wasm";

await initHaiaiWasm();
const agent = await BrowserAgent.load("my-agent", { password: pw });
await agent.client.sendSignedEmail({ to: "other@hai.ai", subject: "Hi", body: "Hello" });
```

## Build

```sh
wasm-pack build --target web --release rust/haiai-wasm
bash node-wasm/scripts/finalize-pkg.sh
# → rust/haiai-wasm/pkg/ is publishable
```

## Type-check the wrapper from a fresh checkout

```sh
tsc --noEmit -p node-wasm/tsconfig.json
```

The `node-wasm/haiai_wasm.d.ts` checked-in stub mirrors the wasm-pack
output so `tsc` works without first running `wasm-pack build`.

## License

BUSL-1.1
