import http from "node:http";
import https from "node:https";

function check(client, options, expected) {
  return new Promise((resolve, reject) => {
    const req = client.get(options, (res) => {
      res.resume();
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
  await Promise.all([
    check(http, { hostname: "127.0.0.1", port: 3000, path: "/health" }, 200),
    // This is an internal liveness probe, not external certificate verification.
    check(
      https,
      {
        hostname: "127.0.0.1",
        port: 8443,
        path: "/",
        servername: process.env.EVK_DOMAIN,
        headers: { Host: process.env.EVK_DOMAIN },
        rejectUnauthorized: false,
      },
      401,
    ),
  ]);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
