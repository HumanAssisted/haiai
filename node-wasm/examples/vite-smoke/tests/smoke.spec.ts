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

test("haiai wasm smoke covers hello + TS save/load + SSE eventStream", async ({ page }) => {
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
