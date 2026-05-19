import { expect, test } from "@playwright/test";

test("haiai wasm smoke renders hello result", async ({ page }) => {
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
});
