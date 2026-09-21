import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { test } from "node:test";
import { parse } from "yaml";
import {
  backendEnvironment,
  renderNginx,
  serverSettings,
  supervise,
  validateSecrets,
} from "./server.mjs";

const settings = serverSettings({
  EVK_DOMAIN: "evk.example.test",
  EVK_PREVIEW_DOMAIN: "preview.example.test",
});
// A test-only hash, never a production/default credential.
const hash = "test:$2y$05$" + "a".repeat(53);

test("configuration rejects missing/unsafe hosts and same-origin preview collisions", () => {
  for (const value of [
    "",
    "https://example.test",
    "example.test:443",
    "example.test/path",
    "x; auth_basic off;",
    "*.example.test",
    "UPPER.test",
    "a..test",
    "-a.test",
    "a".repeat(64) + ".test",
  ]) {
    assert.throws(() => serverSettings({ EVK_DOMAIN: value }));
  }
  assert.throws(() =>
    serverSettings({
      EVK_DOMAIN: "5173.preview.example.test",
      EVK_PREVIEW_DOMAIN: "preview.example.test",
    }),
  );
  assert.throws(() =>
    serverSettings({
      EVK_DOMAIN: "preview.example.test",
      EVK_PREVIEW_DOMAIN: "preview.example.test",
    }),
  );
  assert.deepEqual(serverSettings({ EVK_DOMAIN: "evk.example.test" }), {
    app: "evk.example.test",
    preview: null,
  });
});

test("all proxy routes inherit Basic auth and strip credentials; previews stay separate", async () => {
  const template = await readFile(
    new URL("./nginx.conf.template", import.meta.url),
    "utf8",
  );
  const config = renderNginx(template, settings);
  assert(!config.includes("@@"));
  assert.match(config, /auth_basic_user_file \/run\/evk-secrets\/htpasswd/);
  assert(!config.includes("auth_basic off"));
  assert.match(config, /proxy_set_header Authorization ""/);
  assert.match(config, /proxy_set_header X-VK-Relayed ""/);
  assert.match(config, /proxy_buffering off/);
  assert.match(config, /proxy_set_header Upgrade \$http_upgrade/);
  assert.match(config, /preview\(\?:\/\|\$\).*return 403/);
  assert.match(config, /server_name evk.example.test/);
  const pattern = config
    .match(/server_name "~\^(.*?)\$";/)[1]
    .replace("(?<preview_target>", "(");
  const hostPattern = new RegExp(`^${pattern}$`);
  for (let port = 0; port <= 65536; port++) {
    assert.equal(
      hostPattern.test(`${port}.preview.example.test`),
      port >= 1024 && port <= 65535,
      String(port),
    );
  }
  for (const host of [
    "1023.preview.example.test",
    "65536.preview.example.test",
    "0.preview.example.test",
    "5173.previewXexample.test",
    "5173.other.test",
  ]) {
    assert(!hostPattern.test(host), host);
  }
  assert(
    !renderNginx(template, { app: settings.app, preview: null }).includes(
      "preview_target",
    ),
  );
});

test("backend stays on loopback with only the app origin allowed", () => {
  const env = backendEnvironment(
    { HOST: "0.0.0.0", PORT: "80", VK_ALLOWED_ORIGINS: "*" },
    settings,
  );
  assert.equal(env.HOST, "127.0.0.1");
  assert.equal(env.PORT, "");
  assert.equal(env.BACKEND_PORT, "3000");
  assert.equal(env.PREVIEW_PROXY_PORT, "3001");
  assert.equal(env.VK_ALLOWED_ORIGINS, "https://evk.example.test");
  assert.equal(env.VK_PREVIEW_DOMAIN, "preview.example.test");
});

