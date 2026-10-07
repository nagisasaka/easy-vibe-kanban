#!/usr/bin/env python3
"""CI-only integration fixture: run as root in a DISPOSABLE LVK container."""
import json
from pathlib import Path
import urllib.error
import urllib.request


def api(path, body=None, method=None):
    request = urllib.request.Request("http://127.0.0.1:3000/api" + path,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Content-Type": "application/json"}, method=method)
    with urllib.request.urlopen(request, timeout=20) as response:
        value = json.load(response)
        assert value["success"], value
        return value["data"], response.headers


workspace, _ = api("/workspaces", {"name": "Disposable maintenance fixture"})
session, _ = api("/sessions", {"workspace_id": workspace["id"], "executor": "CODEX"})
route = f"/sessions/{session['id']}/queue"
payload = {"message": "Never execute this fixture", "executor_config": {"executor": "CODEX"}}
gate = Path("/run/lvk-maintenance/active")
gate.parent.mkdir(parents=True, exist_ok=True)
try:
    queued, _ = api(route, payload)
    assert queued["status"] == "queued"
    gate.touch()
    status, headers = api(route)
    assert headers["x-lvk-maintenance-fence"] == "1"
    assert status["status"] == "queued", "Maintenance must preserve queued work"
    try:
        api(route, payload | {"message": "Must not replace the saved queue"})
        raise AssertionError("Queue admitted during maintenance")
    except urllib.error.HTTPError as error:
        assert error.code == 409, error.code
    status, _ = api(route)
    assert status["message"]["data"]["message"] == payload["message"]
    api(route, method="DELETE")
    assert api(route)[0]["status"] == "empty"
finally:
    gate.unlink(missing_ok=True)
    api(route, method="DELETE")
print("PASS: maintenance fences queue admission and preserves existing follow-ups")
