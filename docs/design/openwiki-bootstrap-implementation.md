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
ordinary LLM Conditions remain unchanged. NodeExecution holds a compact reference
to the validated report in the repository's existing shared folder.

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

Refine returns `RefinementReport` version 1 as JSON, which the host validates and
saves separately from the NodeExecution handoff. `findingIndex` refers to the zero-based index of the frozen
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

## Full review reports without handoff quotas

This revision supersedes the original limit of 12 findings and 12,000 UTF-8 bytes
for Review and Refine results. The ordinary Workflow handoff still truncates at
12,000 **characters**; its policy and ordinary LLM Conditions do not change.
The old byte budget was particularly restrictive for Japanese reports. It could
encourage prioritising a short list, but does not by itself prove why a particular
model run returned only four findings.

The independent Reviewer remains ReadOnly, with no writer MCP, extra file-writing
tool, or Generator conversation. It returns its full schema-valid final JSON.
EVK validates the response and existing evidence-file checks before atomically
publishing `knowledge/bootstrap-reports/<workflow-run-id>/review.json` in the
repository-scoped persistent shared folder. The file contains `identity` and
`report`; the latter contains the original validated findings in their stable
array order. This is not a source-repository file or a Wiki publication change.

NodeExecution stores only a versioned reference: repository, workspace, source
SHA, Workflow run, phase, Session and AgentRun identity; SHA-256 of the stored
bytes; verdict and counts. The router reads the compact verdict. At actual Refine
dispatch EVK verifies that reference against DB phase identities and the file,
then supplies a JSON-encoded absolute file path instead of embedding the findings.
The Refiner must read every finding in bounded sections, not assume that one
possibly truncated tool response is complete, and must not modify the input.
Finding text remains evidence, not instructions or executable template content.

Every material finding still needs exactly one resolution. The Refiner's complete
report follows the same host-validated path into `refine.json`; NodeExecution
again contains only a compact reference. There is no finding/resolution count
quota. The subsequent field-ceiling removal also drops the 8,000-character
description/reason/summary ceiling, 120-character title ceiling, 240-character
path ceiling and 32-path array ceiling from both model-visible schemas and
validators. Required non-empty content, unsafe-path rejection and verdict
validation remain in place.
The prompt explicitly forbids dropping substantiated material coverage gaps to
fit a handoff or inventing dispositions when output/investigation limits are hit.

Both JSON input and stored reports have a 1 MiB resource guard (stored metadata
and JSON formatting also count towards the file limit). This is not a target
report size or a coverage quota. Oversized output fails explicitly; nothing is
silently truncated. Provider output/context limits still exist. These changes
remove EVK's small handoff-derived budget, not every model or resource limit.

### Model-visible ceilings and host-only defences

Review and Refine schemas no longer contain `maxLength` or `maxItems`. Their
normal prompts do not advertise the host byte ceiling as an authoring budget.
Required fields, types, enums, non-empty strings and required evidence are still
visible correctness requirements, not output quotas. There is no numeric writing
target or ceiling per description, title, reason, summary or evidence-path list.
Longer valid paths remain subject to the operating system's actual filesystem
constraints. The validators accept and preserve these larger fields rather than
secretly applying the removed limits after generation.

Host-only defences remain unchanged:

- Raw Review/Refine JSON must fit the existing 1 MiB byte guard before parsing.
  The stored report, including identity and JSON formatting, has its own 1 MiB
  read/write guard. A raw report near the limit can therefore parse successfully
  but fail storage after envelope/formatting overhead. Oversized reports fail
  explicitly; content is never truncated or converted into a pass.
- The host-generated reference is capped at 4,096 bytes. DB-bound repository,
  workspace, source SHA, run, phase, Session and AgentRun identity and SHA-256
  content checks reject foreign, missing or changed reports at phase boundaries
  and before publication.
- Shared storage uses temporary files, synchronisation and no-clobber publication;
  only byte-identical duplicate writes are idempotent. Symlinks and non-regular
  files are rejected, with no-follow/non-blocking opens on Unix. This is not
  complete OS-level isolation or proof against all filesystem races.
