#!/usr/bin/env python3
"""Install the optional updater on a Linux/systemd Docker host, as root."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--deployment", required=True)
    parser.add_argument("--repository", required=True, help="GitHub owner/repository")
    parser.add_argument("--compose-file", action="append", required=True)
    parser.add_argument("--state-directory", default="/var/lib/lvk-updater")
    args = parser.parse_args()
    if os.geteuid() != 0:
        parser.error("Run the installer with sudo on the Docker host")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository):
        parser.error("Expected GitHub owner/repository")
    deployment = Path(args.deployment).resolve(strict=True)
    root = Path(args.state_directory).absolute()
    config_path = Path("/etc/lvk-updater.json")
    if config_path.exists():
        parser.error("Already installed. Review /etc/lvk-updater.json and update the host kit explicitly")
    credentials = deployment / "secrets/e2e.json"
    value = json.loads(credentials.read_text())
    if set(value) != {"username", "password"} or not all(isinstance(v, str) and v for v in value.values()):
        parser.error("secrets/e2e.json must contain username and password")
    os.chown(credentials, 0, 10001)
    credentials.chmod(0o640)
    for file in args.compose_file:
        if not (deployment / file).is_file():
            parser.error(f"Missing compose file: {file}")
    root.mkdir(mode=0o755)
    for name in ("requests", "status", "jobs", "backups", "maintenance"):
        path = root / name
        path.mkdir(mode=0o700 if name in ("jobs", "backups") else 0o755)
        if name == "requests":
            os.chown(path, 10001, 10001)
    kit = Path("/opt/lvk-developer")
    kit.mkdir(mode=0o755, exist_ok=True)
    for name in ("updater.py", "client.py", "verify-image.sh", "README.md"):
        shutil.copy2(Path(__file__).with_name(name), kit / name)
        (kit / name).chmod(0o755 if name.endswith((".py", ".sh")) else 0o644)
    shutil.copy2(Path(__file__).with_name("compose.updater.yaml"), deployment / "compose.updater.yaml")
    compose_files = args.compose_file + ["compose.updater.yaml"]
    config = {
        "repository": args.repository,
        "deployment_directory": str(deployment),
        "state_directory": str(root),
        "compose_files": compose_files,
        "services": ["lvk", "development"],
        "health_timeout_seconds": 2700,
    }
    config_path.write_text(json.dumps(config, indent=2) + "\n")
    config_path.chmod(0o600)
    # Source code and configuration stay host-owned. The runtime cannot edit them.
    Path("/etc/systemd/system/lvk-updater.service").write_text("""[Unit]
Description=LVK developer image updater
After=docker.service network-online.target
Requires=docker.service
[Service]
Type=oneshot
ExecStart=/usr/bin/python3 /opt/lvk-developer/updater.py --config /etc/lvk-updater.json
TimeoutStartSec=3h
UMask=0077
""")
    Path("/etc/systemd/system/lvk-updater.timer").write_text("""[Unit]
Description=Check queued LVK developer updates
[Timer]
OnBootSec=60
OnUnitInactiveSec=30
Unit=lvk-updater.service
[Install]
WantedBy=timers.target
""")
    subprocess.run(["systemctl", "daemon-reload"], check=True)
    subprocess.run(["systemctl", "enable", "--now", "lvk-updater.timer"], check=True)
    print("Installed. Add compose.updater.yaml when recreating the deployment after current work finishes.")
    if root != Path("/var/lib/lvk-updater"):
        print(f"Set LVK_UPDATER_STATE={root} in the host .env before using the Compose overlay.")


if __name__ == "__main__":
    main()
