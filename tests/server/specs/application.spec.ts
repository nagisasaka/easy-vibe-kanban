import { expect, test } from "@playwright/test";

// Read-only checks: safe for the development app without creating paid agent runs.
test("the deployed application renders and reaches its real backend", async ({
  page,
  request,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const response = await page.goto("/", { waitUntil: "domcontentloaded" });
  expect(response?.status()).toBe(200);
  await expect(page).toHaveTitle(/Lucky Vibe Kanban/i);
  await expect(page.locator("#root")).not.toBeEmpty();
  await expect(page.locator("#root")).toContainText(/\S/);
  await expect(page.getByRole("button").first()).toBeVisible();
  const info = await request.get("/api/info");
  expect(info.status()).toBe(200);
  expect((await info.json()).success).toBe(true);
  await page.reload();
  await expect(page.locator("#root")).toContainText(/\S/);
  expect(errors).toEqual([]);
});

test("HTTPS ingress requires authentication", async ({ baseURL }) => {
  test.skip(!baseURL?.startsWith("https:"), "Internal disposable HTTP fixture");
  // Playwright's request factory inherits configured HTTP credentials. Use an
  // independent request to verify the unauthenticated ingress path.
  expect((await fetch(new URL("/api/info", baseURL))).status).toBe(401);
});
