---
title: "OpenWiki repository memory implementation checkpoints"
description: "Architecture mapping, implementation decisions and verification for repository memory."
---

# OpenWiki repository memory implementation

The normative contract is `openwiki-repository-memory.md`, read in full before
implementation. This document records implementation and verification, not a
replacement specification.

## Architecture mapping

| Responsibility                          | Existing owner / extension point                                                                                     |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Repository identity and source path     | `db::models::repo::Repo`                                                                                             |
| Workspace and repository membership     | `Workspace`, `WorkspaceRepo`, `Session`                                                                              |
| Worktree provisioning                   | `WorkspaceManager`, `WorktreeManager`                                                                                |
| Repository shared storage               | `workspace_manager::shared_resources`, `utils::path::shared_resources_dir`                                           |
| Canonical agent launch and frozen input | server `sessions/agent_run.rs`, `LocalAgentRunPort::execution_env`                                                   |
| Codex host authentication and MCP       | existing Codex executor / app-server, upstream OpenWiki user integration                                             |
| Agent completion                        | `LocalContainerService::handle_agent_run_terminal`, AgentRun terminal events                                         |
| Existing automatic commits              | `try_commit_changes` handles CleanupScript only; canonical agents do not enter that path                             |
| Commit text                             | cleanup message, merge label and agent-authored commits; no standalone semantic-summary service found                |
| Source integration                      | `routes/workspaces/git.rs::merge_workspace`, `GitService::merge_changes` (squash; original ancestry is insufficient) |
| Remote PR observation                   | `PrMonitorService`, distinct from locally available integrated source                                                |
| Repository UI                           | existing repository settings and `/api/repos` routes                                                                 |

## Implementation decisions

- Use `persistent/knowledge` underneath the existing repository shared root.
  Event and workspace IDs come from EVK, never from model-generated paths.
- Keep immutable semantic events separate from retryable receipts and durable
  reconciliation state. Failed receipts do not acknowledge events.
- Extend the current executor context and completion/commit flow; do not add a
  separate model client for summaries. Semantic fields originate in the current
  coding session; Git supplies revisions and changed paths.
- OpenWiki 0.5.1 supports user-level `integrations install codex` and the public
  MCP sequence begin / submit_plan / next_page / submit_page / finish. Its
  bundled host integration requires sequential page work. EVK does not supply a
  competing page scheduler or modify upstream source.
- Normal source work and manifest writes stay parallel. Only integrated Wiki
  publication is serialised. Maintenance must validate finalisation and source
  revision before acknowledging events; process exit alone is insufficient.
- Preserve the existing `.llm-wiki` feature and user edits; do not silently
  translate or delete its files. Canonical OpenWiki is separately named
  `openwiki/` and obeys the new single-writer policy.

## Checkpoints

1. [x] Read specification, all repository AGENTS files, inspect architecture.
2. [x] Pinned dependency and public adapter compatibility (image run pending daemon).
3. [x] Manifest, atomic outbox, receipts, memory and repository lock.
4. [x] Runtime, commit and integrated-source lifecycle connections.
5. [x] Host-driven bootstrap/reconciliation, recovery and status UI (real-Codex smoke pending).
6. [x] Complete-diff/specification review, local regression suites and strict Clippy; external checks are explicitly deferred under the user's sandbox validation policy.

## Environment and baseline

- Existing uncommitted changes: `wikiBootstrap.ts` and its test. User supplied
  the untracked specification and a Windows Zone.Identifier file. Preserve all.
- npm registry resolves `openwiki@0.5.1` (Node >=22). Its package was inspected
  outside the repository; no upstream code is vendored.
- Docker CLI is installed, but `/var/run/docker.sock` does not exist. Built-image
  runtime verification needs a Docker daemon. This does not block implementation
  or fixture-based tests.
- Release/publish/push operations are not part of this implementation.

## Verification (13 September 2026)

