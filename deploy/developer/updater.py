#!/usr/bin/env python3
"""Optional Linux host service. Never installed in the LVK application image.

Only the fixed host configuration controls Docker commands/paths. Requests can
select an immutable image from the configured repository, not shell commands.
"""
import argparse
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import stat
import subprocess
import time
import urllib.request
import uuid


class Busy(Exception):
    pass


TERMINAL = {"succeeded", "failed", "cancelled", "rolled_back", "recovery_required"}
IN_PROGRESS = {"freezing", "stopped", "backing_up", "applying", "verifying", "rolling_back"}
RUN_TERMINAL = ("succeeded", "failed", "cancelled", "crashed", "audit_failed")
DB_RULES = {
    "agent_runs": ("status", RUN_TERMINAL),
    "agent_run_attempts": ("status", RUN_TERMINAL),
    "agent_process_registry": ("registry_status", ("exited",)),
    "agent_run_commands": ("delivery_status", ("delivered", "failed")),
    "orchestration_outbox": ("delivery_status", ("delivered", "failed")),
    "execution_processes": ("status", ("completed", "failed", "killed")),
    "orchestration_runs": ("status", ("succeeded", "failed", "cancelled")),
    "workflow_runs": ("status", ("succeeded", "failed", "canceled")),
    "integration_runs": ("status", ("succeeded", "failed", "cancelled", "blocked")),
    "resource_operations": ("status", ("succeeded", "cancelled", "recovered")),
    "resource_mediations": ("status", ("applied", "stale", "failed")),
}


