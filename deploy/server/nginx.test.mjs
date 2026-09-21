// Native nginx integration: no Docker, no real EVK database or agent calls.
import assert from "node:assert/strict";
import { spawn, spawnSync, execFileSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:http";
import https from "node:https";
import { mkdtemp, readFile, writeFile, mkdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";
import { renderNginx, serverSettings } from "./server.mjs";

const nginx = process.env.EVK_TEST_NGINX || "nginx";
const available = spawnSync(nginx, ["-v"]).status === 0;
if (!available && process.env.EVK_REQUIRE_NGINX_TEST === "1") {
  throw new Error("nginx is required for this CI gate");
}

test(
  "HTTPS ingress protects HTTP/WS/SSE and isolates previews",
  {
    skip: !available && "nginx not installed (set EVK_TEST_NGINX to a binary)",
    timeout: 20000,
  },
  async (t) => {
    const dir = await mkdtemp(join(tmpdir(), "evk-nginx-integration-"));
    t.after(() => rm(dir, { recursive: true, force: true }));
    const sockets = new Set();
    let streamEnded = false;
    const backend = createServer((req, res) => {
      if (req.url === "/stream") {
        res.writeHead(200, { "Content-Type": "text/event-stream" });
        res.write("data: first\n\n");
        const timer = setTimeout(() => {
          streamEnded = true;
          res.end("data: done\n\n");
        }, 500);
        res.on("close", () => clearTimeout(timer));
      } else {
        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify({ path: req.url, headers: req.headers }));
      }
    });
    backend.on("connection", (socket) => {
      sockets.add(socket);
      socket.on("close", () => sockets.delete(socket));
    });
    backend.on("upgrade", (req, socket) => {
      assert.equal(req.headers.authorization, undefined);
      assert.equal(req.headers["sec-websocket-protocol"], "vite-hmr");
      socket.write(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Protocol: vite-hmr\r\n\r\n",
      );
    });
    backend.listen(0, "127.0.0.1");
    await once(backend, "listening");
    t.after(() => {
      for (const socket of sockets) socket.destroy();
      backend.close();
    });
    const backendPort = backend.address().port;
    const reservation = createServer();
    reservation.listen(0, "127.0.0.1");
    await once(reservation, "listening");
    const port = reservation.address().port;
    await new Promise((resolve) => reservation.close(resolve));
    for (const name of ["client", "proxy", "fastcgi", "uwsgi", "scgi"])
      await mkdir(join(dir, name));
    const certPath = join(dir, "fullchain.pem"),
      keyPath = join(dir, "privkey.pem");
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
    // Password evk-test-only; this fixture is never used by the distribution.
    await writeFile(
      join(dir, "htpasswd"),
      "test:$2b$12$1ea3Q20isUydEeMVrwel0ePEoaznkgIHV3kX8vsbMCjuqzQvsTUki\n",
    );
    await writeFile(join(dir, "mime.types"), "types { text/html html; }\n");
    const settings = serverSettings({
      EVK_DOMAIN: "evk.example.test",
      EVK_PREVIEW_DOMAIN: "preview.example.test",
    });
    const template = await readFile(
      new URL("./nginx.conf.template", import.meta.url),
      "utf8",
    );
    const config = renderNginx(template, settings)
      // Node child pipes are Unix sockets, unlike Docker's stdout pipe; nginx
      // cannot reopen them as /dev/stdout. Keep this test's access log in its tempdir.
      .replace("/dev/stdout", join(dir, "access.log"))
      .replaceAll("/tmp/evk-server", dir)
      .replace("/etc/nginx/mime.types", join(dir, "mime.types"))
      .replace("/run/evk-secrets/tls/fullchain.pem", certPath)
      .replace("/run/evk-secrets/tls/privkey.pem", keyPath)
      .replace("/run/evk-secrets/htpasswd", join(dir, "htpasswd"))
      .replaceAll("listen 8443", `listen ${port}`)
      .replaceAll("127.0.0.1:3000", `127.0.0.1:${backendPort}`)
      .replaceAll("127.0.0.1:3001", `127.0.0.1:${backendPort}`);
    const configPath = join(dir, "nginx.conf");
    await writeFile(configPath, config);
    const checked = spawnSync(
      nginx,
      ["-e", "stderr", "-t", "-c", configPath, "-p", dir],
      { encoding: "utf8" },
    );
    assert.equal(checked.status, 0, checked.stderr);
    const child = spawn(
      nginx,
      ["-e", "stderr", "-c", configPath, "-p", dir, "-g", "daemon off;"],
      { stdio: "pipe" },
    );
    let errors = "";
    child.stderr.on("data", (data) => {
      errors += data;
    });
    child.stdout.resume();
    t.after(async () => {
      if (child.exitCode === null) {
        const exited = once(child, "exit");
        child.kill("SIGTERM");
        const timer = setTimeout(() => child.kill("SIGKILL"), 1000);
        await exited;
        clearTimeout(timer);
      }
    });
    const ca = await readFile(certPath);
    const authorization = `Basic ${Buffer.from("test:evk-test-only").toString("base64")}`;
    function request(path = "/", host = settings.app, headers = {}) {
      return new Promise((resolve, reject) => {
        const req = https.get(
          {
            hostname: "127.0.0.1",
            port,
            path,
            servername: host,
            ca,
            headers: { Host: host, ...headers },
          },
          (res) => {
            let body = "";
            res.setEncoding("utf8");
            res.on("data", (chunk) => {
              body += chunk;
            });
            res.on("end", () =>
              resolve({ status: res.statusCode, body, headers: res.headers }),
            );
          },
        );
        req.on("upgrade", (res, socket) => {
          socket.destroy();
          resolve({ status: res.statusCode, headers: res.headers });
        });
        req.on("error", reject);
        req.setTimeout(3000, () => req.destroy(new Error("request timeout")));
      });
    }
    let ready = false;
    for (let i = 0; i < 50; i++) {
      try {
        await request();
        ready = true;
        break;
      } catch {
        await delay(20);
      }
    }
    assert(ready, errors);
    for (const path of ["/", "/api/info", "/api/events", "/stream"])
      assert.equal((await request(path)).status, 401, path);
    assert.equal(
      (
        await request("/", settings.app, {
          Authorization: "Basic d3Jvbmc6d3Jvbmc=",
        })
      ).status,
      401,
    );
    const authorised = await request("/api/info?q=kept", settings.app, {
      Authorization: authorization,
      "X-VK-Relayed": "1",
    });
    assert.equal(authorised.status, 200);
    const data = JSON.parse(authorised.body);
    assert.equal(data.path, "/api/info?q=kept");
    assert.equal(data.headers.authorization, undefined);
    assert.equal(data.headers["x-vk-relayed"], undefined);
    assert.equal(data.headers["x-forwarded-proto"], "https");
    for (const path of [
      "/api/preview/5173",
      "/api/preview/5173/index.html",
      "/api/host/host-id/preview/5173",
    ])
      assert.equal(
        (await request(path, settings.app, { Authorization: authorization }))
          .status,
        403,
      );
    assert.equal((await request("/", "5173.preview.example.test")).status, 401);
    assert.equal(
      (
        await request("/", "5173.preview.example.test", {
          Authorization: authorization,
        })
      ).status,
      200,
    );
    for (const target of [3000, 3001, 8443])
      assert.equal(
        (
          await request("/", `${target}.preview.example.test`, {
            Authorization: authorization,
          })
        ).status,
        403,
      );
    const wsHeaders = {
      Upgrade: "websocket",
      Connection: "Upgrade",
      "Sec-WebSocket-Protocol": "vite-hmr",
    };
    assert.equal((await request("/ws", settings.app, wsHeaders)).status, 401);
    assert.equal(
      (
        await request("/ws", settings.app, {
          ...wsHeaders,
          Authorization: authorization,
        })
      ).status,
      101,
    );
    assert.equal(
      (
        await request("/ws", "5173.preview.example.test", {
          ...wsHeaders,
          Authorization: authorization,
        })
      ).status,
      101,
    );
    await new Promise((resolve, reject) => {
      const req = https.get(
        {
          hostname: "127.0.0.1",
          port,
          path: "/stream",
          servername: settings.app,
          ca,
          headers: { Host: settings.app, Authorization: authorization },
        },
        (res) => {
          res.once("data", (chunk) => {
            try {
              assert.match(chunk.toString(), /data: first/);
              assert.equal(streamEnded, false);
              resolve();
            } catch (error) {
              reject(error);
            } finally {
              req.destroy();
            }
          });
        },
      );
      req.on("error", reject);
      req.setTimeout(3000, () => req.destroy(new Error("stream timeout")));
    });
  },
);