| Check                                                                       | Result                                                                                                                                           |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `cargo test --workspace --no-fail-fast`                                     | 789 passed, 0 failed, 3 fixture/illustrative entries ignored; exit zero on the final complete implementation                                     |
| Services library tests                                                      | 50 passed, including 12 memory-lifecycle and 7 OpenWiki adapter/publication cases                                                                |
| Repository-memory storage/locking tests                                     | 5 passed, including explicit unlock with an inherited open file description                                                                      |
| Services stress test before the final extra regression                      | 49 passed each time in three runs with eight threads after the lock fix                                                                          |
| Codex memory initial/resume and control-command tests                       | Passed                                                                                                                                           |
| Explicit compaction stdio test                                              | Passed: checkpoint → compact → reread, no paid model                                                                                             |
| Full web-core Vitest, `CI=true`                                             | 53 files, 297 tests passed                                                                                                                       |
| `env -u CI pnpm run workflow:e2e`                                           | 18 browser tests passed using the installed Chrome channel; no paid models                                                                       |
| `pnpm run generate-types`                                                   | Passed; Rust remains the source of truth                                                                                                         |
| Local frontend build                                                        | Passed; existing chunk-size/Browserslist/Tailwind warnings                                                                                       |
| Remote frontend build                                                       | Passed; browser bundle build is independent of private Remote Rust dependencies                                                                  |
| `pnpm run check:npx-cli`                                                    | Passed after installing the existing locked NPX CLI dependencies with scripts disabled                                                           |
| Local frontend, remote frontend, shared web-core/UI TypeScript checks       | Passed                                                                                                                                           |
| `cargo check --workspace`                                                   | Passed                                                                                                                                           |
| `pnpm run check`                                                            | Locally executable checks pass; Remote Rust is deferred, failing before compilation on the private `billing` dependency                          |
| Remote `cargo test --manifest-path crates/remote/Cargo.toml --no-fail-fast` | Deferred: dependency access fails before compilation                                                                                             |
| `pnpm run lint`                                                             | Frontend/UI and strict local Rust Clippy pass; Remote Rust is deferred on the private dependency                                                 |
| Scoped lint of new settings component and machine API                       | Passed using the existing local-web ESLint configuration                                                                                         |
| Real pinned npm package, public CLI/MCP smoke                               | Passed; installation/reinstallation, init, rejected premature finish, plan/page/Claims/finish, update/no-op, instruction and source preservation |
| Docker image runtime                                                        | Unverified: Docker socket/daemon unavailable                                                                                                     |
| Manual real-Codex run through EVK UI                                        | Documented, not executed; automated checks make no paid model calls                                                                              |
| Local development API through `localhost:4020/api/info`                     | HTTP 200 after final validation                                                                                                                  |

The user's updated completion policy treats unavailable Docker-daemon and private
billing checks as **deferred external verification, not implementation blockers**.
Implementation acceptance uses the completed specification mapping, passing
locally executable checks, no known unresolved implementation defect, and the
reproducible external checks below. The aggregate `pnpm run check` and `pnpm run
lint` commands still exit non-zero at Remote dependency resolution: they are not
reported as passing in full. Neither Docker image execution nor a paid real-Codex
run is claimed to have passed.

The three ignored Rust entries are two subprocess fixture entry points (executed
by their parent tests) and an existing illustrative `stderr_processor` doctest,
not skipped executable repository-memory tests. The browser suite initially
could not find CI Chromium; its download failed at TLS setup. The already
installed Chrome channel ran all 18 tests successfully without changing the
test configuration or assertions.

## Compatibility findings and bounded baseline fixes

- Upstream 0.5.1 does **not** support `--version` (even returning success for the
  unknown flag). Compatibility therefore reads its public `--help` version banner.
- Upstream init writes an optional native-provider Actions workflow and managed
  agent-instruction blocks. EVK excludes the Actions workflow and installation
  artifacts from publication; it validates preservation of user text around blocks.
- Baseline Workflow tests expected legacy process identities and mutable run
  graphs. Fixtures now provide canonical identities, preserve immutable graph
  snapshots, and verify that editing a template affects a new run, not retry.
  Runtime behaviour was not weakened to satisfy the old assertions.
- Fixed the optional-translator English fallback leaving raw interpolation
  markers, restored a missing diff-adapter snapshot, supplied missing QA mock
  fields, and removed unused Claude bindings. These small baseline fixes permit
  the existing full unit suites to run. They are separate from the memory design.
- Strict local Clippy now passes without weakening the gate. Bounded helper
  signatures, named row types, explicit boxing of large enum variants, test
  placement and feature/platform-scoped imports remove baseline findings.
  Native Audit's constructor now attaches optional metadata through a builder;
  its bytes, schema, checksums and replay contract are unchanged.
