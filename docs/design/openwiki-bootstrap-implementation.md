---
title: OpenWiki bootstrap implementation record
description: Architecture mapping, compatibility decisions and verification for the repository-scoped bootstrap workflow.
---

# OpenWiki bootstrap implementation record

Normative specification: [Bootstrap v2](openwiki-bootstrap-workflow-v2.md).

## Existing architecture

- `server/routes/openwiki.rs` reserves a dedicated ordinary Workspace, binds one repository and the integrated target branch, prepares the public OpenWiki Codex integration, then reserves and launches one AgentRun. The existing repository recovery monitor verifies Native Audit, restores instructions and invokes publication after that run exits.
- `utils/repository_memory.rs` owns the repository-scoped shared-folder state, operating-system lock, instruction restoration journal, receipts and publication identity.
- `local-deployment/agent_run_port.rs` authorises maintenance execution using the durable workspace/session/run identity. `executors/executors/codex.rs` installs the per-thread MCP configuration.
- `server/workflow_runtime/runner.rs` uses the existing Workflow planner, node records, frozen graph, fresh AgentRun requests and durable orchestration outbox. Its current Issue identifier is mandatory; Workspace creation and Session allocation are separate concerns.
- The Workflow completion boundary currently treats AgentRun success as node success. Bootstrap needs a product validation gate before success can become a downstream handoff.
- OpenWiki publication already validates the starting source SHA and Wiki-only changes, creates the maintenance commit and squashes it into the target branch. Its durable identity supports recovery after Git succeeds but receipt persistence fails.

## Implementation checkpoints

1. Add a backwards-compatible repository execution scope, the internal system template and bounded, validated coverage verdict contract.
2. Extend maintenance ownership to the Bootstrap Workflow and delegate individual child roles before dispatch. Reuse the existing repository recovery monitor and Workflow runner; do not add a second scheduler.
3. Implement fresh writer/reviewer sessions, thread-local Reviewer isolation, content-based mutation checks and phase-safe instruction restoration.
4. Connect validated PASS/REFINE routing and final publication to the existing runner; keep ordinary Sync on its existing single-AgentRun path.
5. Add no-model tests for both branches, failure cleanup, ownership, publication and regressions. Generate types, format, run quality gates, review and correct findings.

## Compatibility decisions

- Repository runs have no Issue or WorkflowAttempt. Their repository identity is explicit; ordinary Issue runs retain their existing API semantics.
- Bootstrap uses a server-owned template and immutable per-run graph. Fresh Session bindings must never be written back into the global template.
- The repository lock serialises Bootstrap transitions as well as start/publication. Other generic Workflow entrypoints must not independently advance a repository-owned run.
- Only Generate and Refine receive writer authority. Reviewer isolation must override inherited OpenWiki configuration without modifying shared user configuration. No conversation or template-expanded upstream text is supplied to Review.
- Reviewer comparison uses the pre-review dirty worktree as its baseline, including file contents, rather than requiring an empty Git diff.
- Removing intermediate checkpoints means no intermediate Wiki commits or resumable agent stages. Existing restoration journals, Native Audit, durable command identities and final-publication recovery remain required safety mechanisms.
- Stop requests do not prove process exit. Cleanup retains ownership while child liveness or instruction restoration is unresolved. Interrupted Bootstrap work is not automatically resumed or retried with another paid model call.
- Existing publication remains one logical operation with safe idempotent recovery; a crash after target reflection must not create another Wiki commit.
- A small pass-through Transform separates the unselected branch from Publish. This preserves the runner's existing skip semantics without manufacturing another Publish iteration. The canonical plan represents an unselected branch as Cancelled with AllowPartial; a selected Refine failure still fails the product workflow.
- The durable orchestration plan has a backwards-compatible product-validation flag. Generic recovery cannot turn raw agent success into Bootstrap success. Validated completion and its orchestration audit event commit atomically. Cancellation still converges if a child exited successfully before its result was validated.
- The Git temporary-index helper now stages the already-filtered, literal file list. Tests reproduced an empty diff for a new `openwiki/` when exclusion magic was also included in the NUL input. The fix covers both status and patch paths, preserves the real index and continues excluding shared resources and dependencies.
- Changed-path enumeration includes both sides of a rename. A regression test reproduced an authoritative source file moved into `openwiki/` bypassing the old destination-only publication check; it is now rejected before any commit.
- Both server startup entrypoints fence interrupted Bootstrap owners before replaying queued commands. The existing repository recovery monitor starts after generic recovery, including the embedded-server entrypoint.

