import { expect, test, type Page } from "@playwright/test";

async function mount(page: Page) {
  await page.goto("/workspaces/create", { waitUntil: "load" });
  await expect(page.getByRole("button").first()).toBeVisible({
    timeout: 120_000,
  });
  await page.keyboard.press("Escape");
  await page.evaluate(async (path) => {
    (await import(/* @vite-ignore */ path)).mount();
  }, `/@fs${process.env.LVK_E2E_SOURCE_ROOT}/tests/server/fixtures/async-questions.tsx`);
  return page.getByRole("region", { name: "Async question regression" });
}

test("asynchronous questions preserve drafts, deliver while running, and follow up after completion", async ({
  page,
}) => {
  test.skip(
    !process.env.LVK_E2E_SOURCE_ROOT,
    "Requires the development Vite server",
  );
  const sent: Array<{ path: string; content: string }> = [];
  let reject = true;
  await page.route("**/__async-question-test/*", async (route) => {
    sent.push({
      path: new URL(route.request().url()).pathname,
      content: route.request().postDataJSON().content,
    });
    await route.fulfill({ status: reject ? 409 : 200, json: {} });
  });
  let panel = await mount(page);
  await panel.getByRole("button", { name: /unanswered|未回答/ }).click();
  await panel.getByRole("button", { name: "Blue", exact: true }).click();
  expect(sent).toHaveLength(0); // Selection alone must never submit an answer.
  await expect(panel.getByLabel("Which colour?")).toHaveValue("Blue");
  await panel.getByLabel("Any constraints?").fill("Keep the existing layout");
  await panel.getByLabel("Normal message").fill("Unrelated draft");
  await panel.getByRole("button", { name: "Continue work" }).click();
  await expect(panel.getByLabel("Work progress")).toHaveText("1");
  await panel
    .getByRole("button", { name: /Send answer|回答を送信/ })
    .first()
    .click();
  await expect(panel.getByRole("alert")).toHaveText("Test delivery rejected");
  await expect(panel.getByLabel("Which colour?")).toHaveValue("Blue");
  await expect(panel.getByLabel("Normal message")).toHaveValue(
    "Unrelated draft",
  );
  expect(sent[0].path).toContain("/steer");
  reject = false;
  await panel
    .getByRole("button", { name: /Send answer|回答を送信/ })
    .first()
    .click();
  await expect(panel.getByLabel("Which colour?")).toHaveCount(0);
  await expect(panel.getByLabel("Any constraints?")).toHaveValue(
    "Keep the existing layout",
  );
  panel = await mount(page); // Reload: retain answered state and the remaining draft.
  await panel.getByRole("button", { name: /unanswered|未回答/ }).click();
  await expect(panel.getByLabel("Which colour?")).toHaveCount(0);
  await expect(panel.getByLabel("Any constraints?")).toHaveValue(
    "Keep the existing layout",
  );
  await panel.getByRole("button", { name: "Finish run" }).click();
  await panel.getByRole("button", { name: /Send answer|回答を送信/ }).click();
  await expect(
    panel.getByRole("button", { name: /unanswered|未回答/ }),
  ).toHaveCount(0);
  expect(sent.at(-1)).toEqual({
    path: "/__async-question-test/follow-up",
    content: "Question: Any constraints?\nAnswer: Keep the existing layout",
  });
  await panel.getByRole("button", { name: "Switch session" }).click();
  await panel.getByRole("button", { name: /unanswered|未回答/ }).click();
  await expect(panel.getByLabel("Which colour?")).toHaveValue("");
});

test("a delayed answer stays with its original session", async ({ page }) => {
  test.skip(
    !process.env.LVK_E2E_SOURCE_ROOT,
    "Requires the development Vite server",
  );
  let release!: () => void;
  const accepted = new Promise<void>((resolve) => {
    release = resolve;
  });
  let requests = 0;
  await page.route("**/__async-question-test/*", async (route) => {
    requests += 1;
    await accepted;
    await route.fulfill({ status: 200, json: {} });
  });
  const panel = await mount(page);
  await panel.getByRole("button", { name: /unanswered|未回答/ }).click();
  await panel.getByRole("button", { name: "Blue", exact: true }).click();
  await panel
    .getByRole("button", { name: /Send answer|回答を送信/ })
    .first()
    .click();
  await expect.poll(() => requests).toBe(1);
  await expect(
    panel.getByRole("button", { name: /Sending|送信中/ }),
  ).toBeDisabled();
  await panel.getByRole("button", { name: "Switch session" }).click();
  await panel.getByRole("button", { name: /unanswered|未回答/ }).click();
  await panel.getByLabel("Which colour?").fill("Other session draft");
  release();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          JSON.parse(
            localStorage.getItem("lvk:async-questions:regression:a") ?? "{}",
          ).answered?.length,
      ),
    )
    .toBe(1);
  await expect(panel.getByLabel("Which colour?")).toHaveValue(
    "Other session draft",
  );
  await panel.getByRole("button", { name: "Switch session" }).click();
  await panel.getByRole("button", { name: /unanswered|未回答/ }).click();
  await expect(panel.getByLabel("Which colour?")).toHaveCount(0);
  expect(requests).toBe(1);
});
