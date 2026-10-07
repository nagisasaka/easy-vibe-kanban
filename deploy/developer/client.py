#!/usr/bin/env python3
"""Unprivileged, asynchronous interface to the optional host updater."""
import argparse
import json
import os
from pathlib import Path
import re
import urllib.parse
import urllib.request
import uuid


def public_release(repository, tag):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Expected GitHub owner/repository")
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9]+(?:[.-][A-Za-z0-9]+)*)?", tag):
        raise ValueError("Expected a version tag such as v0.1.44-lvk.1-server.6")
    query = urllib.parse.urlencode({"event": "push", "branch": tag, "per_page": 10})
    request = urllib.request.Request(
        f"https://api.github.com/repos/{repository}/actions/workflows/publish-server.yml/runs?{query}",
        headers={"User-Agent": "lvk-update-client", "Accept": "application/vnd.github+json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        runs = json.load(response)["workflow_runs"]
    run = next((r for r in runs if r["status"] == "completed" and r["conclusion"] == "success"), None)
    if not run:
        raise ValueError("No successful release run for this tag yet; wait for CI to finish")
    repository = repository.lower()
    query = urllib.parse.urlencode({"scope": f"repository:{repository}:pull", "service": "ghcr.io"})
    with urllib.request.urlopen(f"https://ghcr.io/token?{query}", timeout=30) as response:
        token = json.load(response)["token"]
    request = urllib.request.Request(
        f"https://ghcr.io/v2/{repository}/manifests/{tag[1:]}", method="HEAD",
        headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        digest = response.headers["Docker-Content-Digest"]
    if not digest or not re.fullmatch(r"sha256:[a-f0-9]{64}", digest):
        raise ValueError("Registry did not return an immutable image digest")
    return {"image": f"ghcr.io/{repository}@{digest}", "revision": run["head_sha"], "run_id": run["id"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", default="/run/lvk-update")
    sub = parser.add_subparsers(dest="action", required=True)
    submit = sub.add_parser("submit")
    submit.add_argument("--image", required=True, help="GHCR reference pinned by sha256 digest")
    submit.add_argument("--revision", required=True, help="Full source commit SHA")
    submit.add_argument("--run-id", required=True, type=int, help="Successful release Actions run")
    release = sub.add_parser("release", help="Resolve a public fork's successful tag release and queue it")
    release.add_argument("--repository", required=True)
    release.add_argument("--tag", required=True)
    for action in ("status", "cancel"):
        sub.add_parser(action).add_argument("job_id")
    args = parser.parse_args()
    root = Path(args.directory)
    if args.action in ("submit", "release"):
        request = public_release(args.repository, args.tag) if args.action == "release" else {
            "image": args.image, "revision": args.revision, "run_id": args.run_id}
        if not re.fullmatch(r"ghcr\.io/[a-z0-9_.-]+/[a-z0-9_.-]+@sha256:[a-f0-9]{64}", request["image"]):
            parser.error("Use an immutable GHCR image digest")
        if not re.fullmatch(r"[a-f0-9]{40}", request["revision"]) or request["run_id"] <= 0:
            parser.error("A full commit SHA and positive run ID are required")
        job_id = str(uuid.uuid4())
        target = root / "requests" / f"{job_id}.json"
        temporary = target.with_suffix(".tmp")
        with temporary.open("x") as stream:
            json.dump(request, stream)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.rename(target)
        print(json.dumps({"job_id": job_id, "status": "queued"}))
        print("End this agent run after submission. The host waits until all work is idle.")
        return
    job_id = str(uuid.UUID(args.job_id))
    if args.action == "cancel":
        (root / "requests" / f"{job_id}.cancel").touch(exist_ok=True)
        print("Cancellation requested; only queued/waiting jobs can be cancelled.")
    else:
        path = root / "status" / f"{job_id}.json"
        print(path.read_text() if path.exists() else json.dumps({"job_id": job_id, "status": "queued"}))


if __name__ == "__main__":
    main()
