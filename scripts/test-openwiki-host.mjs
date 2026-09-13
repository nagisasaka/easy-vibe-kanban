// Public CLI/MCP compatibility smoke. No models, API keys or upstream imports.
// Run: OPENWIKI_BIN=/path/to/openwiki node scripts/test-openwiki-host.mjs
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";

const pin = (
  await readFile(new URL("../assets/openwiki-version", import.meta.url), "utf8")
).trim();
const binary = process.env.OPENWIKI_BIN ?? "openwiki";
const env = { ...process.env, OPENWIKI_TELEMETRY_DISABLED: "1" };
const help = spawnSync(binary, ["--help"], { encoding: "utf8", env });
assert.equal(help.status, 0, "OpenWiki CLI must be installed");
assert.equal(
  help.stdout.match(/OpenWiki v(\S+)/)?.[1],
  pin,
  "OpenWiki version pin",
);
const root = await mkdtemp(path.join(tmpdir(), "evk-openwiki-host-"));
function git(...args) {
  const output = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(output.status, 0, output.stderr);
  return output.stdout.trim();
}
git("init", "-b", "main");
git("config", "user.email", "openwiki-fixture@example.invalid");
git("config", "user.name", "OpenWiki fixture");
await writeFile(
  path.join(root, "README.md"),
  "# Counter fixture\n\nThis project exports an add function for adding two numbers.\n",
);
await writeFile(
  path.join(root, "index.js"),
  "export function add(a, b) { return a + b; }\n",
);
git("add", ".");
git("commit", "-m", "fixture source");
const head = git("rev-parse", "HEAD");
const install = () =>
  spawnSync(binary, ["integrations", "install", "codex", "--project", root], {
    encoding: "utf8",
    env,
  });
assert.equal(install().status, 0, "Project-scoped public integration install");
assert.equal(install().status, 0, "Repeated integration install is idempotent");
await mkdir(path.join(root, "openwiki"), { recursive: true });
const instructions =
  "# User instructions\nPreserve evidence and write concise fixture documentation.\n";
await writeFile(path.join(root, "openwiki/INSTRUCTIONS.md"), instructions);
const child = spawn(binary, ["mcp", "--host", "codex"], {
  cwd: root,
  env,
  stdio: ["pipe", "pipe", "inherit"],
});
let sequence = 0;
const pending = new Map();
const lines = createInterface({ input: child.stdout });
lines.on("line", (line) => {
  let frame;
  try {
    frame = JSON.parse(line);
  } catch {
    return;
  }
  const waiter = pending.get(frame.id);
  if (!waiter) return;
  pending.delete(frame.id);
  if (frame.error) waiter.reject(new Error(JSON.stringify(frame.error)));
  else waiter.resolve(frame.result);
});
function call(method, params) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Timeout: ${method}`));
    }, 30000);
    pending.set(id, {
      resolve: (value) => {
        clearTimeout(timeout);
        resolve(value);
      },
      reject: (error) => {
        clearTimeout(timeout);
        reject(error);
      },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`,
    );
  });
}
function data(result) {
  if (result.isError) throw new Error(JSON.stringify(result));
  return (
    result.structuredContent ??
    JSON.parse(result.content.find((block) => block.type === "text").text)
  );
}
const tool = async (name, args) =>
  data(await call("tools/call", { name, arguments: args }));
let succeeded = false;
try {
  await call("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "evk-openwiki-smoke", version: "1" },
  });
  child.stdin.write(
    `${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`,
  );
  const tools = await call("tools/list", {});
  assert.ok(tools.tools.some((entry) => entry.name === "openwiki_finish"));
  const begin = await tool("openwiki_begin", {
    root,
    mode: "init",
    language: "en",
  });
  assert.equal(begin.status, "active");
  assert.equal(typeof begin.runId, "string");
  const runId = begin.runId;
  const premature = await call("tools/call", {
    name: "openwiki_finish",
    arguments: { runId },
  });
  assert.equal(
    premature.isError,
    true,
    "An incomplete run must not be acknowledged",
  );
  await tool("openwiki_submit_plan", {
    runId,
    pages: [
      {
        path: "/openwiki/quickstart.md",
        title: "Quickstart",
        purpose: "Explain the fixture add function",
        seedPaths: ["index.js", "README.md"],
      },
    ],
  });
  const job = await tool("openwiki_next_page", { runId });
  assert.equal(job.status, "pending");
  await writeFile(
    path.join(root, "openwiki/quickstart.md"),
    "---\ntype: concept\ntitle: Quickstart\ndescription: The fixture add function.\ntags: [fixture]\n---\n\n# Quickstart\n\nThe add function returns the sum of a and b.\n",
  );
  await tool("openwiki_submit_page", {
    runId,
    jobId: job.job.id,
    claims: [
      {
        statement: "The add function returns the sum of a and b.",
        evidence: [{ resource: "repo://index.js#L1" }],
      },
    ],
  });
  assert.equal(
    (await tool("openwiki_next_page", { runId })).status,
    "complete",
  );
  const finished = await tool("openwiki_finish", { runId });
  assert.equal(finished.status, "complete");
  assert.notEqual(finished.sourceChanged, true);
  assert.equal(
    await readFile(path.join(root, "openwiki/INSTRUCTIONS.md"), "utf8"),
    instructions,
  );
  assert.equal(
    git("rev-parse", "HEAD"),
    head,
    "OpenWiki must not commit source",
  );
  assert.equal(
    await readFile(path.join(root, "index.js"), "utf8"),
    "export function add(a, b) { return a + b; }\n",
  );
  const changedPaths = [
    git("diff", "--name-only", "HEAD"),
    git("ls-files", "--others", "--exclude-standard"),
  ]
    .join("\n")
    .split("\n")
    .filter(Boolean);
  for (const file of changedPaths) {
    assert.ok(
      file.startsWith("openwiki/") ||
        file.startsWith(".agents/skills/openwiki/") ||
        [
          "AGENTS.md",
          "CLAUDE.md",
          ".codex/config.toml",
          ".github/workflows/openwiki-update.yml",
        ].includes(file),
      `Unexpected upstream setup/source mutation: ${file}`,
    );
  }
  git("add", "openwiki", "AGENTS.md", "CLAUDE.md");
  git("commit", "-m", "fixture wiki publication");
  const update = await tool("openwiki_begin", {
    root,
    mode: "update",
    language: "en",
  });
  if (update.status !== "noop") {
    // Metadata/setup drift may require a supported empty update plan. This
    // still exercises deterministic no-change finalisation without a model.
    await tool("openwiki_submit_plan", { runId: update.runId, pages: [] });
    assert.equal(
      (await tool("openwiki_next_page", { runId: update.runId })).status,
      "complete",
    );
    assert.equal(
      (await tool("openwiki_finish", { runId: update.runId })).status,
      "complete",
    );
  }
  assert.equal(
    await readFile(path.join(root, "openwiki/INSTRUCTIONS.md"), "utf8"),
    instructions,
  );
  console.log(
    `PASS: OpenWiki ${pin}; install/reinstall, host MCP, init, rejected premature finish, plan/page/claims/finalisation, user instructions and source preservation. No model call.`,
  );
  succeeded = true;
} finally {
  lines.close();
  child.kill("SIGTERM");
  await new Promise((resolve) => {
    if (child.exitCode !== null) resolve();
    else child.once("exit", resolve);
  });
  if (succeeded) await rm(root, { recursive: true });
  else console.error(`Failed fixture preserved at ${root}`);
}