- Required evidence, valid relative paths, existing non-symlink files, verdict
  consistency and exactly one disposition per material finding remain checked.
  Source HEAD, maintenance ownership, reviewer mutation checks and all-attempt
  Native Audit operation-completion proofs still gate publication.

The byte guards are not caps on total model tokens, cumulative audit/storage,
process-wide memory or the semantic work performed. Serialisation and upstream
audit ingestion can allocate before these local guards apply. They also do not
prove that an evidence-backed statement is true or that coverage is complete.
The existing 1 MiB setting is retained here, not newly calibrated or claimed as
an experimentally justified optimum. Ordinary shared-record and Workflow handoff
limits are separate contracts and are not increased by this change.

Regression tests first reproduced rejection by the old schema/field ceilings.
They then verify larger Japanese text, titles, paths and evidence lists, exact
Review/Refine file-handoff round trips, recursive absence of model-visible upper
bounds, retained non-empty requirements and explicit rejection at the host byte
boundary. No real-model generation or quality comparison is part of this change.

Validation after removing field ceilings (16 September 2026):

- The new schema/long-field tests failed against the previous implementation,
  then passed after the change. The focused prompt/contract suite passes all
  18 tests; the broader server/services/utils/workflow library suites pass 306
  tests with one existing ignored entry.
- `cargo test --workspace --no-fail-fast`: 873 passed, none failed, five existing
  ignored entries. The separate private remote backend remains outside scope.
- Workflow view/runtime/condition-output Vitest: 20 passed. `pnpm run format`,
  `pnpm run check`, `pnpm run lint` and `git diff --check` passed.
- Independent review found no confirmed regressions. No generated types,
  dependencies, upstream OpenWiki instructions, existing Wiki contents or user
  configuration changed. No server restart or real-model/MCP run was performed.

Refine dispatch, Refine completion and final publication revalidate the frozen
Review; publication also revalidates the Refine report when required. The PASS
path verifies its report before publishing too. Missing files, tampering, foreign
identities, symlinks and non-regular files fail closed. Duplicate host completion
may reuse byte-identical files, but cannot overwrite a conflicting report. The
digest is pinned in the DB, not trusted from an agent-writable checksum file.
This is boundary validation, not OS-level immutability or proof against a
temporary edit that an agent later reverses.

No DB migration, generated API type change, new artifact store or OpenWiki change
is required. Existing inline Review/Refine NodeExecution JSON remains readable.
The files follow existing persistent shared-folder retention; they are not
automatically removed on workflow completion. Canonical terminal messages and
Native Audit retain the full agent output for diagnosis.

Automated regressions cover large Japanese reports beyond 12 findings and 12k
bytes, complete material-resolution accounting, short references/prompts, both
PASS and REFINE routes (including no-change Refine), DB identity/digest checks,
legacy inline results, and blocked publication after file tampering. The real
Workflow planner, shared-folder I/O and Git publication run with fake agents;
model reasoning quality is not inferred from these tests.

Validation for the file-backed report revision (15 September 2026):

- `cargo test -p server -p services -p utils -p workflow --lib --no-fail-fast`:
  300 passed; one existing explicitly ignored test.
- Workflow run-view, runtime-view and condition-output Vitest: 20 passed.
- `pnpm run format`, `pnpm run check`, `pnpm run lint`, and `git diff --check`:
  passed. No generated public types changed. The separate private remote backend
  is outside this local-only validation scope.
- No paid Codex/OpenWiki regeneration or matched-source quality comparison was
  performed for this revision. The running development server was not restarted.

For a manual model check, restart the updated backend when no user run is active,
then initialise a **new** Bootstrap against a disposable target branch without an
existing Wiki index. Ordinary Sync does not invoke this independent Reviewer.
Inspect the Review final response in its Session/audit, the short NodeExecution
reference, the corresponding `review.json`, and the Refiner's file-read audit.
Verify every material index has a disposition before publication. Do not require
more findings as a success criterion: an exhaustive investigation can legitimately
find few problems. A matched-source comparison is needed to evaluate quality.

## Concept-oriented explanation homes

