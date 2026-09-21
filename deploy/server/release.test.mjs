import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { parse } from "yaml";

const root = new URL("../../", import.meta.url);
test("release builds the server target, tests before publishing, and does not publish main", async () => {
  const workflow = parse(
    await readFile(
      new URL(".github/workflows/publish-server.yml", root),
      "utf8",
    ),
  );
  assert.deepEqual(workflow.on.push, { tags: ["v*"] });
  assert("workflow_dispatch" in workflow.on);
  const job = workflow.jobs["server-image"];
  assert.equal(job.permissions.packages, "write");
  assert.equal(workflow.permissions.contents, "read");
  const steps = job.steps;
  const build = steps.findIndex((step) =>
    step.uses?.startsWith("docker/build-push-action@"),
  );
  const smoke = steps.findIndex((step) => step.run?.includes("smoke-image.sh"));
  const publish = steps.findIndex((step) => step.run?.includes("docker push"));
  assert(build < smoke && smoke < publish);
  assert.equal(steps[build].with.target, "server");
  assert.equal(steps[build].with.push, false);
  assert.equal(steps[build].with.load, true);
  assert.match(steps[publish].if, /github.event_name == 'push'/);
  assert.match(steps[publish].if, /github.ref_type == 'tag'/);
  assert(!JSON.stringify(workflow).includes(":latest"));
  for (const step of steps) {
    if (step.run)
      assert.equal(
        spawnSync("bash", ["-n"], { input: step.run }).status,
        0,
        step.name,
      );
  }
});

test("shell examples and CI smoke script parse", async () => {
  const script = await readFile(
    new URL("deploy/server/smoke-image.sh", root),
    "utf8",
  );
  assert.equal(spawnSync("bash", ["-n"], { input: script }).status, 0);
  const docs = await readFile(
    new URL("docs/self-hosting/server-container.mdx", root),
    "utf8",
  );
  for (const match of docs.matchAll(/```bash\n([\s\S]*?)```/g)) {
    const checked = spawnSync("bash", ["-n"], {
      input: match[1],
      encoding: "utf8",
    });
    assert.equal(checked.status, 0, checked.stderr);
  }
});

test("Docker distribution includes the prebuilt app, process host, pinned tools, and a non-root entrypoint", async () => {
  const docker = await readFile(new URL("Dockerfile", root), "utf8");
  assert.match(docker, /--bin server --bin agent-process-host/);
  assert.match(
    docker,
    /COPY --from=builder \/usr\/local\/bin\/agent-process-host/,
  );
  assert.match(docker, /FROM runtime AS server/);
  assert.match(docker, /ARG CODEX_VERSION=\d+\.\d+\.\d+/);
  assert.match(docker, /rust-toolchain\.toml/);
  assert.match(
    docker,
    /COPY --from=builder \/usr\/local\/cargo\/bin \/usr\/local\/bin/,
  );
  assert.match(docker, /USER appuser[\s\S]*ENTRYPOINT.*server\.mjs/);
  const entrypoints = docker.match(/^ENTRYPOINT .*$/gm);
  assert.match(entrypoints.at(-1), /tini.*server\.mjs/);
  const ignore = await readFile(new URL(".dockerignore", root), "utf8");
  for (const entry of [
    "**/secrets/",
    "**/.codex/",
    "**/.ssh/",
    "**/.evk-shared/",
  ])
    assert(ignore.includes(entry));
});
