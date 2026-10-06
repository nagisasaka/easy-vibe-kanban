import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const command = process.env.LVK_TEST_COMPOSE || "docker";
const prefix = process.env.LVK_TEST_COMPOSE ? [] : ["compose"];
const available =
  spawnSync(command, [...prefix, "version"], { stdio: "ignore" }).status === 0;

test(
  "Compose resolves LVK storage and settings",
  { skip: !available && "Docker Compose is unavailable" },
  async () => {
    const temporary = await mkdtemp(join(tmpdir(), "lvk-compose-test-"));
    try {
      const envFile = join(temporary, ".env");
      await writeFile(envFile, "");
      const env = Object.fromEntries(
        Object.entries(process.env).filter(
          ([key]) => !/^(LVK_|COMPOSE_)/.test(key),
        ),
      );
      Object.assign(env, {
        LVK_IMAGE: "ghcr.io/example/lvk:test",
        LVK_HOST: "192.0.2.1",
        LVK_ACME_AGREE_TOS: "yes",
      });
      const files = [
        "compose.yaml",
        "compose.acme.yaml",
        "compose.development.yaml",
      ];
      const config = JSON.parse(
        execFileSync(
          command,
          [
            ...prefix,
            "--env-file",
            envFile,
            ...files.flatMap((file) => ["-f", file]),
            "--profile",
            "development",
            "config",
            "--format",
            "json",
          ],
          {
            cwd: fileURLToPath(new URL("./", import.meta.url)),
            env,
            encoding: "utf8",
          },
        ),
      );
      assert.equal(config.name, "lvk-server");
      assert.equal(config.services.lvk.image, "ghcr.io/example/lvk:test");
      for (const service of ["lvk", "development", "certificates"])
        assert.equal(
          config.services[service].environment.LVK_HOST,
          "192.0.2.1",
        );
      assert.equal(
        config.services.certificates.environment.LVK_ACME_AGREE_TOS,
        "yes",
      );
      assert.equal(Object.keys(config.volumes).length, 6);
      for (const [key, volume] of Object.entries(config.volumes)) {
        assert.equal(volume.name, `lvk-server_${key}`);
        assert.equal(Boolean(volume.external), false);
      }
    } finally {
      await rm(temporary, { recursive: true, force: true });
    }
  },
);
