import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("i18nextLng", "en"));
});

test("context is collapsed, read-only and independent of the task description", async ({
  page,
}) => {
  await page.goto("/?mode=card-context");
  await expect(
    page.getByRole("textbox", { name: "Task description" }),
  ).toHaveValue("New task");
  const preview = page.locator("textarea[readonly]");
  await expect(preview).toBeHidden();
  await page.locator("summary").click();
  await expect(preview).toBeVisible();
  await expect(preview).not.toBeEditable();
  await expect(
    page.getByRole("checkbox", { name: "LLM Wiki", exact: true }),
  ).toBeChecked();
  const shared = page.getByRole("checkbox", {
    name: "Shared directories",
    exact: true,
  });
  await expect(shared).toBeChecked();
  await expect(preview).toHaveValue(/\.evk-shared\/cache/);
  await expect(preview).toHaveValue(/\.evk-shared\/persistent/);
  await expect(preview).not.toHaveValue(/\.evk\//);
  await shared.uncheck();
  await expect(preview).not.toHaveValue(/\.evk-shared\/persistent/);
  await expect(preview).toHaveValue(/Consult prior knowledge/);
  await page
    .getByRole("textbox", { name: "Task description" })
    .fill("Edited task");
  await expect(page.getByTestId("stored-description")).toContainText(
    "Edited task",
  );
  await expect(page.getByTestId("stored-description")).toContainText(
    "<!-- vk:pipeline:start -->",
  );
  await expect(shared).not.toBeChecked();
});

test("legacy Wiki instructions are displayed separately without changing stored text", async ({
  page,
}) => {
  await page.goto("/?mode=card-context&legacy=1");
  const stored = await page.getByTestId("stored-description").textContent();
  await expect(
    page.getByRole("textbox", { name: "Task description" }),
  ).toHaveValue("Existing task");
  await page.locator("summary").click();
  await expect(
    page.getByRole("checkbox", { name: "LLM Wiki", exact: true }),
  ).toBeChecked();
  await expect(
    page.getByRole("checkbox", { name: "Shared directories", exact: true }),
  ).not.toBeChecked();
  await expect(page.getByTestId("stored-description")).toHaveText(stored!);
});
