---
title: Local execution bridge
description: Connect a Windows user-session Runner to LVK, transfer dirty source snapshots, run project scripts and recover uncertain operations.
---

# Local execution bridge

You can run a card's project on a Windows PC while LVK and its AI run on Linux.
The first version provides an HTTP API and Python CLI, not a settings screen.
Python 3.11 or later is required on the Runner; Windows 10 or later is the production
platform. Install the project's own tools separately (Python/tkinter, .NET SDK,
PowerShell, and optionally ADB). LVK does not supply build recipes or GUI automation.

## Ownership and execution

| Concept | Responsibility |
| --- | --- |
| Runner | Explicitly selected machine, scoped credential, reported OS/tools/user desktop capability |
| Resource | Existing canonical shared resource, revision, fence and exclusive owner |
| Operation | One atomic resource bundle covering start, observations, additional commands, cleanup and verification |
| Source | Immutable snapshot tied to a Workspace, base Git HEAD and SHA-256 content manifest |
| Command | Immutable UUID, durable delivery/result, sequential admission within an interactive Operation |

Register a canonical **execution slot** resource for the PC, and bind its Runner to
that resource. Claim it in every operation. This initial version serialises work on
one Runner through the existing coordinator. A desktop, microphone or Android
serial number is a separate resource: claim every resource that your scripts use
in the same operation. Capabilities do not imply ownership. Do not register a
second identity for the same physical resource to bypass an owner.

The existing resource allocator grants the entire bundle, checks revisions and
increments fences. There is no second distributed lock system, TTL release or
automatic scheduling across machines. A future Mac/ADB adapter can use the same
source, delivery and resource contracts; only the local process/observation
adapter needs to change.

Windows pulls work from LVK. No Windows inbound port is required. All commands
run as the user who launches the Runner. Run it in a logged-in, unlocked user
session for GUI work. A service in Session 0 can execute headless scripts but
cannot substitute for a user's desktop. Desktop operations are rejected when the
Runner cannot open the input desktop; screen lock/logoff can interrupt GUI tests.

The Runner is a **trusted arbitrary-code executor**, not a sandbox. Use a dedicated
user account and only enrol it in an LVK authority you trust. Project scripts must
not escape process containment through services, scheduled tasks, WMI, an existing
external daemon (including a pre-existing build server), or another machine. For effects outside the process tree, your
cleanup and verification scripts must prove the external resource is idle.

## Connect your Windows PC

1. Use the server built from this change. The existing 8443 development server
   watches another checkout and will not expose these endpoints until this code
   is integrated there. For checkout testing, use an isolated development database
   and an unused port, such as 39117; never run a second authority against the same
   real device. A debug build uses this checkout's `dev_assets`; check that this
   directory is disposable before using it for tests. Isolate temporary runtime
   files too, because LVK writes a port-discovery file under its temporary directory:

   ```bash
   OPENSSL_NO_VENDOR=1 CARGO_PROFILE_DEV_DEBUG=0 SQLX_OFFLINE=true cargo build -p server -p local-deployment --bin server --bin agent-process-host
   bridge_tmp=$(mktemp -d /tmp/lvk-bridge-runtime.XXXXXX)
   TMPDIR="$bridge_tmp" BACKEND_PORT=39117 PREVIEW_PROXY_PORT=39119 HOST=127.0.0.1 target/debug/server
   ```

   Keep the existing 8443 service unchanged. An isolated backend has its own cards
   and sessions; IDs from another LVK database cannot be used there. If you need
   the normal card UI, run `BACKEND_PORT=39117 FRONTEND_PORT=39120 pnpm run local-web:dev`
   in another host terminal and also forward port 39120 through SSH. Open
   `http://127.0.0.1:39120` to create an ordinary project/card in that backend.
   Use a small Python/.NET project for the initial transfer, not the full LVK
   repository, which exceeds the initial snapshot limit.
2. From the LVK host, inspect the existing registry:

   ```bash
   python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 resources
   ```

   Reuse the PC's canonical resource if it exists. Otherwise save a resource request
   as `windows-slot.json`, then register it:

   ```json
   {
     "resource_key": "windows:my-pc:execution",
     "name": "My Windows PC execution slot",
     "description": "One operation at a time on this PC. Project children must be stopped and external device state independently verified before release.",
     "state": "idle"
   }
   ```

   ```bash
   python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 register-resource windows-slot.json
   python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 enroll "My Windows PC" RESOURCE_UUID
   ```

   Replace `RESOURCE_UUID` with the returned resource ID. Enrolment returns `id`
   and a one-time plaintext `token`; LVK stores only its hash. Keep this output
   private. Save the Runner ID and resource ID. Enrolment is an operator action
   through the existing LVK control API, not an unauthenticated public pairing flow.