## Implemented code paths

`Repository Settings → POST OpenWiki sync → prepare_run` still selects the local
integrated branch, creates the dedicated workspace, installs the pinned public
OpenWiki integration and creates missing Wiki instructions. Only an absent target
Wiki index selects Bootstrap. Existing Sync continues down its original path.

`bootstrap::start → reserve_repository_workflow → drive_workflow_run` stores the
repository owner, a frozen system graph and fresh Session bindings. The existing
orchestration outbox launches each child after `prepare_child_dispatch` records its
exact Session, AgentRun, node and phase. The local AgentRun adapter authorises those
identities and the Codex adapter applies per-thread writer or reviewer settings.

`recover_repo → recover_locked → DeploymentAgentRunReconciliationBoundary` checks
terminal state, Native Audit proof or bounded Review JSON, source HEAD and restored
instructions before completing an agent node. The closed Review schema rejects
unknown fields, oversized output, unsafe evidence paths and inconsistent verdicts.
The Condition's limited `OpenWikiCoverageReview` source selects PASS or REFINE;
ordinary LLM Conditions remain unchanged. NodeExecution holds the validated JSON.

The End node calls the existing `publish_validated_wiki`. Finalisation reuses the
existing publication checkpoint, commit trailer, target reflection and receipts.
Only a failure after that checkpoint permits publication-only replay. Earlier
failures enter cleanup, send durable Cancel commands, wait for all child exits,
restore instructions, fail the workflow and clear the repository owner.

The additive migration rebuilds `workflow_runs` with exclusive Issue/repository
scope. Existing identifiers, attempts, nodes, frozen graphs and foreign-key
references are preserved; no historical events, Wiki files or attempts are deleted.

## Phase completion contract

These rules supersede the single-operation/mandatory Refine-operation assumptions
in the original v2 specification. The Generate/Review/Refine graph and independent
session boundaries remain unchanged.

The phase, not the final OpenWiki mode or latest RunAttempt, is the unit of
completion. `routes/openwiki/completion.rs` enumerates all persisted attempts of
the delegated AgentRun in attempt-number order and joins their Native Audit
streams. Missing/corrupt audit, identity mismatch, an unconfirmed process exit,
overlapping attempts or an unfinished operation prevents completion. A closed
audit showing no OpenWiki side effects does not invalidate a subsequent success.
Missing audit is never interpreted as an empty attempt, even for an apparent
launch failure: without independent proof of no launch it remains unknown.
Terminal projection commits just before process-exit registration. For the latest
attempt only, a normal running/spawned registration can remain pending for up to
ten seconds after the terminal update; the existing monitor polls again instead
of immediately failing the node. Older unfinished attempts and unreachable hosts
are not granted this exception. The phase cannot advance during this window.

`services/openwiki/completion.rs` replays started/completed root-thread MCP calls.
It binds each attempt's root using its own thread-start/resume RPC response,
matches call IDs and run IDs separately, checks request/response roots and modes,
and retains completed init/update facts across attempts. A failed begin has
unknown side effects and fails closed. Page/finish validation errors may recover
within the same active run. Duplicate completions are fingerprinted; conflicting
duplicates, orphan completions, foreign resumed runs and child-thread writers
are rejected. An observed same-run re-begin can continue within one attempt.
Cross-attempt recovery of an unfinished OpenWiki operation is not implemented;
start a new Bootstrap after cleanup. No new operation-history table is added.

Generate requires its own completed init and permits optional subsequent forced
updates. It cannot start a new init after completing initialisation. Refine may
perform multiple forced updates, or perform no OpenWiki operation at all when
all material findings are refuted/already satisfied. Bootstrap authoring always
uses `update + force=true` to avoid a source-unchanged no-op bypassing a requested
correction. Ordinary Sync retains its existing no-op and completion behaviour.

Refine returns `RefinementReport` version 1 as bounded JSON in the existing
NodeExecution output. `findingIndex` refers to the zero-based index of the frozen
Review findings array; the reviewer schema does not change. Every material finding
needs exactly one resolution; unknown/duplicate indexes and unresolved dispositions
are rejected. `fixed` requires Wiki paths, independent evidence and a completed
update; `refuted` requires independent evidence; `already_satisfied` requires Wiki
paths and independent evidence. Evidence paths must be existing, non-symlink files
and cannot use Wiki content or generated setup instructions as independent proof.
The role prompt distinguishes current implementation from documented intent/history.
Neither path validation nor report structure proves that an argument is true.

