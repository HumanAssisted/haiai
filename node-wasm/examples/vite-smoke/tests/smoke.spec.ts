import { expect, test } from "@playwright/test";

// HAIAI_WASM_PRD §5.5 + Issue 010 follow-up. The smoke entry in
// `src/main.ts` performs three checks in sequence and writes the
// outcome to `window.__haiaiSmoke`:
//
//   1. hello()
//   2. TS `BrowserAgent.save / .load` round-trip
//   3. `client.eventStream({transport: "sse"})` against a mocked SSE
//      response (the same `window.fetch` shim handles both /hello and
//      /events).
//
// Each Playwright assertion below maps to one of those.

type SmokeReport = {
  status?: string;
  helloMessage?: string;
  jacsId?: string;
  saveLoad?: {
    ok: boolean;
    error?: string;
    originalJacsId?: string;
    loadedJacsId?: string;
  };
  sseStream?: {
    ok: boolean;
    error?: string;
    eventTypes: string[];
  };
};

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "Authorization, Content-Type",
  "Access-Control-Allow-Methods": "GET,POST,OPTIONS",
};

test("haiai wasm smoke covers hello + TS save/load + SSE eventStream", async ({ page }) => {
  await page.route("**/api/v1/agents/hello", async (route) => {
    if (route.request().method() === "OPTIONS") {
      await route.fulfill({ status: 204, headers: corsHeaders });
      return;
    }
    await route.fulfill({
      status: 200,
      headers: {
        ...corsHeaders,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        timestamp: new Date().toISOString(),
        client_ip: "127.0.0.1",
        hai_public_key_fingerprint: "test",
        message: "hello from @haiai/wasm playwright route",
        hai_signed_ack: "stub-ack",
        hello_id: "smoke-1",
      }),
    });
  });
  await page.route("**/api/v1/agents/**/events", async (route) => {
    if (route.request().method() === "OPTIONS") {
      await route.fulfill({ status: 204, headers: corsHeaders });
      return;
    }
    await route.fulfill({
      status: 200,
      headers: {
        ...corsHeaders,
        "Content-Type": "text/event-stream",
      },
      body:
        "data: {\"type\":\"smoke-1\",\"seq\":1}\n\n" +
        "data: {\"type\":\"smoke-2\",\"seq\":2}\n\n" +
        "data: {\"type\":\"smoke-3\",\"seq\":3}\n\n",
    });
  });

  await page.goto("/");
  // Wait for the smoke script to populate window.__haiaiSmoke.
  await page.waitForFunction(
    () => (window as unknown as { __haiaiSmoke?: { status?: string } }).__haiaiSmoke?.status === "done",
    null,
    { timeout: 30_000 },
  );
  await expect(page.locator("#status")).toHaveText("done");

  const helloText = await page.locator("#hello-result").textContent();
  expect(helloText).toContain("hello from @haiai/wasm");
  const jacsId = await page.locator("#jacs-id").textContent();
  expect(jacsId?.length ?? 0).toBeGreaterThan(0);

  // Pull the full smoke report and assert each Issue 010 sub-check.
  const report: SmokeReport = await page.evaluate(
    () => (window as unknown as { __haiaiSmoke: SmokeReport }).__haiaiSmoke,
  );

  // (TS facade) BrowserAgent.save → BrowserAgent.load round-trip
  expect(report.saveLoad?.ok, `saveLoad failed: ${report.saveLoad?.error ?? "(no error)"}`).toBe(true);
  expect(report.saveLoad?.originalJacsId).toBeTruthy();
  expect(report.saveLoad?.loadedJacsId).toBe(report.saveLoad?.originalJacsId);

  // (SSE) eventStream iterator pulled three events from the mocked SSE response.
  expect(report.sseStream?.ok, `sseStream failed: ${report.sseStream?.error ?? "(no error)"}`).toBe(true);
  expect(report.sseStream?.eventTypes ?? []).toEqual(["smoke-1", "smoke-2", "smoke-3"]);
});
