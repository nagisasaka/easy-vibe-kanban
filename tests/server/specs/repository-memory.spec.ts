import { expect, test } from "@playwright/test";

test("existing Wiki adoption, scoped requests and initialisation use the saved repository configuration", async ({
  page,
}) => {
  test.skip(
    !process.env.LVK_E2E_SOURCE_ROOT,
    "Requires the development Vite server",
  );
  const state = {
    enabled: true,
    status: "stale",
    wiki_exists: true,
    target_branch: "main",
    output_language: "ja",
    last_success: null,
    active_run_id: null,
    bootstrap: null,
    coding_errors: [],
    unverified_before: null,
  };
  const requests: unknown[] = [];
  await page.route(
    "**/api/repos/adoption-regression/memory{,/sync}",
    async (route) => {
      if (route.request().method() === "POST") {
        requests.push(route.request().postDataJSON());
      } else if (route.request().method() === "PUT") {
        Object.assign(state, route.request().postDataJSON());
        state.wiki_exists = state.target_branch === "main";
        state.status = state.wiki_exists ? "stale" : "uninitialized";
      }
      await route.fulfill({ json: { success: true, data: state } });
    },
  );
  await page.goto("/workspaces/create", { waitUntil: "load" });
  await expect(page.getByRole("button").first()).toBeVisible({
    timeout: 120_000,
  });
  await page.keyboard.press("Escape");
  await page.evaluate(async (path) => {
    (await import(/* @vite-ignore */ path)).mount();
  }, `/@fs${process.env.LVK_E2E_SOURCE_ROOT}/tests/server/fixtures/repository-memory.tsx`);
  const panel = page.getByRole("region", {
    name: "Repository memory regression",
  });
  const adopt = panel.getByRole("button", {
    name: "Adopt existing Wiki",
    exact: true,
  });
  await expect(adopt).toBeEnabled();
  await expect(
    panel.getByRole("button", { name: "Initialize Wiki", exact: true }),
  ).toHaveCount(0);
  expect(requests).toHaveLength(0);
  await adopt.click();
  await expect.poll(() => requests.length).toBe(1);
  expect(requests[0]).toEqual({ from_commit: null });
  await panel
    .getByRole("button", { name: "Compare with current main", exact: true })
    .click();
  await page
    .getByRole("menuitem", { name: "Changes since a commit", exact: true })
    .click();
  await expect(adopt).toBeDisabled();
  await panel.getByPlaceholder("Commit SHA").fill("HEAD~1");
  await expect(adopt).toBeDisabled();
  await panel.getByPlaceholder("Commit SHA").fill("1bd490d1");
  await adopt.click();
  await expect.poll(() => requests.length).toBe(2);
  expect(requests[1]).toEqual({ from_commit: "1bd490d1" });
  await panel.getByRole("textbox").first().fill("new-branch");
  await expect(adopt).toBeDisabled();
  await panel
    .getByRole("button", { name: "Save settings", exact: true })
    .click();
  const initialize = panel.getByRole("button", {
    name: "Initialize Wiki",
    exact: true,
  });
  await expect(initialize).toBeEnabled();
  await expect(panel.getByPlaceholder("Commit SHA")).toHaveCount(0);
  await initialize.click();
  await expect.poll(() => requests.length).toBe(3);
  expect(requests[2]).toEqual({ from_commit: null });
  // HTTP is intercepted: no model, maintenance owner or canonical Wiki is touched.
});
