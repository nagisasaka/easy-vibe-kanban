import { mkdirSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";

// Opt in: this starts real, authenticated Codex turns on the selected server.
test("real Codex steering, editable FIFO queue, cancellation and resume", async ({
  page,
  request,
}) => {
  test.skip(
    process.env.LVK_E2E_REAL_CODEX !== "1",
    "Requires an authenticated development Codex",
  );
  test.setTimeout(600_000);
  const terminal = new Set([
    "succeeded",
    "failed",
    "cancelled",
    "crashed",
    "audit_failed",
  ]);
  const api = async (
    path: string,
    data?: unknown,
    method = data === undefined ? "GET" : "POST",
  ) => {
    if (path.endsWith("/cancel"))
      data = {
        command_id: randomUUID(),
        idempotency_key: randomUUID(),
        correlation_id: randomUUID(),
        created_at: new Date().toISOString(),
        ...(data as object),
      };
    const response = await request.fetch(path, { method, data });
    expect(response.ok(), `${path}: ${await response.text()}`).toBeTruthy();
    const body = await response.json();
    expect(response.ok(), JSON.stringify(body)).toBeTruthy();
    expect(body.success, JSON.stringify(body)).toBeTruthy();
    return body.data;
  };
  let workspaceId: string | undefined;
  let sessionId: string | undefined;
  const runs = () => api(`/api/agent-runs/session/${sessionId}`);
  const config = { executor: "CODEX", execution_mode: "code" };
  try {
    await page.goto("/workspaces/create");
    await expect(page.getByRole("button").first()).toBeVisible({
      timeout: 120_000,
    });
    await page.keyboard.press("Escape");
    const directory = `/repos/.lvk-queue-e2e-${randomUUID()}`;
    mkdirSync(directory);
    const created = await api("/api/workspaces/start", {
      mode: "direct_folder",
      name: "Real Codex queue E2E",
      repos: [],
      directory_path: directory,
      linked_issue: null,
      executor_config: config,
      attachment_ids: [],
      prompt:
        "Bounded integration test. Do not read or change files, use subagents, or access credentials. Run a shell command that sleeps for 45 seconds. Then reply INITIAL_DONE. If a steering message arrives, include its supplied token in your final reply. Do not start any additional task.",
    });
    workspaceId = created.workspace.id;
    console.log("Created real Codex queue test", workspaceId);
    sessionId = created.agent_run.state.session_id;
    const firstRun = created.agent_run.agent_run_id;
    await page.goto(`/workspaces/${workspaceId}`);
    await page.keyboard.press("Escape");
    const editor = page
      .locator('[contenteditable="true"][role="textbox"]')
      .first();
    const queueButton = page.getByRole("button", {
      name: /^(Queue|キューに追加)( unavailable:.*)?$/,
    });
    await expect(queueButton).toBeVisible({ timeout: 120_000 });
    const append = async (text: string) => {
      await editor.fill(text);
      await expect(queueButton).toBeEnabled();
      const response = page.waitForResponse(
        (r) =>
          r.request().method() === "POST" &&
          new URL(r.url()).pathname === `/api/sessions/${sessionId}/queue`,
      );
      await queueButton.click();
      expect((await response).ok()).toBeTruthy();
      await expect(editor).toHaveText("");
    };
    console.log("Queue controls ready");
    await append("Reply exactly FIFOA. Do not use tools.");
    await append("Reply exactly FIFOB. Do not use tools.");
    await append("Reply exactly DELETEME. Do not use tools.");
    const queue = page.getByRole("region", { name: "Follow-up queue" });
    await expect(queue.locator("li")).toHaveCount(3);
    await queue
      .locator("li")
      .nth(2)
      .getByRole("button", { name: /^(Delete|削除)$/ })
      .click();
    await expect(queue.locator("li")).toHaveCount(2);
    await queue
      .getByRole("button", {
        name: /^(Move queued message 2 up|予約 2 を上へ)$/,
      })
      .click();
    await expect(queue.locator("li").first()).toContainText("FIFOB");
    await queue
      .locator("li")
      .first()
      .getByRole("button", { name: /^(Edit|編集)$/ })
      .click();
    await queue
      .getByRole("textbox", { name: /^(Queued message|予約内容)$/ })
      .fill("Reply exactly FIFOBEDITED. Do not use tools.");
    await queue.getByRole("button", { name: /^(Save|保存)$/ }).click();
    await expect(queue.locator("li").first()).toContainText("FIFOBEDITED");
    await page.screenshot({ path: test.info().outputPath("queue.png") });
    console.log("Queue edits and ordering verified");
    // The new composer draft remains independent of the submitted queue.
    const token = `STEER${randomUUID()}`;
    await editor.fill(
      `Include ${token} in your final reply for the current task.`,
    );
    const steerResponse = page.waitForResponse(
      (r) =>
        r.request().method() === "POST" &&
        new URL(r.url()).pathname.endsWith("/steer"),
    );
    await page.getByRole("button", { name: "Steer", exact: true }).click();
    expect((await steerResponse).ok()).toBeTruthy();
    console.log("Steering accepted");
    await expect
      .poll(async () => (await api(`/api/agent-runs/${firstRun}`)).status, {
        timeout: 180_000,
      })
      .toBe("succeeded");
    await expect
      .poll(
        async () => {
          const list = await runs();
          return (
            list.length === 3 &&
            list.every(
              (r: { state: { status: string } }) =>
                r.state.status === "succeeded",
            )
          );
        },
        { timeout: 180_000 },
      )
      .toBe(true);
    const completed = (await runs()).sort(
      (a: { created_at: string }, b: { created_at: string }) =>
        a.created_at.localeCompare(b.created_at),
    );
    expect(JSON.stringify(completed[0].state.terminal_output)).toContain(token);
    expect(JSON.stringify(completed[1].state.terminal_output)).toContain(
      "FIFOBEDITED",
    );
    expect(JSON.stringify(completed[2].state.terminal_output)).toContain(
      "FIFOA",
    );
    expect((await api(`/api/sessions/${sessionId}/queue`)).status).toBe(
      "empty",
    );

    console.log("Sequential Codex outputs verified");
    // Cancel a real turn with two pending follow-ups; neither may be discarded.
    const longRun = await api(`/api/sessions/${sessionId}/follow-up`, {
      prompt:
        "Run a shell command that sleeps 90 seconds, then reply WAIT_DONE. Do nothing else.",
      executor_config: config,
    });
    await expect(queueButton).toBeVisible({ timeout: 60_000 });
    await append("Reply exactly RESUMEDONE. Do not use tools.");
    await append("Reply exactly RESUMEDTWO. Do not use tools.");
    await api(`/api/agent-runs/${longRun.agent_run_id}/cancel`, {
      reason: "Queue cancellation E2E",
    });
    console.log("Cancellation acknowledged");
    await expect
      .poll(async () => (await api(`/api/sessions/${sessionId}/queue`)).paused)
      .toBe(true);
    expect(
      (await api(`/api/sessions/${sessionId}/queue`)).messages,
    ).toHaveLength(2);
    await expect(
      queue.getByRole("button", { name: /^(Resume queue|予約を再開)$/ }),
    ).toBeVisible();
    await queue
      .getByRole("button", { name: /^(Resume queue|予約を再開)$/ })
      .click();
    await expect
      .poll(
        async () => {
          const list = await runs();
          return (
            list.length === 6 &&
            list.every((r: { state: { status: string } }) =>
              terminal.has(r.state.status),
            )
          );
        },
        { timeout: 180_000 },
      )
      .toBe(true);
    const final = (await runs()).sort(
      (a: { created_at: string }, b: { created_at: string }) =>
        a.created_at.localeCompare(b.created_at),
    );
    expect(final[3].state.status).toBe("cancelled");
    expect(final[4].state.status).toBe("succeeded");
    expect(final[5].state.status).toBe("succeeded");
    expect(JSON.stringify(final[4].state.terminal_output)).toContain(
      "RESUMEDONE",
    );
    expect(JSON.stringify(final[5].state.terminal_output)).toContain(
      "RESUMEDTWO",
    );
    console.log("Cancellation and resume verified");
  } catch (error) {
    console.error("E2E primary failure", error);
    throw error;
  } finally {
    if (sessionId) {
      await api(`/api/sessions/${sessionId}/queue`, undefined, "DELETE");
      for (const run of await runs()) {
        if (!terminal.has(run.state.status))
          await api(`/api/agent-runs/${run.agent_run_id}/cancel`, {
            reason: "E2E cleanup",
          });
      }
    }
    if (workspaceId)
      await api(`/api/workspaces/${workspaceId}`, { archived: true }, "PUT");
    console.log("E2E cleanup finished");
  }
});