3. Copy `scripts/local-runner/runner.py` and `windows_job.py` from this checkout to
   `C:\lvk-runner`. The server Docker image also ships these files and `control.py`
   under `/usr/local/share/lvk/local-runner/`; you can copy them out with `docker cp`
   and run the installed control CLI there without checking out LVK source. Do not put credentials in the project or commit them.
4. Connect to your authenticated HTTPS LVK address. For an unpublished development
   endpoint, use an outbound SSH tunnel from Windows instead:

   ```powershell
   ssh -N -L 39118:127.0.0.1:39117 your-user@your-lvk-server
   ```

   Keep the tunnel open. In a second PowerShell window:

   ```powershell
   cd C:\lvk-runner
   $env:LVK_RUNNER_TOKEN = [System.Net.NetworkCredential]::new('', (Read-Host "Runner token" -AsSecureString)).Password
   py -3 runner.py --url http://127.0.0.1:39118 --runner RUNNER_UUID --root C:\lvk-runner-data
   ```

   Replace `RUNNER_UUID`. The token travels in `X-LVK-Runner-Token`. If your HTTPS
   reverse proxy requires Basic authentication, set `LVK_HTTP_AUTH` to `user:password`
   in that window too; this is separate from the Runner credential. TLS verification
   is enabled; there is no insecure certificate override. HTTP is accepted only
   on loopback, and redirects are rejected to avoid credential forwarding.
5. Confirm the connection from the host:

   ```bash
   python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 runners
   ```

   Expect a recent `last_seen`, `os: "Windows"`, `protocol: 1`, available tool paths,
   and `desktop: true` when running on an accessible user desktop. A heartbeat is
   connection evidence, not proof that any prior operation stopped.

## Capture and run a card

Use an interactive Workspace's Session UUID. From its host context, capture the
repository by its directory name relative to the multi-repository Workspace root:

```bash
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 capture SESSION_UUID REPOSITORY_DIRECTORY
```

For a direct-folder Workspace whose root is the repository, use `.`. Capture reads
tracked files and non-ignored untracked files, including staged/unstaged edits and
tracked deletions. It does not commit or modify the source. It reads twice to reject
ordinary edits racing with capture. The returned `id`, `head`, `digest` and
`file_count` identify the actual transferred snapshot. The digest is SHA-256 over
the sorted entries `UTF8(path) + NUL + ASCII(file_sha256) + LF`.

The initial transport is limited to 10,000 files and 16 MiB of decoded source.
Symlinks, submodules, case-colliding names and unsupported Windows paths are
rejected. Git-ignored environments/build outputs are omitted; install dependencies
on the Runner through your scripts. Deliberately tracked secrets would be copied,
so review what Git tracks. Snapshot capture is one repository at a time. Git administrative data is not
transferred; project versioning scripts can use `LVK_SOURCE_HEAD` (base commit),
`LVK_SOURCE_ID` and `LVK_SOURCE_DIGEST` (the exact dirty snapshot).

Save this request as `run.json`, replacing the UUIDs and current resource revision.
Use a newly generated UUID for `request_id` and retain the file unchanged for retries:

```json
{
  "request_id": "OPERATION_UUID",
  "session_id": "SESSION_UUID",
  "purpose": "Windows Python and .NET toolchain smoke test",
  "claims": [{"resource_id": "RESOURCE_UUID", "expected_revision": 1, "resulting_state": null}],
  "script": "python --version; if ($LASTEXITCODE) { exit $LASTEXITCODE }; dotnet --info; if ($LASTEXITCODE) { exit $LASTEXITCODE }; 'runner-ok' | Set-Content (Join-Path $env:LVK_ARTIFACT_DIR 'result.txt')",
  "verification_script": "if (-not (Test-Path (Join-Path $env:LVK_ARTIFACT_DIR 'result.txt'))) { throw 'Missing result' }; if ((Get-Content (Join-Path $env:LVK_ARTIFACT_DIR 'result.txt')).Trim() -ne 'runner-ok') { throw 'Invalid result' }",
  "working_dir": ".",
  "timeout_seconds": 120,
  "runner": {
    "runner_id": "RUNNER_UUID",
    "source_id": "SOURCE_UUID",
    "interactive": false,
    "desktop": false,
    "cleanup_script": "Get-Location | Out-Null"
  }
}
```

Here no long-lived app or external device is used. For real applications, replace
the cleanup/verification scripts with actual resource state checks. `working_dir`
for a Runner is relative to the captured repository, not to the server Workspace.
PowerShell non-terminating errors are promoted to errors; explicitly check
`$LASTEXITCODE` after **each** native command in multi-command scripts.

