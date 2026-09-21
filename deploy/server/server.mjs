import { spawn } from "node:child_process";
import { X509Certificate, createPrivateKey } from "node:crypto";
import { constants } from "node:fs";
import { access, mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

export function domain(value, name) {
  if (
    typeof value !== "string" ||
    value.length > 253 ||
    !/^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(
      value,
    )
  ) {
    throw new Error(
      `${name} must be a lowercase DNS hostname, without scheme, port, or path`,
    );
  }
  return value;
}

export function serverSettings(env) {
  const app = domain(env.EVK_DOMAIN, "EVK_DOMAIN");
  const preview = env.EVK_PREVIEW_DOMAIN
    ? domain(env.EVK_PREVIEW_DOMAIN, "EVK_PREVIEW_DOMAIN")
    : null;
  if (preview && (app === preview || app.endsWith(`.${preview}`))) {
    throw new Error("EVK_DOMAIN must be outside the preview wildcard domain");
  }
  return { app, preview };
}

export function renderNginx(template, settings) {
  const preview = settings.preview;
  const previewServer = preview
    ? `server {
        listen 8443 ssl;
        server_name "~^(?<preview_target>102[4-9]|10[3-9][0-9]|1[1-9][0-9]{2}|[2-9][0-9]{3}|[1-5][0-9]{4}|6[0-4][0-9]{3}|65[0-4][0-9]{2}|655[0-2][0-9]|6553[0-5])[.]${preview.replaceAll(".", "[.]")}$";
        # Only unprivileged development ports, excluding EVK and nginx ports.
        if ($preview_target ~ "^(3000|3001|8443)$") { return 403; }
        location / { proxy_pass http://127.0.0.1:3001; }
    }`
    : "";
  return template
    .replace("@@APP_DOMAIN@@", settings.app)
    .replace("@@PREVIEW_SERVER@@", previewServer);
}

export function validateSecrets(
  certificate,
  key,
  passwords,
  settings,
  now = Date.now(),
) {
  const cert = new X509Certificate(certificate);
  if (Date.parse(cert.validFrom) > now || Date.parse(cert.validTo) <= now) {
    throw new Error(
      "TLS certificate is not currently valid; renew it before starting",
    );
  }
  for (const host of [
    settings.app,
    ...(settings.preview ? [`3000.${settings.preview}`] : []),
  ]) {
    if (!cert.checkHost(host))
      throw new Error(`TLS certificate does not cover ${host}`);
  }
  if (!cert.checkPrivateKey(createPrivateKey(key)))
    throw new Error("TLS certificate/key mismatch");
  const entries = passwords
    .split(/\r?\n/)
    .filter((line) => line.trim() && !line.startsWith("#"));
  if (
    !entries.length ||
    entries.some(
      (line) =>
        !/^[A-Za-z0-9_.-]+:\$2[aby]\$\d{2}\$[./A-Za-z0-9]{53}$/.test(line),
    )
  ) {
    throw new Error(
      "htpasswd must contain bcrypt entries; generate it with htpasswd -B",
    );
  }
}

// Keep public listeners out of the Rust services, irrespective of inherited env.
export function backendEnvironment(env, settings) {
  return {
    ...env,
    HOST: "127.0.0.1",
    PORT: "",
    BACKEND_PORT: "3000",
    PREVIEW_PROXY_PORT: "3001",
    VK_ALLOWED_ORIGINS: `https://${settings.app}`,
    VK_PREVIEW_DOMAIN: settings.preview ?? "",
    BROWSER: "true",
  };
}

export function supervise(
  commands,
  env,
  {
    spawnProcess = spawn,
    shutdownMs = 120_000,
    signalProcess = process.kill.bind(process),
  } = {},
) {
  if (!commands.length)
    return Promise.reject(new Error("No services configured"));
  return new Promise((resolve) => {
    const children = new Set();
    let stopping = false;
    let result = 0;
    let timeout;
    const kill = (child, signal) => {
      if (child.pid) {
        try {
          signalProcess(-child.pid, signal);
        } catch (error) {
          if (error.code !== "ESRCH") {
            result = 1;
            console.error(
              `Unable to signal service ${child.pid}: ${error.message}`,
            );
          }
        }
      }
    };
    const finish = () => {
      if (!stopping || children.size) return;
      clearTimeout(timeout);
      process.off("SIGTERM", stop);
      process.off("SIGINT", stop);
      resolve(result);
    };
    const stop = () => {
      if (stopping) return;
      stopping = true;
      for (const child of children) kill(child, "SIGTERM");
      timeout = setTimeout(() => {
        for (const child of children) kill(child, "SIGKILL");
      }, shutdownMs);
      finish();
    };
    process.on("SIGTERM", stop);
    process.on("SIGINT", stop);
    for (const [command, ...args] of commands) {
      let child;
      try {
        child = spawnProcess(command, args, {
          env,
          stdio: "inherit",
          detached: true,
        });
      } catch (error) {
        console.error(`Unable to start service ${command}: ${error.message}`);
        result = 1;
        stop();
        break;
      }
      children.add(child);
      child.once("error", () => {
        result = 1;
        children.delete(child);
        stop();
        finish();
      });
      child.once("exit", () => {
        children.delete(child);
        if (!stopping) {
          result = 1;
          stop();
        }
        finish();
      });
    }
  });
}

async function main() {
  const settings = serverSettings(process.env);
  const secrets = "/run/evk-secrets";
  validateSecrets(
    await readFile(`${secrets}/tls/fullchain.pem`),
    await readFile(`${secrets}/tls/privkey.pem`),
    await readFile(`${secrets}/htpasswd`, "utf8"),
    settings,
  );
  for (const dir of ["/home/appuser", "/repos", "/var/tmp"])
    await access(dir, constants.W_OK);
  for (const name of ["client", "proxy", "fastcgi", "uwsgi", "scgi"])
    await mkdir(`/tmp/evk-server/${name}`, { recursive: true, mode: 0o700 });
  const config = "/tmp/evk-server/nginx.conf";
  const template = await readFile(
    new URL("./nginx.conf.template", import.meta.url),
    "utf8",
  );
  await writeFile(config, renderNginx(template, settings), { mode: 0o600 });
  const check = spawn("nginx", ["-e", "stderr", "-t", "-c", config], {
    stdio: "inherit",
  });
  const code = await new Promise((resolve, reject) => {
    check.once("error", reject);
    check.once("exit", resolve);
  });
  if (code !== 0) throw new Error("nginx configuration validation failed");
  process.exitCode = await supervise(
    [
      ["/usr/local/bin/server"],
      ["nginx", "-e", "stderr", "-c", config, "-g", "daemon off;"],
    ],
    backendEnvironment(process.env, settings),
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    console.error(`EVK startup failed: ${error.message}`);
    process.exitCode = 1;
  });
}