No Git diff is required for a completed update. With no completed update, Refine
must have no `fixed` dispositions and its restored worktree fingerprint must match
the independent Review baseline. Publication revalidates the resolution report;
it still happens exactly once through the existing Wiki-only publication path.
Setup/restore remains at phase boundaries, not between individual operations.

The host safety prompt is shared with Sync, but its operation policy is supplied
separately so that Refine does not inherit an unconditional `begin` instruction.
OpenWiki itself, its installed Skill, the Workflow graph, independent reviewer,
and source/ownership/publication guards are unchanged. No migration or generated
API type change is needed. Start a fresh Bootstrap after updating the server;
an old Refine output without the new report is not silently accepted.

These checks establish closure of the audited owned operations. They do not
prove absence of transient source edits later reverted, direct file edits after
the final finish, or activity outside the owned audited host. Existing exclusive
ownership, final source/diff guards and OpenWiki's `sourceChanged` result still
apply; stronger temporal file integrity would require separate runtime controls.

The read-only `services` example `openwiki_completion_fixture` replays supplied
audit directories in attempt order for protocol diagnosis. It does not approve
publication or replace the DB identity/ownership checks. Automated tests use fake
MCP frames with real Native Audit checksums and SQLite attempt records; no paid
model calls are required.

Validation of the phase-completion revision:

- Rust library suites for server, services, executors and workflow: 521 passed,
  three explicitly ignored subprocess-fixture/installed-CLI entrypoints.
- Workflow view/settings Vitest: eight passed.
- Repository format, check, lint and generated-types check passed.
- The previously rejected real Generate audit (`init` then forced self-correction
  `update`, both finished) passes the read-only protocol replay. This neither
  changes the historical workflow status nor approves/publishes its Wiki.
- A fresh paid-Codex Generate/Review/Refine execution remains a manual smoke test;
  automated tests do not assert model reasoning quality or complete coverage.

## Documentation authority prompt supplement

Bootstrap's shared `DOCUMENTATION_GUIDANCE` in
`crates/services/src/services/openwiki/bootstrap.rs` is included once in each
fresh Generate, Review and Refine prompt. It supplements the public host Skill;
it does not copy upstream prompts or alter the page queue, Claims schema,
review schema, deterministic branch or publication lifecycle. Ordinary Sync and
existing `openwiki/INSTRUCTIONS.md` contents remain unchanged.

You can set repository-specific scope and priorities in `INSTRUCTIONS.md`.
The reviewer may consult that brief for scope, never to obtain writer authority.
All roles distinguish current implementation from documented intent and historical
rationale, check document status/applicability, preserve inspected documentation
as evidence of recorded decisions, and avoid promoting proposals to implemented
behaviour. Code/doc disagreements remain explicit rather than automatically
invalidating the document. No executable instruction is inferred from evidence.

Generate maps high-value documentation alongside independent source exploration
and passes only relevant paths and constraints through existing page fields.
Review checks rationale/contracts, status confusion, duplication and routing to
canonical docs, using its existing bounded findings and summary fields. Material
findings affect safe changes, compatibility or operational decisions, not merely
style or the absence of a reproduced document. Refine checks original documents
and relevant source/tests/configuration, choosing correction, context or links
when a new page is unnecessary. No role is asked to read every document or copy
whole documents into each page context.

All roles receive `.openwikiignore` discipline. Configure exclusions on the
integrated branch before starting; writers must not change ignore rules inside
maintenance. OpenWiki 0.5.1 rejects ignored Claims evidence and excludes ignored
source contents from its fingerprint. Host Codex file tools are not routed
through OpenWiki's native filesystem backend, so prompt discipline is not a new
enforced host read boundary. Exclusions and uninspected areas remain limitations,
not proof of coverage. The reviewer still uses the same JSON schema and file
existence checks; no ignore parser or new validation abstraction is introduced.

Prompt contract tests cover shared-policy injection, authority distinctions,
bounded guidance, independent source/docs discovery, read-only review/schema
preservation, documentation-finding handoff and unchanged ordinary Sync. They do
not prove how a model follows the policy. For a manual comparison, include an
accepted rationale-only ADR, an unimplemented proposal and a canonical API/runbook
document, then inspect preservation of intent, status labelling and useful links
rather than page count.

Validation of this supplement:

- OpenWiki service tests: 36 passed, including five new prompt contract tests;
  the separately invoked installed-CLI version probe also passed without model calls.