```bash
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 submit run.json
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 status OPERATION_UUID
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 commands OPERATION_UUID
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 download OPERATION_UUID ./windows-results
```

`status` waits up to 25 seconds. A queued response is not completion. Download uses
a new directory and separates artifacts by command UUID. Each result has its
runtime/command identity, success, containment settlement, log and SHA-256 artifacts.
Read the operation to find its source ID, then use `control.py source SOURCE_UUID`
to retrieve the retained HEAD, manifest digest and per-file hashes.
Project scripts write images/reports to `$env:LVK_ARTIFACT_DIR`. Limits are 1 MiB of
log and 16 MiB of artifacts per command. Do not write credentials to logs/artifacts.

## Launch, observe, act, finish

Set `runner.interactive=true` to keep an operation open after the initial command.
Set `runner.desktop=true` for GUI work and add the canonical desktop resource to
`claims` before submitting. All needed resources must be claimed up front.

Your initial script can build the project and use `Start-Process -PassThru` to
launch it, saving its PID in the operation directory. The process stays in the
operation's Windows Job Object while the initial PowerShell command returns. Do
not use `-Wait` if you want subsequent commands to inspect that app.

For example, a tkinter project can start with:

```powershell
$p = Start-Process python -ArgumentList 'app.py' -PassThru
$p.Id | Set-Content .lvk-app.pid
```

Supply immutable cleanup and verification scripts when submitting the operation.
Add your project's external-state cleanup where needed; the Runner itself stops
the contained process tree without relying on a possibly reused PID:

```powershell
# cleanup_script: this example has no external state to restore.
# The Runner terminates its Job Object members after this script returns.
Get-Location | Out-Null
```

```powershell
# verification_script (after the Runner terminates remaining job members)
if (Test-Path .lvk-app.pid) {
    if (Get-Process -Id ([int](Get-Content .lvk-app.pid)) -ErrorAction SilentlyContinue) {
        throw 'Application is still running'
    }
}
```

Use your project's UI test tools, UI Automation scripts or screenshot script for
observation. A command that captures the desktop into the artifact directory can
be saved as `observe.ps1`:

```powershell
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bitmap.Size)
    $bitmap.Save((Join-Path $env:LVK_ARTIFACT_DIR 'desktop.png'))
} finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}
```

Review the screenshot's scope: it includes the user's desktop. Then submit a step
from the LVK host and retrieve its results. Use a stable UUID per step:

```bash
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 step OPERATION_UUID observe.ps1 --request-id STEP_UUID
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 commands OPERATION_UUID
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 finish OPERATION_UUID --request-id FINISH_UUID
```

Each command starts a fresh PowerShell process in the same operation directory
and Job Object. Keep cross-command app state in project files or app IPC, rather
than PowerShell variables. Wait for each command's `status: "done"` before
submitting the next. Finish closes
command admission permanently and runs the original cleanup and verification.
The total operation deadline continues across observations and AI turns; it is not
extended by heartbeats or step submission. Use a new operation and source snapshot
for edited source. LVK never overwrites a running operation's source directory.

For .NET use the same flow, replacing the project scripts with `dotnet build`,
`dotnet test`, and `Start-Process` on the built executable. For ADB, report the tool
as a capability and claim the canonical device resource as well as the execution
slot. This version does not claim to contain a pre-existing ADB daemon or device
process: explicitly stop and verify those external effects in project scripts.

## Cancellation and uncertain outcomes

```bash
python3 scripts/local-runner/control.py --url http://127.0.0.1:39117 cancel OPERATION_UUID
```

A running cancellation requests cleanup and retains the entire resource bundle.
A deadline, failed command/verifier, or LVK restart likewise produces
`recovery_required`. The remote machine may still be working when the network
fails. Reconnect the **same Runner with the same data root**. Do not delete its
journal or enrol a replacement identity to get around retained ownership.

The local journal records command admission before process creation. Duplicate
messages return the saved result. A command admitted before a crash with no saved
result is never automatically re-executed. On Windows, reconnection terminates and
inspects the operation's globally named Job Object before reporting containment
settlement, including across Windows session changes. An access failure cannot
be treated as an empty job.
The first authenticated poll binds the Runner to an installation UUID persisted
in that data root. A second, fresh root cannot execute the same Runner identity.
The root also has an OS file lock against two processes; these delivery safeguards
do not replace resource coordination. Keep the Runner ID/token/root private and do
not clone the root onto another machine.

