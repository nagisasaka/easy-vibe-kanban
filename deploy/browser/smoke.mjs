// Also used by CI/the host updater against a disposable app database.
import assert from "node:assert/strict";
import { readFile, mkdir } from "node:fs/promises";
import { chromium } from "playwright";

const url = process.env.LVK_E2E_BASE_URL;
if (!url) throw new Error("Set LVK_E2E_BASE_URL to the application to verify");
const credentials = process.env.LVK_E2E_CREDENTIALS_FILE
  ? JSON.parse(await readFile(process.env.LVK_E2E_CREDENTIALS_FILE, "utf8"))
  : undefined;
const browser = await chromium.launch();
// A cold Vite checkout can take longer than a bundled release. Bound the whole
// probe while still closing Chromium before the host command's timeout.
const watchdog = setTimeout(
  () => void browser.close().catch(() => {}),
  150_000,
);
try {
  const context = await browser.newContext({
    httpCredentials: credentials
      ? {
          username: credentials.username,
          password: credentials.password,
          origin: new URL(url).origin,
        }
      : undefined,
  });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const response = await page.goto(url, {
    waitUntil: "commit",
    timeout: 30_000,
  });
  assert.equal(response?.status(), 200);
  assert.match(await page.title(), /Lucky Vibe Kanban/i);
  await page.waitForFunction(
    () => document.querySelector("#root")?.textContent?.trim(),
    undefined,
    { timeout: 120_000 },
  );
  await page
    .getByRole("button")
    .first()
    .waitFor({ state: "visible", timeout: 120_000 });
  const info = await context.request.get(new URL("/api/info", url).href);
  assert.equal(info.status(), 200);
  assert.equal((await info.json()).success, true);
  assert.deepEqual(errors, [], "Browser JavaScript errors");
  if (process.env.LVK_E2E_OUTPUT_DIR) {
    await mkdir(process.env.LVK_E2E_OUTPUT_DIR, { recursive: true });
    await page.screenshot({
      path: `${process.env.LVK_E2E_OUTPUT_DIR}/application.png`,
      fullPage: true,
    });
  }
  console.log(
    JSON.stringify({
      browser: browser.version(),
      title: await page.title(),
      url,
      errors,
    }),
  );
} finally {
  clearTimeout(watchdog);
  await browser.close();
}