The information-architecture supplement adds a reader-oriented quality policy,
not a directory schema or a page-count gate. Important concepts, contracts and
operations need a stable, evidence-backed explanation home. Familiar domain names
do not make their boundaries, lifecycle or ordinary workflows trivial. Detailed
contracts have one primary home; other views keep useful local context and link
to it in prose explaining the relationship. Small topics can use a focused,
directly linkable section. Recorded rationale stays distinct from inference;
undocumented motives remain unknown.

`KNOWLEDGE_ORGANISATION_GUIDANCE` in
`crates/services/src/services/openwiki.rs` reaches Generate, Refine and ordinary
Sync once through `OpenWikiAdapter::host_prompt`. The independent Review prompt
includes the same policy directly, without writer instructions, change hints or
Generator history. The existing graph construction and actual dispatch paths
both use these service prompt builders; no new workflow input or artifact is
introduced.

- Generate's page-planning guidance prefers independent pages for substantial,
  repeatedly referenced concepts. A short, annotated fictional job-processing
  example distinguishes concept, workflow and architecture pages and demonstrates
  an explanatory body link. It is explicitly not a required taxonomy or checklist.
  Planning uses existing page purposes, `relatedPages` and page instructions;
  OpenWiki still owns page jobs, managed indexes and finalisation.
- Review evaluates whether a new reader can find and understand concepts and
  relationships without assembling scattered fragments. A name, heading or link
  alone is insufficient. Findings identify unanswered questions, inspected
  evidence and impact on safe development decisions. Missing standalone files or
  preferred directory names alone do not constitute material findings. Review
  does not receive the Generator's illustrative tree or plan.
- Refine can clarify, consolidate, split a substantial concept, or repair
  explanatory links. Reorganisation follows the existing OpenWiki lifecycle and
  updates affected references. Accurate knowledge and useful paths are preserved;
  already-satisfied or refuted findings still permit a successful no-change result.
- Ordinary Sync locates existing explanation homes for changed concepts and
  reconciles affected summaries, workflows and links. It does not rebuild the
  taxonomy or manufacture edits to match an example. Its existing no-op contract
  remains intact, without forcing an update.

The Review/Refine JSON schemas, material-finding threshold, operation-sequence
proof, publication and independent-session boundaries do not change. Structural
remedies use the existing `recommendedAction` values with detail in `description`,
not new action tokens. The earlier file-backed reports and ordinary 12k-character
Workflow handoff policy remain unchanged.

You can continue setting repository-specific priorities in
`openwiki/INSTRUCTIONS.md`. This supplement neither overwrites existing instructions
nor changes the seed file. EVK-specific entities such as Workspace and Session
are not hard-coded as required concepts for other repositories. No upstream
OpenWiki Skill or prompt is copied, patched or replaced.

Prompt and runner regressions check shared-policy injection, Generator-only
examples, role isolation, unchanged JSON contracts and existing PASS/REFINE,
no-change and publication behaviour. They verify EVK's inputs and control paths,
not model comprehension or generated Wiki quality. No real-model regeneration,
Chrome MCP acceptance run or matched-source quality comparison is part of this
revision. For a later comparison, hold the source SHA, model/effort and report
handoff implementation constant, then compare answers to concrete concept and
change questions for correctness, missing conditions and the reading needed to
answer them; page count or matching the sample directory tree is not success.

Validation of the information-architecture supplement:

- `cargo test --workspace --no-fail-fast`: 869 passed, none failed, five existing
  ignored entries (three subprocess-fixture entrypoints, the optional installed
  OpenWiki CLI probe and one documentation example). This covers the root Rust
  workspace, not the separate private remote backend.
- Focused Bootstrap prompt/contract tests: 15 passed. Workflow run-view,
  runtime-view and condition-output Vitest: 20 passed.
- `pnpm run format`, `pnpm run check`, `pnpm run lint` and `git diff --check`
  passed. No API types, generated files or dependencies changed.
- Independent diff review found no confirmed regressions. Runner tests use fake
  agents; production pre-dispatch prompt rebuilding was checked in code, not by
  launching Codex. Existing uncommitted report-handoff changes were preserved.
  No server restart, real Wiki generation or MCP quality comparison was performed.

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
