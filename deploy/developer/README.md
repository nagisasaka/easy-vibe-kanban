# Develop LVK using your own LVK server

This optional Linux/systemd host kit updates a developer's LVK installation
from their own GitHub repository. It is **not included in the application
image**. Normal LVK users do not need this service. The standard server image
does include headless Chromium, Playwright and its system dependencies.

## Development loop

1. Fork the repository and enable Actions. Start the official stable image with
   the server Compose kit and its optional source development service.
2. Check out your fork under `/repos`, configure Git push authentication for your
   own fork, and log in to your agent. Keep stable and development homes separate.
3. Edit source from stable LVK, then run `pnpm run server:e2e` against the
   development URL. Source changes do not require an image build.
4. Integrate the changes you want to adopt, push a new `v*` version tag and wait
   for `publish-server.yml` to succeed. Manual workflow dispatch validates but
   does not publish. Copy the published **digest**, full source SHA and run ID.
5. Submit an update, then end the requesting agent run. Check the saved job
   status from a new run after the update. A pending request does not force any
   active agent, goal, terminal, resource operation or workflow to stop.

The development service watches **one** checkout (`LVK_DEV_REPO`). It does not
automatically follow the card that requested testing. Select the intended
worktree when starting a preview, or use separate ports. Serialise use of a
shared preview with resource coordination. The updater does not switch branches,
pull source, commit files, merge cards, or automatically deploy every push.

## Install on the Docker host

Requirements: Linux Docker Engine/Compose, systemd, Python **3.11+**, Bash,
OpenSSL, GNU tar, and enough free storage for a full persistent-data backup.
The current kit supports the repository's `lvk` and `development` services,
local Docker named volumes and its standard source runner. Custom runners or
extra background processes fail closed until their owner reviews the setup.
Bootstrap a release containing this kit once through SSH before queueing
updates: older releases cannot fence their in-memory follow-up queues. The
development checkout must also contain the queue fence; a missing capability
causes the host service to wait instead of assuming an empty queue is safe.

Copy this directory from the reviewed source revision to the host. The host
installation is independent of the writable checkout: application agents
cannot replace root-executed updater code. Update this kit separately via SSH
when its host code changes.

Create `secrets/e2e.json` in the deployment directory with the existing HTTPS
Basic authentication username/password (JSON keys `username` and `password`).
Do not put it in Git or paste it into an agent conversation. The installer
restricts it to root and the container's group 10001. The application agent is
a trusted user and can read these credentials for its browser tests.

Run from this directory on the host, substituting your deployment and fork:

```bash
sudo python3 install.py \
  --deployment /opt/lvk-server \
  --repository YOUR_ACCOUNT/lucky-vibe-kanban \
  --compose-file compose.yaml \
  --compose-file compose.acme.yaml \
  --compose-file compose.development.yaml
```

Include `-f compose.updater.yaml` in subsequent Compose operations. Recreate
the application services after existing work finishes to mount the request,
read-only status, maintenance and browser-credential paths. The timer is
`lvk-updater.timer`; configuration is `/etc/lvk-updater.json`; the reviewed host
scripts are in `/opt/lvk-developer`. Preserve the existing Compose project name
and volumes. Do not run `down -v`.

The default state directory is `/var/lib/lvk-updater`. If you choose another,
also set `LVK_UPDATER_STATE` in the deployment `.env`. Public GHCR images and
public Actions metadata need no GitHub token. For private forks/rate limits,
configure host Docker's pull credentials and an optional `github_token_file`
containing a token with **Actions: read** only for the configured repository.
Do not mount the host Docker socket or this token into application containers.

## Browser E2E

From the checkout inside stable LVK (the overlay supplies the URL and credential
file):

```bash
pnpm run server:e2e
pnpm run workflow:e2e
```

`server:e2e` opens the real app, checks rendering, reload and backend responses,
and verifies anonymous HTTPS requests are rejected. It is read-only: no paid
agent run is started and no production cards are deleted. Extend these tests
for feature-specific flows using a dedicated development database.
`workflow:e2e` runs the separate UI fixtures at port 4175. Both default to bundled
Chromium; an explicit `PLAYWRIGHT_CHANNEL` selects a branded browser for the UI
fixtures if you have installed one.

For a different deployment, explicitly set `LVK_E2E_BASE_URL` and
`LVK_E2E_CREDENTIALS_FILE`. TLS validation stays enabled. Reports are under
`playwright-report/server` and `test-results/server`. These can contain private
application content. Traces are off unless `LVK_E2E_TRACE=1`.

