// HAIAI_WASM_PRD §5.5 / Task 036 + Issue 010 follow-up — Vite smoke entry.
//
// Verifies that the published `@haiai/wasm` shape:
//   1. Bundles cleanly under Vite.
//   2. Loads the wasm artifact in the browser.
//   3. Calls `BrowserAgent.createEphemeral` + `client.hello()` against
//      a mocked fetch (so the smoke runs without network).
//   4. **Issue 010 follow-up**: drives the TS `BrowserAgent.save/load`
//      facade end-to-end through `@jacs/wasm`'s localStorage path,
//      proving the wrapper that the wasm-pack tests cannot reach.
//   5. **Issue 010 follow-up**: drives `client.eventStream({transport})`
//      against a mocked SSE transport so a real `EventStreamHandle`
//      iterates three events. The same flow is exercised through both
//      `client.connectSse(url, authHeader)` (override path) and the
//      typed iterator surface so a regression at either layer trips
//      the assertion.

import { BrowserAgent, initHaiaiWasm, version, HaiaiWasmError } from "@haiai/wasm";

interface SmokeReport {
  status: string;
  helloMessage: string;
  jacsId: string;
  // (4) TS save/load round-trip
  saveLoad: {
    ok: boolean;
    error?: string;
    originalJacsId?: string;
    loadedJacsId?: string;
  };
  // (5) SSE eventStream iteration
  sseStream: {
    ok: boolean;
    error?: string;
    eventTypes: string[];
  };
}

declare global {
  interface Window {
    __haiaiSmoke: SmokeReport;
  }
}

// Stub `fetch` so the smoke runs offline. Vite serves over http://localhost:5173/
// but the BrowserAgent fires requests at https://hai.ai/ unless overridden.
// We intercept:
//   - /api/v1/agents/hello — synthetic HelloResult
//   - /api/v1/agents/{id}/events (SSE) — a multi-event stream that the
//     `EventStreamHandle` will iterate via `nextEvent()`.
//
// Anything else falls through to the original fetch (which will fail
// fast against the Vite dev server — we never want production HTTP
// from the smoke).
const originalFetch = window.fetch.bind(window);
function responseWithUrl(body: BodyInit | null, init: ResponseInit, url: string): Response {
  const response = new Response(body, init);
  Object.defineProperty(response, "url", { value: url });
  return response;
}

window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
  const url =
    typeof input === "string"
      ? input
      : input instanceof URL
        ? input.toString()
        : input.url;

  if (url.includes("/api/v1/agents/hello")) {
    return responseWithUrl(
      JSON.stringify({
        timestamp: new Date().toISOString(),
        client_ip: "127.0.0.1",
        hai_public_key_fingerprint: "test",
        message: `hello from @haiai/wasm v${version()}`,
        hai_signed_ack: "stub-ack",
        hello_id: "smoke-1",
      }),
      { headers: { "Content-Type": "application/json" } },
      url,
    );
  }

  // Mocked SSE stream — three back-to-back events, then close. The
  // wasm `SseParser` chunks them apart. We send the canonical
  // `data: <json>\n\n` per the SSE spec.
  if (url.includes("/events") || url.includes("/sse")) {
    const body =
      "data: {\"type\":\"smoke-1\",\"seq\":1}\n\n" +
      "data: {\"type\":\"smoke-2\",\"seq\":2}\n\n" +
      "data: {\"type\":\"smoke-3\",\"seq\":3}\n\n";
    return responseWithUrl(
      body,
      {
        headers: { "Content-Type": "text/event-stream" },
      },
      url,
    );
  }

  return originalFetch(input, init);
};

const SAVE_KEY = "haiai-wasm-vite-smoke-save-load";
const SAVE_PASSWORD = "vite-smoke-PASSWORD-9876543210!";
const SMOKE_BASE_URL = window.location.origin;