def atomic_json(path, value, mode=0o600):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    with os.fdopen(os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, mode), "w") as stream:
        os.fchmod(stream.fileno(), mode)
        json.dump(value, stream, indent=2)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(tmp, path)
    fd = os.open(path.parent, os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def read_request(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > 4096:
            raise ValueError("Request must be a small regular JSON file")
        return json.loads(stream.read(4097))


def validate_request(request, repository):
    if set(request) != {"image", "revision", "run_id"}:
        raise ValueError("Unexpected request fields")
    if not re.fullmatch(re.escape(f"ghcr.io/{repository.lower()}@sha256:") + r"[a-f0-9]{64}", request["image"]):
        raise ValueError("Image must be pinned to a digest in the configured repository")
    if not re.fullmatch(r"[a-f0-9]{40}", request["revision"]):
        raise ValueError("Expected a full commit SHA")
    if type(request["run_id"]) is not int or request["run_id"] <= 0:
        raise ValueError("Expected a positive Actions run ID")
    return request


def validate_release(run, labels, request, repository):
    if not (
        run["id"] == request["run_id"]
        and run["repository"]["full_name"].lower() == repository.lower()
        and run["head_repository"]["full_name"].lower() == repository.lower()
        and run["path"] == ".github/workflows/publish-server.yml"
        and run["event"] == "push"
        and run["status"] == "completed"
        and run["conclusion"] == "success"
        and run["head_sha"] == request["revision"]
        and labels.get("org.opencontainers.image.revision") == request["revision"]
        and labels.get("org.opencontainers.image.source", "").lower() == f"https://github.com/{repository.lower()}"
        and labels.get("io.lvk.actions-run-id") == str(request["run_id"])
        and labels.get("io.lvk.maintenance-queue-barrier") == "1"
        and labels.get("io.lvk.maintenance-http-barrier") == "1"
    ):
        raise ValueError("Image identity and successful release workflow do not match")


def assert_database_idle(path):
    # Read the WAL normally. immutable=1 would silently ignore committed WAL data.
    if not path.is_file():
        raise Busy("Application database is missing; cannot establish idle state")
    try:
        with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=0.1) as db:
            for table, (column, terminal) in DB_RULES.items():
                placeholders = ",".join("?" for _ in terminal)
                count = db.execute(f"SELECT count(*) FROM {table} WHERE {column} NOT IN ({placeholders})", terminal).fetchone()[0]
                if count:
                    raise Busy(f"{table}: {count} unfinished operation(s)")
            if db.execute("SELECT count(*) FROM resource_holders").fetchone()[0]:
                raise Busy("Shared resources are still owned")
            if db.execute("SELECT count(*) FROM scheduled_tasks WHERE enabled=1").fetchone()[0]:
                raise Busy("Disable scheduled tasks before updating")
            if db.execute("""SELECT count(*) FROM agent_run_state st JOIN agent_runs r ON r.id=st.agent_run_id
                WHERE json_extract(st.state_json,'$.goal.status')='active'
                AND r.id=(SELECT a.id FROM agent_runs a JOIN agent_run_state ast ON ast.agent_run_id=a.id WHERE a.session_id=r.session_id ORDER BY a.created_at DESC,a.id DESC LIMIT 1)""").fetchone()[0]:
                raise Busy("An active goal must finish or be explicitly paused")
    except sqlite3.Error as error:
        raise Busy(f"Cannot establish database idle state: {error}") from error


def known_service_process(command, development=False):
    common = [
        r"/usr/bin/tini -- node /opt/lvk-server/server\.mjs",
        r"node /opt/lvk-server/server\.mjs", r"/usr/local/bin/server",
        r"nginx: master process nginx -e stderr -c /tmp/lvk-server/nginx\.conf -g daemon off;",
        r"nginx: worker process",
    ]
    dev = [
        r"/sbin/docker-init -- node /opt/lvk-development/development\.mjs",
        r"node /opt/lvk-development/development\.mjs",
        r"node /usr/local/bin/pnpm run (dev:container|backend:dev:watch)",
        r"sh -c \./scripts/start-container-dev\.sh",
        r"node (/usr/local/bin/pnpm exec concurrently|\./node_modules/\.bin/\.\./concurrently/dist/bin/concurrently\.js) --kill-others --names backend,frontend --prefix-colors cyan,magenta pnpm run backend:dev:watch pnpm --dir packages/local-web run dev --host \"\$HOST\" --port \"\$FRONTEND_PORT\" --strictPort",
        r"/bin/sh -c pnpm run backend:dev:watch",
        r'/bin/sh -c pnpm --dir packages/local-web run dev --host "\$HOST" --port "\$FRONTEND_PORT" --strictPort',
        r"node /usr/local/bin/pnpm --dir packages/local-web run dev --host 127\.0\.0\.1 --port 4020 --strictPort",
        r"sh -c DISABLE_WORKTREE_CLEANUP=1 RUST_LOG=debug cargo watch -w crates -x 'build --bin server --bin agent-process-host' -x 'run --bin server'",
        r"/usr/local/bin/cargo-watch watch -w crates -x build --bin server --bin agent-process-host -x run --bin server",
        r'sh -c VITE_OPEN=\$\{VITE_OPEN:-false\} vite --host 127\.0\.0\.1 --port 4020 --strictPort',
        r"node /repos/[^ ]+/packages/local-web/node_modules/\.bin/\.\./vite/bin/vite\.js --host 127\.0\.0\.1 --port 4020 --strictPort",
        r"/repos/[^ ]+/node_modules/\.pnpm/@esbuild\+linux-x64@[0-9.]+/node_modules/@esbuild/linux-x64/bin/esbuild --service=[0-9.]+ --ping",
        r'sh -c cargo build --bin server --bin agent-process-host && cargo run --bin server; echo "\[Finished running\. Exit status: \$\?\]"',
        r"/var/tmp/lvk-target/debug/server",
    ]
    return any(re.fullmatch(pattern, command) for pattern in common + (dev if development else []))


def database_paths(container):
    mounts = container["info"]["Mounts"]
    env = dict(value.split("=", 1) for value in container["info"]["Config"]["Env"] if "=" in value)
    if env.get("XDG_DATA_HOME", "/home/appuser/.local/share") != "/home/appuser/.local/share":
        raise Busy("Custom XDG_DATA_HOME requires a reviewed host updater configuration")
    if container["service"] == "development":
        repo = Path(env.get("LVK_DEV_REPO", "/repos/lucky-vibe-kanban"))
        relative = repo.relative_to("/repos")
        if ".." in relative.parts:
            raise Busy("Development checkout is outside the supported repos volume")
        repos = next(m for m in mounts if m["Destination"] == "/repos")
        return Path(repos["Source"]) / relative / "dev_assets/db.v2.sqlite", repo / "dev_assets/db.v2.sqlite"
    home = next(m for m in mounts if m["Destination"] == "/home/appuser")
    return Path(home["Source"]) / ".local/share/vibe-kanban/db.v2.sqlite", Path("/home/appuser/.local/share/vibe-kanban/db.v2.sqlite")


class Updater:
    def __init__(self, config):
        self.config = config
        self.root = Path(config["state_directory"])
        self.gate = self.root / "maintenance" / "active"
        self.compose = ["docker", "compose", "--project-directory", config["deployment_directory"]]
        for file in config["compose_files"]:
            self.compose += ["-f", str(Path(config["deployment_directory"]) / file)]
        self.compose += ["--profile", "development"]
        self.services = config.get("services", ["lvk", "development"])
        if not self.services or set(self.services) - {"lvk", "development"}:
            raise ValueError("Only LVK and its optional development service can be updated")

    def command(self, args, timeout=600):
        result = subprocess.run(args, capture_output=True, text=True, timeout=timeout, check=False)
        if result.returncode:
            # Do not leak process arguments, registry credentials, or application logs.
            raise RuntimeError(f"{Path(args[0]).name} command failed ({result.returncode}): {result.stderr[-1000:]}")
        return result.stdout

    def save(self, job, status, **values):
        job.update(status=status, updated_at=datetime.now(timezone.utc).isoformat(), **values)
        atomic_json(self.root / "jobs" / f"{job['id']}.json", job)
        public = {k: job[k] for k in ("id", "status", "updated_at", "request", "message") if k in job}
        atomic_json(self.root / "status" / f"{job['id']}.json", public, 0o644)
        print(f"{job['id']}: {status}", flush=True)

    def inspect(self, container):
        return json.loads(self.command(["docker", "inspect", container]))[0]

    def containers(self):
        result = []
        for service in self.services:
            cid = self.command(self.compose + ["ps", "-q", service]).strip()
            if not cid or "\n" in cid:
                raise Busy(f"Expected one running {service} container")
            info = self.inspect(cid)
            if not info["State"]["Running"] or info["State"]["Paused"]:
                raise Busy(f"{service} must be running and not already paused")
            result.append({"id": cid, "service": service, "info": info})
        return result

    def assert_idle(self, containers):
        for container in containers:
            database, _ = database_paths(container)
            assert_database_idle(database)
            lines = self.command(["docker", "top", container["id"], "-eo", "pid,args"]).splitlines()[1:]
            commands = [line.strip().split(None, 1)[1] for line in lines]
            if not commands or any(not known_service_process(c.strip(), container["service"] == "development") for c in commands):
                raise Busy(f"{container['service']}: extra processes are running (agent, terminal, build or browser)")

    def check_queues(self, containers):
        # The queue GET takes the same admission mutex as queue POST. After the
        # host gate closes, every older writer finishes before this read, and
        # newer queue POSTs are rejected. This includes non-durable follow-ups.
        code = """import json,sqlite3,sys,urllib.request,uuid
db=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True)
with urllib.request.urlopen('http://127.0.0.1:'+sys.argv[2]+'/api/maintenance/ready',timeout=30) as response:
 if response.status!=200 or response.headers.get('x-lvk-maintenance-barrier')!='1': raise RuntimeError('Application does not support draining HTTP handlers')
for (sid,) in db.execute('SELECT id FROM sessions'):
 url='http://127.0.0.1:'+sys.argv[2]+'/api/sessions/'+str(uuid.UUID(bytes=sid))+'/queue'
 with urllib.request.urlopen(url,timeout=15) as response:
  if response.headers.get('x-lvk-maintenance-fence')!='1': raise RuntimeError('Application lacks maintenance queue fence; bootstrap the current release first')
  data=json.load(response)
  if not data.get('success') or data['data']['status']!='empty': raise RuntimeError('A follow-up is queued; finish or cancel it before updating')
"""
        for c in containers:
            mount = next((m for m in c["info"]["Mounts"] if m["Destination"] == "/run/lvk-maintenance"), None)
            if not mount or Path(mount["Source"]) != self.gate.parent or mount["RW"]:
                raise Busy("Install the read-only maintenance mount before enabling host updates")
            _, path = database_paths(c)
            port = "4021" if c["service"] == "development" else "3000"
            try:
                self.command(["docker", "exec", c["id"], "python3", "-c", code, str(path), port], timeout=120)
            except (RuntimeError, subprocess.TimeoutExpired) as error:
                raise Busy(str(error)) from error

    def verify_release(self, request):
        headers = {"Accept": "application/vnd.github+json", "User-Agent": "lvk-host-updater"}
        if self.config.get("github_token_file"):
            headers["Authorization"] = "Bearer " + Path(self.config["github_token_file"]).read_text().strip()
        url = f"https://api.github.com/repos/{self.config['repository']}/actions/runs/{request['run_id']}"
        with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=30) as response:
            run = json.load(response)
        self.command(["docker", "pull", request["image"]], timeout=1800)
        labels = self.inspect(request["image"])["Config"]["Labels"] or {}
        validate_release(run, labels, request, self.config["repository"])
        self.command(["bash", str(Path(__file__).with_name("verify-image.sh")), request["image"]], timeout=600)

    def resume(self, containers):
        for c in containers:
            if self.inspect(c["id"])["State"]["Paused"]:
                self.command(["docker", "unpause", c["id"]])

    def volumes(self, containers):
        volumes = {}
        for c in containers:
            for mount in c["info"]["Mounts"]:
                if mount["Type"] == "volume":
                    volumes[mount["Name"]] = mount["Source"]
        # Never restore a shared volume while another running container writes it.
        selected = {c["id"] for c in containers}
        ids = self.command(["docker", "ps", "-q", "--no-trunc"]).split()
        for cid in ids:
            if cid not in selected and any(m.get("Name") in volumes for m in self.inspect(cid)["Mounts"]):
                raise Busy("Persistent volumes are also used by another running container")
        return volumes

    def backup(self, job):
        destination = self.root / "backups" / job["id"]
        destination.mkdir(mode=0o700)
        needed = sum(int(self.command(["du", "-sb", source]).split()[0]) for source in job["volumes"].values())
        if shutil.disk_usage(destination).free < needed + 1024**3:
            raise RuntimeError("Insufficient backup space; no image or data was changed")
        for name, source in job["volumes"].items():
            # The development Cargo target is reproducible and can exceed 30 GB.
            excludes = ["--exclude=./lvk-target"] if name.endswith("_development-work") else []
            self.command(["tar", "--create", "--file", str(destination / f"{name}.tar"), *excludes, "--directory", source, "."], timeout=1800)
        deployment = Path(self.config["deployment_directory"])
        shutil.copy2(deployment / ".env", destination / "environment")
        self.command(["tar", "--create", "--file", str(destination / "configuration.tar"), "--directory", str(deployment), ".env", "secrets", *self.config["compose_files"]], timeout=120)
        checksums = {}
        for path in destination.iterdir():
            with path.open("rb") as stream:
                checksums[path.name] = hashlib.file_digest(stream, "sha256").hexdigest()
        atomic_json(destination / "checksums.json", checksums)
        os.sync()
        job["backup_complete"] = True

    def set_image(self, image):
        path = Path(self.config["deployment_directory"]) / ".env"
        lines = path.read_text().splitlines()
        lines = [line for line in lines if not re.match(r"^\s*LVK_IMAGE\s*=", line)]
        temp = path.with_suffix(".update-tmp")
        temp.write_text("\n".join(lines + [f"LVK_IMAGE={image}"]) + "\n")
        temp.chmod(0o600)
        temp.replace(path)

    def start_and_verify(self, expected_image):
        self.command(self.compose + ["up", "-d", "--no-build", "--pull", "never", "--force-recreate", *self.services], timeout=300)
        expected_id = self.inspect(expected_image)["Id"]
        deadline = time.monotonic() + self.config.get("health_timeout_seconds", 2700)
        while time.monotonic() < deadline:
            containers = self.containers()
            if any(c["info"]["Image"] != expected_id for c in containers):
                raise RuntimeError("Compose started an unexpected image; check host environment overrides")
            if all(c["info"]["State"].get("Health", {}).get("Status") == "healthy" for c in containers):
                for c in containers:
                    port = 4020 if c["service"] == "development" else 3000
                    self.command(["docker", "exec", "-e", f"LVK_E2E_BASE_URL=http://127.0.0.1:{port}", c["id"], "node", "/opt/lvk-browser/smoke.mjs"], timeout=210)
                return
            time.sleep(5)
        raise RuntimeError("Updated application did not become healthy before the deadline")

    def stop_frozen(self, containers):
        for c in containers:
            # SIGKILL on a frozen, verified idle container closes the admission
            # race: no scheduler/request can launch between the check and stop.
            # SQLite recovers committed WAL data; backup includes WAL and SHM.
            self.command(["docker", "update", "--restart=no", c["id"]])
            self.command(["docker", "kill", c["id"]])

    def rollback(self, job):
        self.save(job, "rolling_back")
        backup = self.root / "backups" / job["id"]
        for name, expected in json.loads((backup / "checksums.json").read_text()).items():
            with (backup / name).open("rb") as stream:
                if hashlib.file_digest(stream, "sha256").hexdigest() != expected:
                    raise RuntimeError("Backup checksum mismatch; persistent data was not touched")
        ids = self.command(self.compose + ["ps", "-q"]).split()
        running = []
        for cid in ids:
            info = self.inspect(cid)
            if info["Config"].get("Labels", {}).get("com.docker.compose.service") in self.services:
                running.append({"id": cid, "service": info["Config"]["Labels"]["com.docker.compose.service"], "info": info})
        try:
            # The verified candidate started behind a closed admission gate;
            # no in-memory queue could be accepted. Do not require its HTTP API
            # to work in order to roll back a failed startup.
            for c in running:
                self.command(["docker", "pause", c["id"]])
            self.assert_idle(running)
            self.stop_frozen(running)
        except Exception:
            self.resume(running)
            raise
        for name, source in job["volumes"].items():
            root = Path(source)
            for child in root.iterdir():
                if name.endswith("_development-work") and child.name == "lvk-target":
                    continue
                if child.is_dir() and not child.is_symlink():
                    shutil.rmtree(child)
                else:
                    child.unlink()
            self.command(["tar", "--extract", "--file", str(backup / f"{name}.tar"), "--directory", source], timeout=1800)
        shutil.copy2(backup / "environment", Path(self.config["deployment_directory"]) / ".env")
        self.command(self.compose + ["up", "-d", "--no-build", "--pull", "never", "--force-recreate", *self.services], timeout=300)
        # The previous release may predate the browser runtime. Its Docker
        # healthcheck is the compatible recovery check.
        deadline = time.monotonic() + self.config.get("health_timeout_seconds", 2700)
        while time.monotonic() < deadline:
            if all(c["info"]["State"].get("Health", {}).get("Status") == "healthy" for c in self.containers()):
                self.save(job, "rolled_back", message="Update failed; matching previous image and persistent data restored")
                self.gate.unlink(missing_ok=True)
                return
            time.sleep(5)
        raise RuntimeError("Recovery healthcheck failed; maintenance remains enabled")

    def process(self, job):
        containers = []
        try:
            if not job.get("validated"):
                self.save(job, "validating")
                self.verify_release(job["request"])
                job["validated"] = True
            containers = self.containers()
            self.assert_idle(containers)
            volumes = self.volumes(containers)
            self.save(job, "freezing", containers=containers, volumes=volumes)
            atomic_json(self.gate, {"job_id": job["id"]}, 0o644)
            self.check_queues(containers)
            for c in containers:
                self.command(["docker", "pause", c["id"]])
            self.assert_idle(containers)
            self.stop_frozen(containers)
            self.save(job, "stopped")
            self.save(job, "backing_up")
            self.backup(job)
            self.save(job, "applying")
            self.set_image(job["request"]["image"])
            self.save(job, "verifying")
            self.start_and_verify(job["request"]["image"])
            self.save(job, "succeeded", message="Verified image deployed; browser and backend checks passed")
            self.gate.unlink(missing_ok=True)
        except Busy as error:
            if job["status"] in {"validating", "queued", "waiting", "freezing"}:
                self.resume(containers)
                self.gate.unlink(missing_ok=True)
                self.save(job, "waiting", message=str(error))
            else:
                self.fail(job, error)
        except Exception as error:
            self.fail(job, error)

    def fail(self, job, error):
        message = str(error)[:1500]
        if job["status"] in {"succeeded", "rolled_back"}:
            print("Deployment committed; maintenance cleanup will retry on the next tick", flush=True)
            return
        if job.get("backup_complete"):
            try:
                self.rollback(job)
                return
            except Exception as recovery:
                message += f"; rollback failed: {recovery}"
        elif job["status"] not in IN_PROGRESS:
            self.save(job, "failed", message=message)
            return
        # Preserve volumes/backups and block subsequent updates for inspection.
        self.save(job, "recovery_required", message=message)

    def recover(self, job):
        if job["status"] != "recovery_required":
            raise ValueError("Only recovery_required jobs need host recovery")
        if job.get("backup_complete"):
            self.rollback(job)
        else:
            # No image change occurs before a complete, journalled backup.
            for c in job.get("containers", []):
                info = self.inspect(c["id"])
                if info["State"]["Paused"]:
                    self.command(["docker", "unpause", c["id"]])
                policy = c["info"]["HostConfig"]["RestartPolicy"]["Name"]
                self.command(["docker", "update", f"--restart={policy}", c["id"]])
                if not info["State"]["Running"]:
                    self.command(["docker", "start", c["id"]])
            self.gate.unlink(missing_ok=True)
            self.save(job, "failed", message="Previous containers resumed; update was not applied. Submit a new job after resolving the cause.")

    def tick(self):
        if self.gate.exists():
            owner = json.loads(self.gate.read_text())["job_id"]
            if str(uuid.UUID(owner)) != owner:
                raise ValueError("Invalid maintenance owner")
            saved = json.loads((self.root / "jobs" / f"{owner}.json").read_text())
            if saved["status"] in {"succeeded", "rolled_back"}:
                self.gate.unlink()
        jobs = []
        for path in sorted((self.root / "jobs").glob("*.json")):
            job = json.loads(path.read_text())
            if job["status"] in IN_PROGRESS:
                self.save(job, "recovery_required", message="Host updater was interrupted; inspect saved journal and backup before recovery")
            if job["status"] == "recovery_required":
                return
            if job["status"] not in TERMINAL:
                jobs.append(job)
        for path in sorted((self.root / "requests").glob("*.json")):
            try:
                job_id = str(uuid.UUID(path.stem))
                if job_id != path.stem or (self.root / "jobs" / path.name).exists():
                    continue
                job = {"id": job_id, "status": "queued"}
                job["request"] = validate_request(read_request(path), self.config["repository"])
                self.save(job, "queued")
                jobs.append(job)
            except (ValueError, TypeError, KeyError, OSError) as error:
                print(f"Rejected request {path.name}: {type(error).__name__}", flush=True)
                try:
                    job_id = str(uuid.UUID(path.stem))
                    self.save({"id": job_id}, "failed", message="Invalid update request; expected an allowed digest, source revision and release run ID")
                except ValueError:
                    pass
        for job in jobs:
            if (self.root / "requests" / f"{job['id']}.cancel").exists():
                self.save(job, "cancelled", message="Cancelled before deployment")
            else:
                self.process(job)
                break


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", default="/etc/lvk-updater.json")
    parser.add_argument("--recover", help="Host-only recovery of an interrupted job UUID")
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text())
    updater = Updater(config)
    with (updater.root / "worker.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        if args.recover:
            job_id = str(uuid.UUID(args.recover))
            updater.recover(json.loads((updater.root / "jobs" / f"{job_id}.json").read_text()))
        else:
            updater.tick()


if __name__ == "__main__":
    main()