- Removed the unreachable in-process launch/reader/monitor implementation from
  `agent_run_port.rs` (its launch was already commented out). The live Process
  Host path, cancellation ownership and Native Audit writer remain. Test-only
  transport fixtures are compiled only for tests; the cancellation failure test
  now checks actual persisted process ownership instead of an unused token map.
  These removals remain recoverable from Git history and the uncommitted diff.
- Removed ten confirmed-unused translation keys (and their locale copies), after
  checking static and dynamic frontend consumers. The existing unused-key gate
  now passes; no translated visible label was removed.
- Source completion now has an immutable pre-commit checkpoint, expected Git
  tree and `EVK-Memory-Source` identity. A per-workspace lock prevents duplicate
  finalisation without serialising independent worktrees. Recovery tests cover
  crashes both before and after source commit, later edits and draft changes.
  Before a later coding launch, successful prior runs are finalised/recovered;
  a missing semantic draft reports its path and does not absorb the new task.
- Parallel Git subprocesses exposed an inherited-file-descriptor lock lifetime
  race. The shared lock guard now explicitly unlocks on drop rather than relying
  on the final descriptor closing. A deterministic duplicated-descriptor test
  reproduces the distinction, and the final full suite passes with this fix.
- The final review rejected timestamp-based PR membership: an unpushed later
  change in the same workspace must not be acknowledged with an earlier PR.
  External PRs now use verified source ancestry as a conservative fallback,
  including delayed events. EVK direct squash integrations retain explicit IDs.
  External squash/rebase PRs without retained merged-head/event metadata require
  explicit source-only Sync; unverified manifests remain unconsumed. A future
  extension can persist pushed-head/event sets and provider merged-head identity
  through the existing PR monitor, without guessing from timestamps.
- The completion audit strengthened the compact/reread instruction to restore
  repository instructions and read-only OpenWiki entry points as well as Workspace
  Memory. The stdio fixture asserts all three. An additional real-Git regression
  follows A source → A Wiki → B rebase/source → B Wiki, proving that B sees the
  latest canonical Wiki and explicit event IDs survive rewritten source ancestry.

## Complete-diff and specification audit

- Reviewed the runtime and API wiring, all new memory/adapter/storage code, Git
  changes, settings UI, generated declarations, tests, Docker inputs, documentation
  and the bounded baseline lint fixes. The original bootstrap-prompt edits and
  user-supplied specification remain untouched. No generated TypeScript was
  hand-edited and no database migration is needed. Static review caught and
  synchronised the Remote lockfile's `utils → tempfile` edge to its already
  locked `tempfile@3.27.0`; no package version, checksum or private dependency
  identity changed. This one-line lockfile synchronisation is explicit because
  full Remote lock regeneration cannot resolve the inaccessible billing source.
- Searched the new feature paths for TODO/FIXME, `todo!`, `unimplemented!`, stub
  implementations and ignored tests. No incomplete feature path or placeholder
  implementation remains. The language-input placeholder is a UI example only.
- Section 20 is implemented with explicit event sets for EVK-owned source
  integrations, including squash. The permitted ancestry fallback applies only
  to external PR observations lacking explicit membership. Unknown external
  squash/cherry-pick events are not falsely acknowledged; source-only manual Sync
  remains available. This conservative boundary is documented, not silently
  labelled automatic external-PR coverage.
- Maintenance uses existing Workspace/Session/AgentRun ownership, shared-folder
  paths, Codex authentication, selected Skills and Git integration. No second
  model API client, scheduler, event database, raw-transcript copier or upstream
  OpenWiki source is introduced. Existing `.llm-wiki` behaviour remains separate.
- Static Docker review confirms Node 22 runtime, the single exact pin shared
  with the Rust adapter, telemetry disabled, public-help health validation, both
  `server` and `agent-process-host` binaries, frontend patch inputs, all local
  Cargo workspace members and their SQLx metadata/assets. The private Remote
  workspace is excluded by the root Cargo workspace; the image's local server
  build does not acquire billing through a newly added dependency. No Codex or
  model API credentials are supplied as Docker build arguments or copied into
  the runtime image. Actual image build/runtime compatibility remains deferred.

## Acceptance mapping

