#!/usr/bin/env python3
"""CI-only integration fixture: run as root in a DISPOSABLE LVK container."""
import json
from pathlib import Path
import sqlite3
import urllib.error
import urllib.request
import uuid


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
# This fixture runs only in the disposable smoke-test container. Model auth is
# deliberately absent: seed a run identity to exercise queue admission without
# starting a provider or allowing the queued prompt to execute.
database = Path("/home/appuser/.local/share/vibe-kanban/db.v2.sqlite")
run_id = uuid.uuid4().bytes
try:
    try:
        api(route, payload)
        raise AssertionError("An idle session accepted a new queue")
    except urllib.error.HTTPError as error:
        assert error.code == 409, error.code
    with sqlite3.connect(database) as db:
        db.execute("PRAGMA foreign_keys = ON")
        db.execute("""INSERT INTO agent_runs (
            id, session_id, workspace_id, request_id, idempotency_key,
            correlation_id, schema_version, payload_version, runtime_profile_id,
            provider_id, workspace_mode, workspace_path, status, request_envelope
        ) VALUES (?, ?, ?, ?, ?, ?, 1, 1, 'CODEX', 'codex',
                  'shared_workspace', '/repos', 'running', '{}')""",
            (run_id, uuid.UUID(session["id"]).bytes,
             uuid.UUID(workspace["id"]).bytes, uuid.uuid4().bytes,
             str(uuid.uuid4()), uuid.uuid4().bytes))
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
        assert error.code == 503, error.code
    status, _ = api(route)
    assert status["message"]["data"]["message"] == payload["message"]
    gate.unlink()
    api(route, method="DELETE")
    assert api(route)[0]["status"] == "empty"
finally:
    gate.unlink(missing_ok=True)
    try:
        api(route, method="DELETE")
    finally:
        with sqlite3.connect(database) as db:
            db.execute("DELETE FROM agent_runs WHERE id = ?", (run_id,))
print("PASS: maintenance fences queue admission and preserves existing follow-ups")
