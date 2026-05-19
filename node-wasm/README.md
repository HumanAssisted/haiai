# @haiai/wasm

Browser-only HAI API client (WASM) on top of [`@jacs/wasm`](https://www.npmjs.com/package/@jacs/wasm).

`@haiai/wasm` lets a web app authenticate, send signed email, drive
benchmark runs, and consume the HAI event stream — without a backend
proxy — by signing every request with a JACS agent that lives in the
browser tab.

## Install

```sh
npm install @haiai/wasm @jacs/wasm
```

Both packages share the same version. `@haiai/wasm` is the HAI
platform glue; `@jacs/wasm` is the underlying JACS protocol library.

## Quickstart

```ts
import { initHaiaiWasm, BrowserAgent, HaiaiWasmError } from "@haiai/wasm";

await initHaiaiWasm();

// Option 1: spin up a one-off agent (useful for tests / demos).
const agent = await BrowserAgent.createEphemeral("ed25519", {
  baseUrl: "https://hai.ai",
});

// Option 2: load an encrypted JACS agent the user imported via file picker.
// const material = await readFileAsText(uploadedJsonFile);
// const agent = await BrowserAgent.importEncrypted(material, password);

// Option 3: verify-only handle (no signing key).
// const verifier = await BrowserAgent.publicOnly("agent-id", "<pubkey-b64>", "ed25519");

// Sign a JSON payload locally — delegates to JACS in-browser.
const signed = agent.sign({ hello: "world" });

// Send an agent-signed email via the HAI API.
try {
  const result = await agent.client.sendSignedEmail({
    to: "alice@hai.ai",
    subject: "Hello from the browser",
    body: "Hi Alice — sent from a Web Worker.",
  });
  console.log("sent", result.message_id);
} catch (err) {
  if (err instanceof HaiaiWasmError && err.code === "Unauthorized") {
    // Token expired / wrong agent — prompt for re-login.
  } else {
    throw err;
  }
}

// Drive the event stream as an async iterator.
for await (const ev of agent.client.eventStream({
  transport: "sse",
  url: "https://hai.ai/api/v1/agents/connect",
  authHeader: agent.buildAuthHeader(Math.floor(Date.now() / 1000), crypto.randomUUID()),
})) {
  console.log(ev.event_type, ev.data);
}

agent.clearSecrets(); // zero the in-memory signer when done
```

## API

### Lifecycle

| Method | Returns | Notes |
|--------|---------|-------|
| `BrowserAgent.createEphemeral(algo, init?)` | `BrowserAgent` | `algo` is `"ed25519"` or `"pq2025"`. |
| `BrowserAgent.importEncrypted(materialJson, password, init?)` | `BrowserAgent` | `materialJson` is the JSON-serialised `AgentMaterial` blob the CLI produces. |
| `BrowserAgent.publicOnly(jacsId, pubkeyB64, algo, init?)` | `BrowserAgent` | Verifier-only — read-only HTTP calls still work. |
| `agent.clearSecrets()` | `void` | Drops the in-memory signer. Subsequent sign attempts throw `Locked`. |
| `agent.isUnlocked()` | `boolean` | |
| `agent.exportAgent()` | `unknown` | JSON-shape suitable for `BrowserAgent.publicOnly`. |
| `agent.publicKeyBase64()` | `string` | |
| `agent.algorithm()` | `"ed25519" \| "pq2025"` | |
| `agent.jacsId()` | `string` | |

### Local crypto (delegate to JACS)

- `agent.sign(payload)` → signed JACS document
- `agent.verify(signedDoc)` → `{ valid, status, ... }`
- `agent.signAgreement(agreement, role?)` / `agent.verifyAgreement(agreement)`
- `agent.canonicalJson(value)` — byte-identical to native
- `agent.buildAuthHeader(tsSeconds, nonce)` — byte-identical to native given the same inputs
- `agent.generateVerifyLink(document, baseUrl?)` — bounded by `MAX_VERIFY_URL_LEN`

### `BrowserHaiClient` (`agent.client`)

Typed wrappers around every `HaiClient` HTTP method. See
[fixtures/wasm_browser_surface.json](../fixtures/wasm_browser_surface.json)
for the full surface. Highlights:

- **Registration / identity**: `hello`, `register`, `rotateKeys`, `verifyStatus`, `updateUsername`, `deleteUsername`
- **Email send + inbox**: `sendEmail`, `sendSignedEmail`, `listMessages`, `getMessage`, `getRawEmail`, `markRead`, `markUnread`, `deleteMessage`, `archive`, `unarchive`, `getUnreadCount`, `getEmailStatus`
- **Reply / forward / search / contacts**: `reply`, `forward`, `searchMessages`, `contacts`
- **Email templates + raw signing**: `createEmailTemplate`, `listEmailTemplates`, `getEmailTemplate`, `updateEmailTemplate`, `deleteEmailTemplate`, `signEmailRaw`, `verifyEmailRaw`
- **Key & verification**: `fetchServerKeys`, `fetchRemoteKey`, `fetchKeyByHash`, `fetchKeyByEmail`, `fetchKeyByDomain`, `fetchAllKeys`, `verifyDocument`, `getVerification`, `verifyAgentDocument`
- **Benchmark RPC**: `benchmark`, `freeRun`, `proRun`, `dnsCertifiedRun`, `submitResponse`
- **Event stream**: `eventStream({ transport: "sse" | "ws", url, authHeader })` returns `AsyncIterableIterator<HaiEvent>`

### Web Worker

`import { createBrowserAgentWorker } from "@haiai/wasm/worker"` — runs
the wasm inside a `DedicatedWorker` so long ops (multi-MB email send,
pq2025 keygen, raw MIME hashing) don't block the main thread.

```ts
import { createBrowserAgentWorker } from "@haiai/wasm/worker";

const w = createBrowserAgentWorker({ baseUrl: "https://hai.ai" });
await w.ready;
await w.createEphemeral("ed25519");
const signed = await w.sign({ hello: "world" });
w.terminate();
```

### Shared types

```ts
import type { EmailMessage, SendEmailOptions, HaiEvent } from "@haiai/wasm/types";
```

## Error codes

Every rejection is a `HaiaiWasmError` with a stable `.code`:

| Code | When |
|------|------|
| `InvalidPassword` | `importEncrypted` with the wrong password. |
| `MalformedEnvelope` / `MalformedKey` / `MalformedDocument` | Input shape wrong. |
| `UnsupportedAlgorithm` | Algorithm string not `"ed25519"`/`"pq2025"`. |
| `Locked` | Sign call after `clearSecrets`. |
| `SignatureInvalid` | Verify path detected tampering. |
| `Validation` | HaiClient input validation. |
| `BadRequest` (400) / `Unauthorized` (401/403) / `NotFound` (404) / `Timeout` (408) / `RateLimited` (429) / `ServerError` (5xx) | HAI API HTTP. |
| `Network` | Underlying `fetch` failure. |
| `MalformedResponse` | Server returned invalid JSON. |
| `Provider` | JACS provider internal. |
| `VerifyLinkTooLarge` | `generateVerifyLink` exceeded length cap. |
| `MissingHostedDocumentId` | Hosted verify mode with no document id. |
| `Unsupported` | Backend doesn't support the method. |
| `Internal` | Unknown / unmapped fallback. |

## Security & limitations

- **No server logs in the browser tab.** All telemetry stays client-side. `agent.metrics()` returns counters synchronously.
- **No env vars.** Configuration goes through constructor options only.
- **Debug logging is off by default.** Set `globalThis.HAIAI_WASM_DEBUG = true` to enable `console.debug` lines from the wasm layer.
- **Browser memory is JS-accessible by design.** Use `clearSecrets()` aggressively when you're done; consider running long-lived sessions inside the Web Worker so the main thread can't introspect.
- **No streaming bodies on wasm.** Reqwest's wasm shim collapses the body to bytes — attachments must be base64-encoded in JSON.
- **localStorage only.** No IndexedDB / file system. Encrypted-agent persistence goes through `@jacs/wasm`'s `localStore`.
- **CORS dependency.** HAI API must allow your origin (`Access-Control-Allow-Origin`) and the `Authorization` header.
- **WebSocket auth (interim).** Browsers can't set custom headers on WS handshake, so the wasm layer encodes the JACS auth header as a `?auth=<encoded>` query parameter. `build_authenticated_ws_url` refuses non-`wss://` URLs (returns `ConfigInvalid`) so the token never goes on the wire in cleartext, but it does end up in server access logs / reverse-proxy logs and the browser's DevTools Network tab. The clean fix is a first-frame auth message (Option C) — the client opens an unauthed `wss://` connection, then sends `{"type":"auth","token":"<header>"}` as the first text frame; hai/api validates and either flips the connection authenticated or closes with code 4401. Tracked alongside the other backend assumptions in `docs/HAIAI_WASM_BACKEND_ASSUMPTIONS.md`.

## Build & test

```sh
# Build from source.
wasm-pack build --target web --release rust/haiai-wasm
bash node-wasm/scripts/finalize-pkg.sh
# → rust/haiai-wasm/pkg/ is publishable

# Type-check the wrapper from a fresh checkout (no wasm-pack output needed).
tsc --noEmit -p node-wasm/tsconfig.json

# Vite + Playwright smoke (after the wasm-pack output is produced):
cd node-wasm/examples/vite-smoke && npm install && npm test
```

## License

BUSL-1.1
