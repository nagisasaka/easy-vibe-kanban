#!/usr/bin/env python3
"""Exercise a disposable LVK server with local mock resources (no hardware/cloud).

Run inside the same host/container as the server:
  python3 scripts/test-resource-coordination.py http://127.0.0.1:39091
Use --require-mediation to require a real Codex assessment with existing auth.
The fixture creates a small temporary Git repository and retained audit records.
Never aim this fixture at an instance containing real shared resources.
"""
import argparse
import json
import pathlib
import shlex
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url")
    parser.add_argument("--require-mediation", action="store_true")
    args = parser.parse_args()

    def api(path, body=None):
        request = urllib.request.Request(args.url.rstrip("/") + "/api" + path,
            data=None if body is None else json.dumps(body).encode(),
            headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(request, timeout=35) as response:
                value = json.load(response)
        except urllib.error.HTTPError as error:
            raise AssertionError(f"{path}: HTTP {error.code}: {error.read().decode()}") from error
        assert value["success"], value
        return value["data"]

    prefix = "/resource-coordination"
    assert not api(prefix + "/snapshot")["resources"], "Use a fresh disposable LVK database"
    root = pathlib.Path(tempfile.mkdtemp(prefix="lvk-resource-smoke-"))
    repo_path = root / "repository"
    repo_path.mkdir()
    subprocess.run(["git", "init", "-b", "main", str(repo_path)], check=True, capture_output=True)
    (repo_path / "README.md").write_text("Disposable shared resource fixture\n")
    subprocess.run(["git", "-C", str(repo_path), "add", "."], check=True)
    subprocess.run(["git", "-C", str(repo_path), "-c", "user.name=LVK Test", "-c", "user.email=lvk-test@example.invalid", "commit", "-m", "Initial fixture"], check=True, capture_output=True)
    repo = api("/repos", {"path": str(repo_path), "display_name": "Resource smoke fixture"})
    sessions = []
    for name in ["Mock Card A", "Mock Card B"]:
        ws = api("/workspaces", {"name": name})
        api(f"/workspaces/{ws['id']}/repos", {"repo_id": repo["id"], "target_branch": "main"})
        sessions.append(api("/sessions", {"workspace_id": ws["id"], "executor": "CODEX", "name": name}))
    resource = api(prefix + "/resources", {"resource_key": "mock:phone", "name": "Mock phone", "description": "Exclusive local mock. busy directory must be absent after use. state file must equal the declared state.", "state": "v1"})
    (root / "state").write_text("v1\n")
    lock, trace, state = (shlex.quote(str(root / item)) for item in ["busy", "trace", "state"])

    def request(session, label, revision=1, script=None, resulting=None, verification=None, timeout=60):
        return {"request_id": str(uuid.uuid4()), "session_id": session["id"], "purpose": label,
            "claims": [{"resource_id": resource["id"], "expected_revision": revision, "resulting_state": resulting}],
            "script": script or f"mkdir {lock}\nprintf '{label}:start\\n' >> {trace}\nsleep 8\nprintf '{label}:end\\n' >> {trace}\nrmdir {lock}",
            "verification_script": verification or f"test ! -d {lock} && test \"$(cat {state})\" = {shlex.quote(resulting or 'v1')}",
            "working_dir": repo["name"], "timeout_seconds": timeout}

    def wait(operation, expected="succeeded", seconds=90):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            result = api(prefix + f"/operations/{operation['id']}?wait_seconds=25")
            if result["status"] not in ["queued", "launching", "running"]:
                assert result["status"] == expected, result
                return result
        raise AssertionError(f"Operation did not reach {expected}: {operation['id']}")

    first_request = request(sessions[0], "A")
    first = api(prefix + "/operations", first_request)
    time.sleep(2)
    second = api(prefix + "/operations", request(sessions[1], "B"))
    assert api(prefix + "/operations", first_request)["id"] == first["id"]
    wait(first)
    wait(second)
    assert (root / "trace").read_text().splitlines() == ["A:start", "A:end", "B:start", "B:end"]
    assert not api(prefix + "/snapshot")["holders"]
    print("PASS: exclusive commands, FIFO, cleanup and idempotent replay", flush=True)

    upgrade = api(prefix + "/operations", request(sessions[0], "schema upgrade", script=f"sleep 2; printf 'v2\\n' > {state}", resulting="v2"))
    stale = api(prefix + "/operations", request(sessions[1], "old schema assumption"))
    wait(upgrade)
    wait(stale, "blocked")
    api(prefix + f"/operations/{stale['id']}/cancel", {})
    print("PASS: persisted schema revision blocks old assumptions", flush=True)

    failure = api(prefix + "/operations", request(sessions[0], "verification failure", revision=2, script="printf 'finished command\\n'", verification="test 1 = 2"))
    wait(failure, "recovery_required")
    view = api(prefix + "/snapshot")
    assert len(view["holders"]) == 1 and view["resources"][0]["health"] == "recovery_required"
    recovered = api(prefix + f"/operations/{failure['id']}/recover", {"evidence": "Mock process settled; busy directory absent; state file contains v2", "claims": [{"resource_id": resource["id"], "expected_revision": 2, "resulting_state": "v2"}]})
    assert recovered["status"] == "recovered"
    print("PASS: failure retains ownership; explicit verified recovery", flush=True)

    timeout = api(prefix + "/operations", request(sessions[0], "timeout", revision=3, script="sleep 30", verification=f"test ! -d {lock}", timeout=2))
    wait(timeout, "recovery_required")
    assert api(prefix + "/snapshot")["holders"][0]["operation_id"] == timeout["id"]
    api(prefix + f"/operations/{timeout['id']}/recover", {"evidence": "Timed-out mock process terminated and no resource state changed", "claims": [{"resource_id": resource["id"], "expected_revision": 3, "resulting_state": "v2"}]})
    print("PASS: timeout terminates process without unlocking implicitly", flush=True)

    if args.require_mediation:
        deadline = time.monotonic() + 200
        while time.monotonic() < deadline:
            assessments = api(prefix + "/mediations")
            if any(m["status"] in ["applied", "stale"] for m in assessments):
                print("PASS: real Codex assessment returned a host-validated decision", flush=True)
                break
            if assessments and all(m["status"] == "failed" for m in assessments):
                raise AssertionError(assessments)
            time.sleep(2)
        else:
            raise AssertionError("Codex mediation did not produce a decision")
    print(json.dumps({"mock_root": str(root), "session_ids": [s["id"] for s in sessions], "resource_id": resource["id"]}), flush=True)


if __name__ == "__main__":
    main()
