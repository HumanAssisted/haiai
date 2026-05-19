// HAIAI_WASM_PRD §5.5 / Task 036 — Vite smoke entry.
//
// Verifies that the published `@haiai/wasm` shape:
//   1. Bundles cleanly under Vite.
//   2. Loads the wasm artifact in the browser.
//   3. Calls `BrowserAgent.createEphemeral` + `client.hello()` against
//      a mocked fetch (so the smoke runs without network).

import { BrowserAgent, initHaiaiWasm, version } from "@haiai/wasm";

declare global {
  interface Window {
    __haiaiSmoke: { status: string; helloMessage: string; jacsId: string };
  }
}

// Stub `fetch` so the smoke runs offline. Vite serves over http://localhost:5173/
// but the BrowserAgent fires requests at https://hai.ai/ unless overridden.
// We intercept any URL containing `/api/v1/agents/hello` and return a
// synthetic HelloResult.
const originalFetch = window.fetch.bind(window);
window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
  const url = typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
  if (url.includes("/api/v1/agents/hello")) {
    return new Response(
      JSON.stringify({
        timestamp: new Date().toISOString(),
        client_ip: "127.0.0.1",
        hai_public_key_fingerprint: "test",
        message: `hello from @haiai/wasm v${version()}`,
        hai_signed_ack: "stub-ack",
        hello_id: "smoke-1",
      }),
      { headers: { "Content-Type": "application/json" } },
    );
  }
  return originalFetch(input, init);
};

async function run(): Promise<void> {
  const statusEl = document.getElementById("status")!;
  const helloEl = document.getElementById("hello-result")!;
  const jacsEl = document.getElementById("jacs-id")!;
  try {
    statusEl.textContent = "initializing";
    await initHaiaiWasm();
    statusEl.textContent = "agent";
    const agent = await BrowserAgent.createEphemeral("ed25519");
    jacsEl.textContent = agent.jacsId();
    statusEl.textContent = "hello";
    const hello = await agent.client.hello(false);
    helloEl.textContent = hello.message;
    statusEl.textContent = "done";
    window.__haiaiSmoke = {
      status: "done",
      helloMessage: hello.message,
      jacsId: agent.jacsId(),
    };
  } catch (err) {
    statusEl.textContent = `error: ${String(err)}`;
    window.__haiaiSmoke = {
      status: "error",
      helloMessage: String(err),
      jacsId: "",
    };
  }
}

run();