After the Runner reports settlement, the operator must inspect the external
resources and submit the existing `/api/resource-coordination/operations/ID/recover`
request with evidence and **every** held resource's current revision and verified
state. Remote recovery refuses release without Runner settlement. Late completion
cannot convert `recovery_required` into success. A lost Windows installation or
lost journal needs an explicit future recovery mechanism; v1 intentionally has no
force-unlock endpoint that can claim an unreachable Runner is stopped.

`POST /api/execution-bridge/runners/ID/disable` prevents new operations but allows
that credential to deliver remaining cleanup/results. It does not terminate work
or release resources. If an enrolment reply or token is lost, an operator can use
`control.py rotate-token RUNNER_UUID` to obtain a new token and revoke the old one.
Stop the original Runner and reconnect with the new token and the same root;
rotation preserves installation identity and never releases an operation. No
automatic artifact retention policy is included. Preserve the database and Runner root while outcomes are unresolved.

## API summary

All routes return LVK's `{success,data,message}` envelope. Control routes use the
existing LVK access boundary. Runner poll/report/settled routes additionally require
the enrolled ID, `X-LVK-Runner-Token` and the bound
`X-LVK-Runner-Installation` identity. Keep the LVK control API behind its normal
authenticated reverse proxy or loopback SSH tunnel.

| Route | Purpose |
| --- | --- |
| `GET/POST /api/execution-bridge/runners` | List capabilities / enrol against an existing execution resource |
| `POST /api/execution-bridge/runners/ID/poll` | Authenticated outbound pull; capabilities and optional known operation IDs |
| `POST /api/execution-bridge/sources` | Capture `{session_id,working_dir}` |
| `GET /api/execution-bridge/sources/ID` | Retrieve immutable HEAD, digest and per-file hashes |
| `POST /api/resource-coordination/operations` | Existing operation spec plus optional `runner` target |
| `GET/POST /api/execution-bridge/operations/ID/commands` | Inspect results / enqueue `{id,kind:"step"|"finish",script}` |
| `POST /api/execution-bridge/runners/R/operations/O/report` | Durable command receipt, scoped to Runner and dispatch runtime |
| `POST /api/execution-bridge/runners/R/operations/O/settled` | Evidence that no operation process can execute further work |

Windows creates commands directly inside a Job Object with
`PROC_THREAD_ATTRIBUTE_JOB_LIST`, forbids normal breakaway and uses
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. It waits for zero active members before
settlement; verification children are also terminated. See Microsoft's
[Job Objects documentation](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
and [kernel object namespaces](https://learn.microsoft.com/windows/win32/termserv/kernel-object-namespaces).
External effects still require your independent verifier.

## Validation boundaries

The Linux `--simulate` adapter is for disposable tests only. It uses process groups
and Bash; it is not a supported production Linux or Mac Runner and deliberately
cannot claim recovery of an unknown process group after a simulator restart.
Never aim the integration fixture at an instance containing real shared resources.

Run the isolated checks with:

```bash
python3 -m unittest discover -s scripts/local-runner -p 'test_*.py' -v
OPENSSL_NO_VENDOR=1 SQLX_OFFLINE=true cargo test -p db resource_coordination
OPENSSL_NO_VENDOR=1 SQLX_OFFLINE=true cargo test -p server --lib execution_bridge
```

`OPENSSL_NO_VENDOR=1` uses the development host's installed OpenSSL headers and
libraries. Omit it when those are unavailable. For an isolated server smoke test,
use `python3 scripts/test-execution-bridge.py http://127.0.0.1:39117` against a fresh
database. It creates a fixture card and mock resource, launches the actual outbound
simulator process, checks artifacts and idempotent steps, and verifies final release.

Validated on the Linux development host: 11 Runner/process/transport tests,
11 resource-coordination tests, 7 bridge/source tests, and the isolated real-server
smoke flow above. The control CLI also downloaded and verified artifacts from all
three commands. The smoke server was stopped with no queued or held operations.
Server/MCP compilation, generated types, formatting and Docker-free server checks
also passed (the latter: 19 passed, 4 skipped). No Windows hardware or Docker image
execution was used for these results.

Windows-only confirmation remains necessary: connection through your actual HTTPS
proxy/tunnel, PowerShell encoding, Job Object creation and nested process cleanup,
user-session versus service execution, desktop locking, tkinter/.NET GUI launch,
screenshot capture, and reconnect after forcibly terminating the Runner. The
Windows examples above are concrete manual acceptance steps, not a claim that
Windows hardware was tested on the Linux development host.

Canonical OpenWiki and the retired `.llm-wiki` pipeline are unchanged. This normal
source-development card records its design and usage here; Wiki publication is a
separate authorised repository-maintenance operation.