test("certificate and password preflight fails closed", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "evk-secrets-test-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const certPath = join(dir, "cert.pem"),
    keyPath = join(dir, "key.pem");
  execFileSync(
    "openssl",
    [
      "req",
      "-x509",
      "-newkey",
      "rsa:2048",
      "-nodes",
      "-days",
      "1",
      "-subj",
      "/CN=evk.example.test",
      "-addext",
      "subjectAltName=DNS:evk.example.test,DNS:*.preview.example.test",
      "-keyout",
      keyPath,
      "-out",
      certPath,
    ],
    { stdio: "ignore" },
  );
  const cert = await readFile(certPath),
    key = await readFile(keyPath);
  validateSecrets(cert, key, hash, settings);
  assert.throws(
    () => validateSecrets(cert, key, hash, settings, 0),
    /not currently valid/,
  );
  assert.throws(
    () => validateSecrets(cert, key, hash, settings, Date.now() + 2 * 86400000),
    /not currently valid/,
  );
  assert.throws(
    () => validateSecrets(cert, key, hash, { app: "other.test" }),
    /does not cover/,
  );
  assert.throws(
    () =>
      validateSecrets(cert, key, hash, {
        app: settings.app,
        preview: "other.test",
      }),
    /does not cover/,
  );
  for (const passwords of [
    "",
    "admin:plaintext",
    "admin:$apr1$bad",
    hash + "\ninvalid",
  ]) {
    assert.throws(
      () => validateSecrets(cert, key, passwords, settings),
      /bcrypt/,
    );
  }
  execFileSync(
    "openssl",
    [
      "genpkey",
      "-algorithm",
      "RSA",
      "-pkeyopt",
      "rsa_keygen_bits:2048",
      "-out",
      join(dir, "other.pem"),
    ],
    { stdio: "ignore" },
  );
  assert.throws(
    () =>
      validateSecrets(
        cert,
        execFileSync("openssl", ["pkey", "-in", join(dir, "other.pem")]),
        hash,
        settings,
      ),
    /mismatch/,
  );
});

test("service failure terminates sibling process groups, then forces stuck processes down", async () => {
  const children = [],
    signals = [];
  const result = supervise(
    [["backend"], ["nginx"]],
    {},
    {
      shutdownMs: 10,
      spawnProcess() {
        const child = new EventEmitter();
        child.pid = 100 + children.length;
        children.push(child);
        return child;
      },
      signalProcess(pid, signal) {
        signals.push([pid, signal]);
        if (signal === "SIGKILL")
          queueMicrotask(() => children[-pid - 100].emit("exit", null, signal));
      },
    },
  );
  children[0].emit("exit", 1);
  assert.equal(await result, 1);
  assert.deepEqual(signals, [
    [-101, "SIGTERM"],
    [-101, "SIGKILL"],
  ]);
});

test("a spawn failure stops other services", async () => {
  const children = [];
  const result = supervise(
    [["backend"], ["nginx"]],
    {},
    {
      spawnProcess() {
        const child = new EventEmitter();
        child.pid = 100 + children.length;
        children.push(child);
        return child;
      },
      signalProcess(pid) {
        queueMicrotask(() => children[-pid - 100].emit("exit", 0));
      },
    },
  );
  children[0].emit("error", new Error("missing executable"));
  assert.equal(await result, 1);
  assert.equal(process.listenerCount("SIGTERM"), 0);
});

test("external shutdown is clean and removes signal handlers", async () => {
  const child = new EventEmitter();
  child.pid = 100;
  const result = supervise(
    [["backend"]],
    {},
    {
      spawnProcess: () => child,
      signalProcess: () => queueMicrotask(() => child.emit("exit", 0)),
    },
  );
  process.emit("SIGTERM");
  assert.equal(await result, 0);
  assert.equal(process.listenerCount("SIGTERM"), 0);
  await assert.rejects(supervise([], {}), /No services/);
});

test("a synchronous spawn exception also shuts down the already started service", async () => {
  const child = new EventEmitter();
  child.pid = 100;
  let calls = 0;
  const result = supervise(
    [["backend"], ["nginx"]],
    {},
    {
      spawnProcess() {
        if (calls++) throw new Error("simulated spawn failure");
        return child;
      },
      signalProcess(pid, signal) {
        assert.equal(pid, -100);
        assert.equal(signal, "SIGTERM");
        queueMicrotask(() => child.emit("exit", 0));
      },
    },
  );
  assert.equal(await result, 1);
  assert.equal(process.listenerCount("SIGTERM"), 0);
});

test("compose publishes HTTPS only and persists every runtime storage root", async () => {
  const compose = parse(
    await readFile(new URL("./compose.yaml", import.meta.url), "utf8"),
  );
  const service = compose.services.evk;
  assert.deepEqual(service.ports, ["443:8443"]);
  assert.equal(service.build, undefined);
  assert.equal(service.privileged, undefined);
  assert.equal(service.network_mode, undefined);
  assert(service.volumes.includes("home:/home/appuser"));
  assert(service.volumes.includes("repos:/repos"));
  assert(service.volumes.includes("work:/var/tmp"));
  const secrets = service.volumes.find((v) => v.target === "/run/evk-secrets");
  assert.equal(secrets.read_only, true);
  assert.equal(secrets.bind.create_host_path, false);
  assert.equal(service.restart, "unless-stopped");
});
