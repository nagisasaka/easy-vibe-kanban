import http from "node:http";
import https from "node:https";
import { isIP } from "node:net";
import { authority, serverSettings } from "./server.mjs";

function check(client, options, expected) {
  return new Promise((resolve, reject) => {
    const req = client.get(options, (res) => {
      res.resume();
      if (client === https) {
        const certificate = res.socket.getPeerCertificate();
        // Flag failed renewals while there is still time to repair them.
        if (!(Date.parse(certificate.valid_to) > Date.now() + 6 * 3600_000)) {
          reject(new Error("Served TLS certificate expires within six hours"));
          return;
        }
      }
      if (res.statusCode !== expected)
        reject(
          new Error(`Expected HTTP ${expected}, received ${res.statusCode}`),
        );
      else resolve();
    });
    req.setTimeout(4000, () => req.destroy(new Error("Healthcheck timed out")));
    req.on("error", reject);
  });
}

try {
  const { app } = serverSettings(process.env);
  await Promise.all([
    check(http, { hostname: "127.0.0.1", port: 3000, path: "/health" }, 200),
    // This is an internal liveness probe, not external certificate verification.
    check(
      https,
      {
        hostname: "127.0.0.1",
        port: 8443,
        path: "/",
        servername: isIP(app) ? "" : app,
        headers: { Host: authority(app) },
        rejectUnauthorized: false,
      },
      401,
    ),
  ]);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