- Bootstrap runner tests: five passed, covering PASS/REFINE, cleanup, failure,
  validation recovery and migration compatibility.
- Root formatting, documentation Prettier checks, frontend typechecks/lint,
  local Rust workspace check/Clippy, unused-i18n checks and diff checks passed.
  Root check/lint still fail at the separate remote dependency described below.
- File-content comparison confirmed only the bootstrap prompt/test module and
  the two OpenWiki documentation files changed during this supplement.
- Real-Codex documentation-quality comparison is not yet performed.

## Manual real-Codex smoke test

This path uses your authenticated Codex account and may incur model usage. It is
not part of the automated no-model suite.

1. Select a disposable local target branch without an OpenWiki index in Repository
   Settings. Do not select your development branch if you do not want a Wiki commit
   on it.
2. Select Initialise Wiki. Confirm there is one maintenance workspace and no new
   Issue. Settings should progress from Generating to Reviewing, optionally
   Refining, then Publishing and Current.
3. In the workspace, inspect the distinct Generate, Review and optional Refine
   sessions. Native Audit should show separate Codex thread starts, a read-only
   reviewer without selected writer skills and disabled OpenWiki MCP, and verified
   finalisation for every started writer operation. Refine corrections use update
   with force enabled; a no-change Refine instead supplies grounded resolutions
   and leaves the restored worktree unchanged.
4. Verify that the maintenance HEAD stays at the starting source during agent
   phases. At completion, the target has one Wiki-only publication and unchanged
   source/tests/configuration. Inspect the resulting Wiki with the existing viewer.
5. On a separate disposable initialisation, stop the active agent. Confirm no
   target publication, restored instructions, Error status after cleanup, and a
   fresh workspace and Generate session on the next Initialise request.
6. Run ordinary Sync on the successfully initialised branch and confirm the
   existing single-session maintenance path. The no-model branch fixture covers
   REFINE deterministically even if the real reviewer chooses PASS.

## Limits

- There is one independent review and at most one refinement, not a coverage
  completeness guarantee or a review loop. Bounded findings group related gaps.
- ReadOnly is the primary mutation boundary. The defence-in-depth fingerprint
  covers Git-visible source/index/HEAD and all `openwiki/` files, including ignored
  Wiki state; ignored dependency caches and uncommitted contents inside submodules
  are not recursively inventoried. It rejects repositories above 200,000 files or
  4 GiB of inspected content rather than silently skipping the check.
- Real paid Codex execution is a manual smoke test, not an automated test claim.
- The separate remote Rust workspace requires the existing private
  `BloopAI/vibe-kanban-private` dependency at commit
  `020091913ce5608d6c8bdb667d124fd22ab10561`. In this environment its SSH fetch fails
  (`cannot run ssh: No such file or directory`). Consequently full root check/lint
  commands cannot complete their remote step. No dependency or quality gate is
  weakened to bypass this requirement.

## Verification

Completed checks so far:

- All five Bootstrap runner integration tests: PASS/REFINE with real migration and
  Git publication, phase failure, maintenance cleanup/liveness, generic recovery
  validation gate/cancellation, and preservation of existing Issue data.
- Existing Workflow route integration tests: 38 passed; Workflow crate: 42 passed.
- OpenWiki setup/proof/publication tests, including dirty-page mutation detection,
  phase-safe restoration, no-op, source drift and publication replay.
- Codex reviewer isolation and exact fresh-child delegation tests.
- Atomic product-completion persistence and rollback injection test.
- Git safety tests: 30 passed, including literal untracked paths, exclusions and
  real-index preservation.
- Full web-core Vitest suite: 298 passed across 53 files.
- Root Rust workspace tests (including integration/doc tests): passed. The final
  Bootstrap additions also passed their focused rerun.
- `pnpm run format`, generated types and `generate-types:check`: passed.
- All frontend typechecks and `cargo check --workspace`: passed.
- Frontend ESLint, `cargo clippy --workspace --all-targets --features qa-mode -- -D warnings`,
  unused-i18n-key checks and `git diff --check`: passed.
- The installed pinned OpenWiki CLI version probe passed without model calls.
- Root `pnpm run check` and `pnpm run lint` both stop at the separate remote Rust
  dependency described above. Their remote step is unverified, not a pass. To
  finish that check, provide the existing private dependency and SSH access, then
  rerun those two commands. No remote Rust files or dependency declarations changed.
