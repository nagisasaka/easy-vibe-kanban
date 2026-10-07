import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

test("the image browser runtime and repository tests pin the same Playwright revision", async () => {
  const root = JSON.parse(
    await readFile(new URL("../../package.json", import.meta.url)),
  );
  const runtime = JSON.parse(
    await readFile(new URL("../browser/package.json", import.meta.url)),
  );
  const lock = JSON.parse(
    await readFile(new URL("../browser/package-lock.json", import.meta.url)),
  );
  assert.equal(
    root.devDependencies["@playwright/test"],
    runtime.dependencies.playwright,
  );
  assert.equal(
    lock.packages["node_modules/playwright"].version,
    runtime.dependencies.playwright,
  );
});