async function runSaveLoadFacade(): Promise<SmokeReport["saveLoad"]> {
  try {
    // Use a fresh ephemeral agent so the save/load round-trip is the
    // only thing under test.
    const original = await BrowserAgent.createEphemeral("ed25519");
    const originalJacsId = original.jacsId();

    // TS `BrowserAgent.save(storageKey, password)` →
    // exportEncrypted(password) → @jacs/wasm localStore.saveEncryptedAgent.
    original.save(SAVE_KEY, SAVE_PASSWORD);

    // TS `BrowserAgent.load(storageKey, { password })` →
    // @jacs/wasm localStore.loadEncryptedAgent → importEncrypted(...).
    const loaded = await BrowserAgent.load(SAVE_KEY, { password: SAVE_PASSWORD });
    if (loaded === null) {
      return { ok: false, error: "BrowserAgent.load returned null after save" };
    }
    const loadedJacsId = loaded.jacsId();
    if (loadedJacsId !== originalJacsId) {
      return {
        ok: false,
        error: `loaded jacsId ${loadedJacsId} ≠ original ${originalJacsId}`,
        originalJacsId,
        loadedJacsId,
      };
    }
    // Wrong-password load must surface as a typed HaiaiWasmError.
    let wrongPasswordError: HaiaiWasmError | null = null;
    try {
      await BrowserAgent.load(SAVE_KEY, { password: "WRONG" });
    } catch (e) {
      wrongPasswordError = e as HaiaiWasmError;
    }
    if (!wrongPasswordError) {
      return {
        ok: false,
        error: "wrong-password load did not throw",
        originalJacsId,
        loadedJacsId,
      };
    }
    if (typeof wrongPasswordError.code !== "string") {
      return {
        ok: false,
        error: `wrong-password threw non-typed error: ${String(wrongPasswordError)}`,
        originalJacsId,
        loadedJacsId,
      };
    }
    return { ok: true, originalJacsId, loadedJacsId };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
}

async function runSseEventStream(): Promise<SmokeReport["sseStream"]> {
  try {
    const agent = await BrowserAgent.createEphemeral("ed25519", { baseUrl: SMOKE_BASE_URL });
    // Drive the eventStream() iterator with an explicit URL+auth (the
    // override path) so we can point the mocked fetch above at it.
    // The `transport: "sse"` arm calls EventStreamHandle.openSse(url, auth).
    const url = `http://localhost:5173/api/v1/agents/${agent.jacsId()}/events`;
    const authHeader = agent.buildAuthHeader(Math.floor(Date.now() / 1000), "vite-smoke-nonce");
    const stream = agent.client.eventStream({
      transport: "sse",
      url,
      authHeader,
    });
    const seen: string[] = [];
    // Bound the loop so a stream that never ends does not hang the smoke.
    for (let i = 0; i < 10; i++) {
      const { value, done } = await stream.next();
      if (done) break;
      const ev = value as { event_type?: string };
      if (ev && typeof ev.event_type === "string") {
        seen.push(ev.event_type);
      }
      if (seen.length >= 3) break;
    }
    await stream.return?.();
    if (seen.length < 3) {
      return {
        ok: false,
        error: `expected ≥3 SSE events, got ${seen.length}`,
        eventTypes: seen,
      };
    }
    return { ok: true, eventTypes: seen };
  } catch (e) {
    return { ok: false, error: String(e), eventTypes: [] };
  }
}

async function run(): Promise<void> {
  const statusEl = document.getElementById("status")!;
  const helloEl = document.getElementById("hello-result")!;
  const jacsEl = document.getElementById("jacs-id")!;
  try {
    statusEl.textContent = "initializing";
    await initHaiaiWasm();
    statusEl.textContent = "agent";
    const agent = await BrowserAgent.createEphemeral("ed25519", { baseUrl: SMOKE_BASE_URL });
    jacsEl.textContent = agent.jacsId();
    statusEl.textContent = "hello";
    const hello = await agent.client.hello(false);
    helloEl.textContent = hello.message;
    statusEl.textContent = "save/load";
    const saveLoad = await runSaveLoadFacade();
    statusEl.textContent = "sse";
    const sseStream = await runSseEventStream();
    statusEl.textContent = "done";
    window.__haiaiSmoke = {
      status: "done",
      helloMessage: hello.message,
      jacsId: agent.jacsId(),
      saveLoad,
      sseStream,
    };
  } catch (err) {
    statusEl.textContent = `error: ${String(err)}`;
    window.__haiaiSmoke = {
      status: "error",
      helloMessage: String(err),
      jacsId: "",
      saveLoad: { ok: false, error: "smoke crashed before save/load" },
      sseStream: { ok: false, error: "smoke crashed before sse", eventTypes: [] },
    };
  }
}

run();
