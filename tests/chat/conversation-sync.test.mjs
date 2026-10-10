import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { chromium } from "@playwright/test";
import { fileURLToPath } from "node:url";

// Real React effects and browser layout, with isolated synthetic transports.
// No development server, shared database, or occupied port is used.
test("chat replay, scope cleanup, and scroll intent", async () => {
  const result = await build({
    entryPoints: [
      fileURLToPath(
        new URL("./conversation-sync.fixture.tsx", import.meta.url),
      ),
    ],
    bundle: true,
    write: false,
    format: "iife",
    jsx: "automatic",
    nodePaths: [
      fileURLToPath(
        new URL("../../packages/web-core/node_modules", import.meta.url),
      ),
    ],
    alias: {
      "@": fileURLToPath(
        new URL("../../packages/web-core/src", import.meta.url),
      ),
    },
    plugins: [
      {
        name: "isolated-chat-transports",
        setup(builder) {
          builder.onResolve(
            {
              filter:
                /localApiTransport$|useExecutionProcessesContext$|contexts\/EntriesContext$|streamJsonPatchEntries$|agentRunApi$/,
            },
            (args) => ({ path: args.path, namespace: "chat-mock" }),
          );
          builder.onLoad(
            { filter: /.*/, namespace: "chat-mock" },
            ({ path }) => ({
              contents: path.endsWith("localApiTransport")
                ? `export async function openLocalApiWebSocket(url) {
          const socket = { url, close() { this.closed = true; } };
          window.sockets.push(socket); return socket;
        }`
                : path.endsWith("useExecutionProcessesContext")
                  ? `export const useExecutionProcessesContext = () => window.processes;`
                  : path.endsWith("EntriesContext")
                    ? `export const useCanonicalAgentSession = () => window.canonical;`
                    : path.endsWith("agentRunApi")
                      ? `export const agentRunsApi = {listForSession: async () => window.runs};`
                      : `export const streamJsonPatchEntries = (url, callbacks) => { const controller = {url, callbacks, close() {this.closed = true}}; window.controllers.push(controller); return controller; };`,
            }),
          );
        },
      },
    ],
  });
  const browser = await chromium.launch({
    headless: true,
    args: ["--no-sandbox"],
  });
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.setContent('<div id="root"></div>');
    await page.addScriptTag({ content: result.outputFiles[0].text });
    await page.waitForFunction(() => window.sockets[0]?.onmessage);
    await page.evaluate(() => {
      window.event = (n) => ({
        schema_version: 1,
        payload_version: 1,
        event_id: `e-${n}`,
        session_id: "s",
        agent_run_id: "run-a",
        turn_id: "t",
        run_attempt_id: "a",
        run_attempt_number: 1,
        sequence: n,
        correlation_id: "c",
        timestamp: "2026-10-09T00:00:00Z",
        native_refs: [],
        payload: { type: "thinking", data: { content: `entry ${n}` } },
      });
      window.send = (type, data, index = 0) =>
        window.sockets[index].onmessage({
          data: JSON.stringify({ type, data }),
        });
      for (let n = 1; n <= 2000; n++)
        window.send("event", { event: window.event(n), replay: true });
    });
    await page.waitForTimeout(50);
    assert.equal(
      await page.evaluate(() => window.stream.timeline.events.length),
      0,
      "do not render partial replay",
    );
    await page.evaluate(() =>
      window.send("ready", {
        state: null,
        cursor: { run_attempt_number: 1, sequence: 2000 },
      }),
    );
    await page.waitForFunction(() => window.stream.isInitialized);
    assert.equal(
      await page.evaluate(() => window.stream.timeline.events.length),
      2000,
    );
    await page.evaluate(() => {
      window.updates = [];
      window.canonical = {
        ...window.canonical,
        conversation: {
          ...window.canonical.conversation,
          entries: [{ content: "new" }],
        },
      };
      window.render();
    });
    await page.waitForFunction(() => window.updates.length > 0);
    assert.deepEqual(
      await page.evaluate(() => window.updates.map((x) => x.type)),
      ["running"],
      "stream update must not reset history",
    );
    await page.evaluate(() => {
      window.scroll.scrollToBottom("auto");
      document
        .getElementById("scroller")
        .dispatchEvent(new WheelEvent("wheel", { deltaY: -500 }));
      document.getElementById("scroller").scrollTop = 1200;
      window.render();
    });
    await page.waitForTimeout(50);
    assert.equal(
      await page.evaluate(() => document.getElementById("scroller").scrollTop),
      1200,
      "stream growth must not override upward input",
    );
    await page.evaluate(() => {
      window.processes = {
        ...window.processes,
        executionProcessesVisible: [
          {
            id: "script-a",
            status: "running",
            run_reason: "setupscript",
            created_at: "2026-10-09T00:00:00Z",
            executor_action: { typ: { type: "ScriptRequest" } },
          },
        ],
      };
      window.render();
    });
    await page.waitForFunction(() => window.controllers.length === 1);
    await page.evaluate(() => {
      window.canonical = {
        ...window.canonical,
        conversation: { ...window.canonical.conversation },
      };
      window.render();
    });
    await page.waitForTimeout(50);
    assert.equal(
      await page.evaluate(() => window.controllers.length),
      1,
      "canonical updates must not reopen script streams",
    );
    await page.evaluate(() => {
      window.processes = { ...window.processes, executionProcessesVisible: [] };
      window.setRun("run-b");
    });
    await page.waitForFunction(() => window.sockets[1]?.onmessage);
    await page.evaluate(() => {
      window.send("event", { event: window.event(2001), replay: false }, 0);
      window.sockets[0].onclose();
    });
    await page.waitForTimeout(50);
    assert.equal(
      await page.evaluate(() => window.stream.timeline.events.length),
      0,
      "old socket must not mutate the new run",
    );
    assert.equal(await page.evaluate(() => window.sockets[0].closed), true);
    assert.equal(await page.evaluate(() => window.controllers[0].closed), true);
    await page.evaluate(() => {
      window.updates = [];
      window.controllers[0].callbacks.onEntries([
        { type: "STDOUT", content: "stale" },
      ]);
    });
    assert.equal(
      await page.evaluate(() => window.updates.length),
      0,
      "closed script callbacks cannot contaminate the next workspace",
    );
    await page.evaluate(() => {
      window.runs = ["old", "latest"].map((id, i) => ({
        agent_run_id: id,
        session_id: "s",
        created_at: `2026-10-09T00:00:0${i}Z`,
        state: {
          agent_run_id: id,
          status: "running",
          projection_status: "current",
          last_run_attempt_number: 1,
          last_event_sequence: 0,
          updated_at: "2026-10-09T00:00:00Z",
        },
      }));
      window.openSession();
    });
    await page.waitForFunction(
      () => window.sockets.filter((s) => /old|latest/.test(s.url)).length === 2,
    );
    await page.evaluate(() => {
      window.readyRun = (id) => {
        const socket = window.sockets.find((s) =>
          s.url.includes("/" + id + "/"),
        );
        const event = {
          ...window.event(1),
          agent_run_id: id,
          event_id: id,
          payload: {
            type: "message",
            data: {
              message: { message_id: id, role: "assistant", content: id },
              final_output: true,
            },
          },
        };
        socket.onmessage({
          data: JSON.stringify({
            type: "event",
            data: { event, replay: true },
          }),
        });
        socket.onmessage({
          data: JSON.stringify({
            type: "ready",
            data: {
              state: window.runs.find((r) => r.agent_run_id === id).state,
              cursor: { run_attempt_number: 1, sequence: 1 },
            },
          }),
        });
      };
      window.readyRun("old");
    });
    await page.waitForTimeout(50);
    assert.equal(
      await page.evaluate(() => window.session.conversation.entries.length),
      0,
      "old run must wait for latest run replay",
    );
    await page.evaluate(() => window.readyRun("latest"));
    await page.waitForFunction(() => !window.session.isLoading);
    assert.ok(
      await page.evaluate(() =>
        window.session.conversation.entries.some(
          (e) => e.content?.content === "latest",
        ),
      ),
      "latest run is visible together with history",
    );
    assert.deepEqual(errors, []);
  } finally {
    await browser.close();
  }
});
