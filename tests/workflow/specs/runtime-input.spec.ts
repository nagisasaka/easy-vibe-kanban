import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

for (const runtime of ["remote", "local"] as const) {
  test(`${runtime}: reload before the draft debounce preserves text and isolates Session identities`, async ({
    page,
  }) => {
    if (runtime === "local") {
      await page.routeWebSocket(
        "**/api/scratch/DRAFT_FOLLOW_UP/*/stream/ws",
        (ws) => {
          ws.send(JSON.stringify({ Ready: true }));
        },
      );
      await page.route("**/api/scratch/DRAFT_FOLLOW_UP/*", (route) =>
        route.fulfill({
          status: 503,
          json: { success: false, message: "scratch temporarily unavailable" },
        }),
      );
    }
    await page.goto(
      `/?mode=runtime-input${runtime === "local" ? "&local=1" : ""}`,
    );
    await expect(page.getByTestId("scratch-ready")).toHaveText("true");
    // Freeze the debounce, not the input event: the server/local scratch still
    // holds its old value when the document is replaced.
    await page.clock.install();
    await page.clock.pauseAt(new Date());
    await page.getByLabel("Draft").fill("not yet acknowledged by scratch");
    await page.reload();
    await expect(page.getByLabel("Draft")).toHaveValue(
      "not yet acknowledged by scratch",
    );
    await page.getByLabel("Identity").selectOption("b");
    await expect(page.getByLabel("Draft")).toHaveValue("");
    await page.getByLabel("Identity").selectOption("a");
    await expect(page.getByLabel("Draft")).toHaveValue(
      "not yet acknowledged by scratch",
    );
  });
}

async function deferredSend(page: Page, failure = false) {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  let received!: () => void;
  const started = new Promise<void>((resolve) => {
    received = resolve;
  });
  await page.route("**/api/sessions/*/follow-up", async (route) => {
    received();
    await gate;
    await route.fulfill({
      status: failure ? 409 : 200,
      json: failure
        ? { success: false, message: "fixture rejected" }
        : { success: true, data: {} },
    });
  });
  return { release, started };
}

test("late success preserves newer text and reload persistence; failure preserves submitted text", async ({
  page,
}) => {
  const send = await deferredSend(page);
  await page.goto("/?mode=runtime-input");
  await expect(page.getByTestId("scratch-ready")).toHaveText("true");
  await page.getByLabel("Draft").fill("submitted");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await send.started;
  await page.getByLabel("Draft").fill("newer text");
  send.release();
  await expect(
    page.getByRole("button", { name: "Send", exact: true }),
  ).toBeEnabled();
  await expect(page.getByLabel("Draft")).toHaveValue("newer text");
  // Navigation before the debounce must flush to the OLD identity.
  await page.getByLabel("Identity").selectOption("b");
  await expect(page.getByLabel("Draft")).toHaveValue("");
  await page.getByLabel("Identity").selectOption("a");
  await expect(page.getByLabel("Draft")).toHaveValue("newer text");
  await page.reload();
  await expect(page.getByLabel("Draft")).toHaveValue("newer text");
  const failed = await deferredSend(page, true);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await failed.started;
  failed.release();
  await expect(page.getByTestId("send-error")).toContainText(
    "fixture rejected",
  );
  await expect(page.getByLabel("Draft")).toHaveValue("newer text");
});

test("late success cannot clear another Session or a changed Skill", async ({
  page,
}) => {
  const send = await deferredSend(page);
  await page.goto("/?mode=runtime-input");
  await page.getByLabel("Draft").fill("first");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await send.started;
  await page.getByLabel("Identity").selectOption("b");
  await page.getByLabel("Draft").fill("second");
  send.release();
  await expect(
    page.getByRole("button", { name: "Send", exact: true }),
  ).toBeEnabled();
  await expect(page.getByLabel("Draft")).toHaveValue("second");
  const next = await deferredSend(page);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await next.started;
  await page.getByLabel("Skill").fill("skill-b");
  next.release();
  await expect(
    page.getByRole("button", { name: "Send", exact: true }),
  ).toBeEnabled();
  await expect(page.getByLabel("Draft")).toHaveValue("second");
});

test("acknowledged new Session selects its identity and clears only its submission", async ({
  page,
}) => {
  await page.route("**/api/sessions", (route) =>
    route.fulfill({
      json: {
        success: true,
        data: { id: "created", workspace_id: "workspace-a" },
      },
    }),
  );
  const send = await deferredSend(page);
  await page.goto("/?mode=runtime-input");
  await page.getByLabel("Identity").selectOption("new");
  await page.getByLabel("Draft").fill("first request");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await send.started;
  send.release();
  await expect(page.getByLabel("Identity")).toHaveValue("created");
  await expect(page.getByLabel("Draft")).toHaveValue("");
  await page.getByLabel("Identity").selectOption("new");
  await expect(page.getByLabel("Draft")).toHaveValue("");
});

test("stream distinguishes loading, ready empty, degraded cache and switched identity", async ({
  page,
}) => {
  const sockets: WebSocketRoute[] = [];
  await page.routeWebSocket("**/api/fixture/*", (ws) => {
    sockets.push(ws);
  });
  await page.goto("/?mode=runtime-input&stream=1");
  await expect.poll(() => sockets.length).toBe(1);
  const result = page.getByTestId("stream");
  await expect(result).toContainText('"isInitialized":false');
  sockets[0].send(JSON.stringify({ Ready: true }));
  await expect(result).toContainText('"value":""');
  sockets[0].send(
    JSON.stringify({
      JsonPatch: [{ op: "replace", path: "/value", value: "cached" }],
    }),
  );
  await expect(result).toContainText("cached");
  sockets[0].close({ code: 1011, reason: "fixture failure" });
  await expect(result).toContainText("Connection lost");
  await expect(result).toContainText("cached");
  await page.getByRole("button", { name: "Switch stream" }).click();
  await expect.poll(() => sockets.length).toBe(2);
  await expect(result).not.toContainText("cached");
  await expect(result).not.toContainText("Connection lost");
  sockets[1].close({ code: 1011 });
  await expect(result).toContainText('"isInitialized":false');
  await expect(result).toContainText("Connection lost");
});