| Section 42 criteria | Implementation / evidence                                                                                                                                                |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1–3                 | Single exact npm pin, Docker Node runtime, supported Codex installer and MCP adapter; image execution and real-Codex smoke remain unverified                             |
| 4–5                 | Allowlisted Git publication; normal-workspace Wiki guard at completion, merge, Push and PR creation, including renames and direct-folder commits                         |
| 6–8                 | Existing coding host authors semantic draft; common completion projects commit text, derives Git fields and atomically publishes UUID event                              |
| 9–13                | Two real worktrees, independent events, explicit squash integration IDs, repository maintenance owner and short integration lock; source drift rejects stale publication |
| 14–15               | Durable receipts, failure/no-op retries, validated publication journal, parent/tree-checked replay, status based on actual integrated revision                           |
| 16–18               | Workspace-local semantic Markdown, persistent developer instructions, incremental checkpoints and explicit compaction test; no transcript copier                         |
| 19–20               | Existing worktrees and Design Arena policy preserved; integration/rebase, failure, symlink, no-op and restart-window tests                                               |
| 21                  | All locally executable gates pass; private Remote checks are explicitly deferred under the updated user policy, not claimed passed                                       |
| 22                  | User lifecycle/recovery/upgrade/troubleshooting guide and model-free/manual smoke paths                                                                                  |

## Deferred external verification — reproducible hand-off

Run these commands from the repository root in an appropriately provisioned
environment. They are verification instructions, not checks executed here.
Do not release, publish, push or create a PR as part of these checks.

### Private Remote dependency and aggregate quality gates

Prerequisites: the repository's Rust toolchain, pnpm dependencies, an SSH client
and authorised GitHub access to the existing private dependency (or an authorised
Cargo cache containing its locked revision). Preserve Remote's existing billing
feature gates and dependency declarations; do not substitute a stub.

```bash
git ls-remote ssh://git@github.com/BloopAI/vibe-kanban-private HEAD
cargo fetch --locked --manifest-path crates/remote/Cargo.toml
pnpm run check
pnpm run lint
cargo test --manifest-path crates/remote/Cargo.toml --no-fail-fast
```

Expected result: dependency resolution includes revision
`020091913ce5608d6c8bdb667d124fd22ab10561`, Remote compiles, both aggregate commands
exit zero, and Remote tests pass. The checked-in SQLx metadata supplies offline
queries; follow `crates/remote/AGENTS.md` if that authorised environment needs a
Postgres-backed metadata refresh. Do not alter metadata merely to bypass a failure.

Current evidence: Cargo fails before Remote compilation with `cannot run ssh`
and the unavailable private revision. The independent read-only command
`env GIT_TERMINAL_PROMPT=0 git ls-remote https://github.com/BloopAI/vibe-kanban-private HEAD`
exits 128 because credentials cannot be requested. There is no Remote compiler or
test finding to diagnose in this environment. Remote manifests and feature-gated
billing code are unchanged. Its lockfile only gains the `utils → tempfile` edge
described above; the existing locked package and private billing revision are
unchanged. This is evidence of the observed access failure, not proof that an
unexecuted Remote compile passes.

### Docker image and public OpenWiki host integration

Prerequisites: Docker daemon with BuildKit, permission to use it, network access
to the declared base images, Rust/public npm dependencies, and sufficient image
build resources. Run the image build in the external validation environment;
do not create release packages on this sandbox machine.

```bash
docker info --format '{{.ServerVersion}}'
docker build --target runtime --tag evk-openwiki:validation .
docker run --rm --entrypoint /bin/sh \
  --mount "type=bind,src=$PWD/scripts/test-openwiki-host.mjs,dst=/validation/scripts/test-openwiki-host.mjs,readonly" \
  --mount "type=bind,src=$PWD/assets/openwiki-version,dst=/validation/assets/openwiki-version,readonly" \
  evk-openwiki:validation -ec '
    node -e "if (Number(process.versions.node.split(\".\")[0]) < 22) process.exit(1)"
    test "$OPENWIKI_TELEMETRY_DISABLED" = 1
    command -v server agent-process-host openwiki
    node /validation/scripts/test-openwiki-host.mjs
  '
```

Expected result: the image builds; all commands run successfully as its default
non-root user; Node is at least 22, telemetry is disabled, both EVK binaries are
installed, and the smoke test reports `PASS: OpenWiki 0.5.1`. The smoke test uses
only the upstream public CLI/MCP, installs/reinstalls the project integration in
a disposable Git repository, validates init/finalisation and update/no-op, and
makes no model call. No authentication material is baked into the image.

