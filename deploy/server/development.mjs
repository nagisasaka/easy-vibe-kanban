import { spawn } from "node:child_process";
import { readFile, writeFile, mkdir, realpath } from "node:fs/promises";
import {
  authority,
  serverSettings,
  renderNginx,
  validateSecrets,
  certificateReloader,
  supervise,
} from "./server.mjs";

export function developmentEnvironment(env, settings) {
  const port = env.LVK_DEV_HTTPS_PORT || "8443";
  if (!/^\d+$/.test(port) || Number(port) < 1024 || Number(port) > 65535)
    throw new Error("LVK_DEV_HTTPS_PORT must be between 1024 and 65535");
  return {
    ...env,
    HOST: "127.0.0.1",
    PORT: "",
    FRONTEND_PORT: "4020",
    BACKEND_PORT: "4021",
    PREVIEW_PROXY_PORT: "4022",
    VK_ALLOWED_ORIGINS: `https://${authority(settings.app)}:${port}`,
    VK_PREVIEW_DOMAIN: "",
    VITE_VK_SHARED_API_BASE: "",
    VK_SHARED_API_BASE: "",
    BROWSER: "true",
    CARGO_BUILD_JOBS: env.CARGO_BUILD_JOBS || "2",
    CARGO_PROFILE_DEV_DEBUG: "0",
    CARGO_INCREMENTAL: "1",
    CARGO_TARGET_DIR: "/var/tmp/lvk-target",
  };
}

const run = (command, args, env) =>
  new Promise((resolve, reject) => {
    const child = spawn(command, args, { env, stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code) =>
      code === 0 ? resolve() : reject(new Error(`${command} exited ${code}`)),
    );
  });

async function main() {
  const settings = { ...serverSettings(process.env), preview: null };
  const env = developmentEnvironment(process.env, settings);
  const repo = await realpath(
    process.env.LVK_DEV_REPO || "/repos/lucky-vibe-kanban",
  );
  if (!repo.startsWith("/repos/"))
    throw new Error("LVK_DEV_REPO must be inside /repos");
  await readFile(`${repo}/package.json`);
  process.chdir(repo);
  const read = async () => {
    const [certificate, key, passwords] = await Promise.all([
      readFile("/run/lvk-secrets/tls/fullchain.pem"),
      readFile("/run/lvk-secrets/tls/privkey.pem"),
      readFile("/run/lvk-secrets/htpasswd", "utf8"),
    ]);
    return { certificate, key, passwords };
  };
  const initial = await read();
  validateSecrets(
    initial.certificate,
    initial.key,
    initial.passwords,
    settings,
  );
  for (const name of ["client", "proxy", "fastcgi", "uwsgi", "scgi"])
    await mkdir(`/tmp/lvk-server/${name}`, { recursive: true, mode: 0o700 });
  const template = await readFile(
    new URL("./nginx.conf.template", import.meta.url),
    "utf8",
  );
  const config = "/tmp/lvk-server/nginx.conf";
  await writeFile(
    config,
    renderNginx(template, settings)
      .replaceAll("http://127.0.0.1:3000", "http://127.0.0.1:4020")
      .replace("private development server", "source development server"),
    { mode: 0o600 },
  );
  const nginx = (args) =>
    run("nginx", ["-e", "stderr", ...args, "-c", config], env);
  await nginx(["-t"]);
  // Installation is cached in the persistent home; no application image build.
  await run("pnpm", ["install", "--frozen-lockfile"], env);
  const reload = certificateReloader({
    read,
    settings,
    initial,
    activate: async () => {
      await nginx(["-t"]);
      await nginx(["-s", "reload"]);
    },
  });
  const timer = setInterval(
    () => reload().catch((error) => console.error(error.message)),
    30_000,
  );
  try {
    process.exitCode = await supervise(
      [
        ["pnpm", "run", "dev:container"],
        ["nginx", "-e", "stderr", "-c", config, "-g", "daemon off;"],
      ],
      env,
    );
  } finally {
    clearInterval(timer);
  }
}

if (process.argv[1] && new URL(import.meta.url).pathname === process.argv[1]) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