The image installs Playwright's pinned browser revision outside mounted homes
at `/opt/lvk-browsers`. Use the repository's lockfile. Other projects with a
different Playwright revision can download their own browser without root:

```bash
export PLAYWRIGHT_BROWSERS_PATH="$HOME/.cache/ms-playwright"
pnpm exec playwright install chromium
```

The generic image's browser/system-library smoke test is also available as
`node /opt/lvk-browser/smoke.mjs` with `LVK_E2E_BASE_URL` set.

## Queue an update

For a public fork, the client resolves the source SHA, successful CI run and
GHCR digest without needing a GitHub token in the agent container:

```bash
python3 deploy/developer/client.py release \
  --repository YOUR_ACCOUNT/lucky-vibe-kanban \
  --tag v0.1.44-lvk.1-server.8
```

From the checkout inside stable LVK, substitute the three values from the
successful version-tag release:

```bash
python3 deploy/developer/client.py submit \
  --image ghcr.io/YOUR_ACCOUNT/lucky-vibe-kanban@sha256:IMAGE_DIGEST \
  --revision FULL_COMMIT_SHA \
  --run-id ACTIONS_RUN_ID
python3 deploy/developer/client.py status JOB_UUID
python3 deploy/developer/client.py cancel JOB_UUID
```

The client saves a request and returns immediately. **End that agent run**;
waiting synchronously from a running agent would prevent its own update.
Cancellation only affects queued/waiting jobs. Successful status includes the
exact request so the next agent can verify which image was installed. Running
processes are never restored magically: any further work is a new execution
using the saved session/history.

The host checks the allowed GHCR repository, digest, image source/SHA/run-ID
labels and successful version-release workflow. Publishing credentials for that
repository are a trust boundary; these checks are not a signed supply-chain
attestation. It tests the image against disposable volumes before interrupting
the installation. That test does not use the real agent's credentials or data.

## Update and recovery contract

The service waits for both applications to be idle. Active goals must finish or
be explicitly paused, scheduled tasks must be disabled, and outstanding shared
resource ownership must be resolved. Unknown database states/processes block
the update instead of being assumed safe. A terminal agent status alone is
insufficient: provider attempts, process registrations, commands and workflows
are also checked.
After closing the maintenance gate, the host waits for in-flight HTTP handlers
through a readiness barrier before freezing processes.
New mutating requests and WebSocket upgrades receive 503 while read-only
internal health/browser probes remain available. It then reads each session's
in-memory follow-up queue through an admission barrier. Existing queued
instructions prevent an update, and new queue entries are rejected until the
gate reopens. The debug application's database is in its checkout's
`dev_assets`; the release application's database is in its persistent home.

To close the race with new requests, the host briefly freezes both containers
and checks their databases and processes again. If work appeared, it unfreezes
them and waits. Otherwise it terminates the **verified idle, frozen containers**
without resuming admission. This is deliberately not a graceful application
shutdown: SQLite's WAL recovery preserves committed transactions. No active
agent may be terminated by this path. No implicit goal cancellation occurs.

With both containers stopped, the service archives all their persistent named
volumes together with deployment configuration. Only the reproducible
`development-work/lvk-target` build cache is excluded. Backups contain secrets;
only root can read them. Backups are kept, never automatically pruned. Monitor
disk usage and remove old, reviewed backups on the host as needed.

The exact image digest is applied, both services restart, and browser/backend
health checks pass before the optional maintenance gate reopens HTTPS. If
verification fails, the service verifies archive checksums and restores the
previous **image and corresponding persistent data**. Old images are never
pointed at a migrated database as a substitute for restoration.

If the host/updater is interrupted mid-transaction or rollback cannot safely
finish, the persisted job becomes `recovery_required` and blocks later updates.
The service does not guess which snapshot to destroy. Inspect it via SSH:

```bash
sudo journalctl -u lvk-updater.service -n 100
sudo cat /var/lib/lvk-updater/jobs/JOB_UUID.json
sudo python3 /opt/lvk-developer/updater.py --recover JOB_UUID
```

Recovery before a completed backup resumes the original containers without
restoring data. Recovery after application of an image restores the recorded
backup, so inspect the job before invoking it. Keep SSH as the independent
repair route. Updating the source checkout, the host kit itself, Docker, or the
host OS remains a separate operation.