Current evidence: `docker info` exits 1 because `/var/run/docker.sock` does not
exist. No image build was attempted or reported successful. The same pinned
public npm CLI/MCP smoke passes on this host, including validation rejection and
user-instruction/source preservation. Static Docker input and dependency review
found no remaining wiring defect; that does not establish runtime image success.

### Optional real-Codex end-to-end smoke

Prerequisites: a running EVK development or verified Docker instance, the pinned
OpenWiki CLI, existing authorised Codex host authentication, and permission to
consume model tokens. Configure credentials only at runtime. No additional OpenAI
API key is required. Use a disposable Git repository, not an active user project.

1. Add the test repository in EVK, enable repository memory and select its local
   `main` branch and language. Select **Initialize Wiki**.
2. Inspect the dedicated maintenance session for successful public OpenWiki
   begin/plan/page/Claims/finish calls. Confirm **Current** and a separate
   Git-tracked `openwiki/` commit on `main`.
3. Create two ordinary workspaces, A and B, and request independent semantic
   source changes. Confirm unchanged `openwiki/`, distinct immutable shared events
   and per-workspace memory files; commit text uses each semantic summary.
4. Integrate A through EVK's merge UI. Confirm the source commit precedes its Wiki
   commit, the receipt names A's event, and status becomes **Current**.
5. Rebase B onto the new `main`, then integrate it. Confirm B receives A's Wiki
   snapshot and the next maintenance run reads the final A+B source state.
6. Stop a maintenance AgentRun using its ordinary audited Stop control. Confirm
   source remains integrated, status is **Error**, and events remain pending.
   Use **Sync Wiki** to retry and verify successful receipts.
7. Run `/compact` in a normal long-lived session. Confirm checkpoint → compact →
   reread and preservation of the workspace's semantic decisions. Start another
   normal workspace and verify it reads the updated canonical Wiki.

This paid end-to-end check is documented but not executed. Deterministic Git,
stdio, adapter and public MCP tests cover the corresponding contracts locally;
they are not represented as a real authenticated model session.

No schema migration is required: new version-1 records occupy a namespace under
the existing shared-folder abstraction. No existing AgentRun event or Wiki data
is deleted. Keep audit streams and failed maintenance workspaces for diagnosis.
External Git operations are not serialised by EVK's lock: source revision checks
fail closed, and unknown squash/cherry-pick membership is not guessed.

## Final local evidence

The final completion audit records stdout/stderr in these sandbox-local files.
They are not committed artifacts or prerequisites for reproducing the tests:

- `/tmp/evk-memory-tests-final-acceptance.log`: full Rust suites, 789 passed.
- `/tmp/evk-memory-sequential-wiki-test.log`: successive A/B Wiki publications.
- `/tmp/evk-memory-vitest-completion-audit.log`: 53 files, 297 passed.
- `/tmp/evk-memory-workflow-e2e-chrome.log`: all 18 browser tests passed.
- `/tmp/evk-memory-public-smoke-completion-audit.log`: real upstream public MCP,
  no paid model calls.
- `/tmp/evk-memory-types-completion-audit.log`: generated types are current.
- `/tmp/evk-memory-build-completion-audit.log`: local frontend build passed.
- `/tmp/evk-memory-remote-web-build-completion.log`: Remote frontend build passed.
- `/tmp/evk-memory-npx-check-final.log`: NPX CLI typecheck passed. Its initially
  missing `cac` dependency was installed from the existing NPX lockfile with
  `npm ci --prefix npx-cli --ignore-scripts`; no manifest or lockfile change.
- `/tmp/evk-memory-clippy-final-acceptance.log`: standalone strict local Clippy
  also exits zero, independently of the aggregate Remote access failure.
- `/tmp/evk-memory-check-completion-audit.log` and
  `/tmp/evk-memory-lint-completion-audit.log`: local stages passed; Remote stopped
  at private dependency resolution, aggregate exit 101.
- `/tmp/evk-memory-remote-tests-completion-audit.log`: the same pre-compilation
  Remote dependency failure, not an executed test failure.
- `/tmp/evk-memory-i18n-completion-audit.log`: unused-translation check passed.

The complete diff is whitespace-clean. No release/publish/push/PR operation was
performed; no existing user Wiki, source change or uncommitted prompt edit was
discarded. Failed fixture logs stay outside Git, and no private credentials or
vendored OpenWiki package are added to the worktree.
