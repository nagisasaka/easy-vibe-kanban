import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { parse } from "yaml";

test("ACME validation and atomic certificate export", () => {
  const result = spawnSync(
    "python3",
    [fileURLToPath(new URL("./acme.test.py", import.meta.url))],
    {
      encoding: "utf8",
      env: { ...process.env, PYTHONDONTWRITEBYTECODE: "1" },
    },
  );
  assert.equal(
    result.status,
    0,
    result.error?.message || result.stdout + result.stderr,
  );
});

test("ACME companion only publishes validation and shares no agent or account credentials", async () => {
  const { services } = parse(
    await readFile(new URL("./compose.acme.yaml", import.meta.url), "utf8"),
  );
  const acme = services.certificates;
  assert.deepEqual(acme.ports, ["80:80"]);
  assert.equal(
    services.evk.depends_on.certificates.condition,
    "service_healthy",
  );
  assert(acme.volumes.includes("acme:/etc/letsencrypt"));
  assert(!JSON.stringify(acme).includes("docker.sock"));
  assert(!JSON.stringify(acme.volumes).includes("htpasswd"));
  assert(!JSON.stringify(acme.volumes).includes("/home/appuser"));
});
