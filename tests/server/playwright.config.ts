import { readFileSync } from "node:fs";
import { defineConfig, devices } from "@playwright/test";

const baseURL = process.env.LVK_E2E_BASE_URL;
if (!baseURL)
  throw new Error(
    "Set LVK_E2E_BASE_URL (for example, https://your-server:8443)",
  );
const credentialsFile = process.env.LVK_E2E_CREDENTIALS_FILE;
const httpCredentials = credentialsFile
  ? JSON.parse(readFileSync(credentialsFile, "utf8"))
  : undefined;

export default defineConfig({
  testDir: "./specs",
  outputDir: "../../test-results/server",
  workers: 1,
  timeout: 240_000,
  expect: { timeout: 30_000 },
  reporter: [
    ["list"],
    ["html", { outputFolder: "../../playwright-report/server", open: "never" }],
  ],
  use: {
    ...devices["Desktop Chrome"],
    baseURL,
    httpCredentials: httpCredentials
      ? {
          username: httpCredentials.username,
          password: httpCredentials.password,
          origin: new URL(baseURL).origin,
        }
      : undefined,
    screenshot: "only-on-failure",
    // Traces can contain credentials and application content; opt in locally.
    trace: process.env.LVK_E2E_TRACE === "1" ? "retain-on-failure" : "off",
  },
});
