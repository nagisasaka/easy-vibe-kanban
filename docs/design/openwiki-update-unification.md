---
title: "OpenWiki update unification and legacy Wiki retirement"
description: "Implementation and real-browser acceptance contract for manifest-guided OpenWiki updates during parallel EVK development."
---

## 1. Purpose and status

Retire EVK's independent `.llm-wiki` execution path and complete one repository-memory lifecycle:

```text
Read the workspace's OpenWiki snapshot
  → develop source and retain Workspace Memory
  → finalise a Change Manifest
  → integrate source into the configured target branch
  → reconcile OpenWiki against that integrated source
  → publish verified Wiki-only changes
  → make the updated snapshot available to subsequent development
```

Normal development remains parallel. Canonical Wiki maintenance has one writer per repository. Change Manifests communicate intent and rationale to the maintenance agent; they do not replace repository evidence.

This is an implementation specification, not a completion report. The drafting inspection used branch `main`, HEAD `9ad6b9ccf7ab535e8d416eb12ed767619d9197ed`, on 17 September 2026. It was a code inspection, not runtime verification. Recheck the actual branch, code and environment before implementation.

Implement the change end to end, including investigation, design, code, automated tests, review, related defect fixes, documentation and Chrome DevTools MCP acceptance using authenticated Codex and OpenWiki. A test plan, mocks or an API-only demonstration do not meet the acceptance contract.

## 2. Specification boundary and initial investigation

Read this document in full, all applicable `AGENTS.md` instructions, the [repository-memory specification](openwiki-repository-memory.md), the relevant [implementation record](openwiki-implementation-checkpoints.md), [Bootstrap implementation record](openwiki-bootstrap-implementation.md), and [user documentation](../workspaces/openwiki.mdx).

This document supersedes the earlier decision to retain the independent `.llm-wiki` feature. Preserve the established OpenWiki architecture and safety contracts unless this document explicitly refines them. Do not repeat completed Bootstrap, document-inventory or Wiki information-architecture work merely because it appears in earlier implementation plans.

Before editing:

1. Record branch, HEAD and uncommitted changes. If implementation begins on `main`, create a suitable development branch without discarding or automatically committing/stashing user changes.
2. Trace normal run start, follow-up, Goal execution, source finalisation, integration, reconciliation, cleanup, publication and Viewer delivery. Map requirements to existing owners and identify the smallest changes.
3. Check Chrome DevTools MCP, Chrome, the EVK development server, Codex authentication and the installed OpenWiki/Docker arrangement. Check the actual host-driven path; do not assume previous availability or require Docker-in-Docker when the supported CLI is already installed in the development container.
4. Record a plan and acceptance checklist. Keep implementation decisions and verification evidence in an implementation record, not in a new tracking subsystem.

Routine decisions are autonomous. Record safe, backward-compatible adaptations. A new requirement for destructive migration, broader privileges or removal of a safety contract requires a decision from the user, not an improvised workaround.

## 3. Existing architecture to reuse

These are inspection starting points, not mandatory new module boundaries. Revalidate names and responsibilities at implementation time.

| Responsibility                                                              | Existing code                                                                                                                                                                                                 |
| --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Memory identities, semantic schemas, shared storage, receipts and locks     | `crates/utils/src/repository_memory.rs`                                                                                                                                                                       |
| Coding run context, source finalisation, integration selection and recovery | `crates/services/src/services/repository_memory.rs`                                                                                                                                                           |
| Executor context and terminal-run handling                                  | `crates/local-deployment/src/agent_run_port.rs`, `crates/local-deployment/src/container.rs`                                                                                                                   |
| Provider-neutral guidance, Codex execution and compaction                   | `crates/executors/src/executors/provider_adapter.rs`, `crates/executors/src/executors/codex.rs`, `crates/executors/src/executors/codex/client.rs`                                                             |
| Source merge and publication preparation                                    | `crates/server/src/routes/workspaces/git.rs`                                                                                                                                                                  |
| Repository configuration, Sync preparation and recovery                     | `crates/server/src/routes/openwiki.rs`                                                                                                                                                                        |
| OpenWiki prompts, setup isolation and Wiki publication                      | `crates/services/src/services/openwiki.rs`, `crates/services/src/services/openwiki/`                                                                                                                          |
| Bootstrap attempt aggregation and operation proof                           | `crates/server/src/routes/openwiki/completion.rs`, `crates/services/src/services/openwiki/completion.rs`                                                                                                      |
| Legacy activation                                                           | `assets/pipelines/wikillm.toml`, `assets/skills/knowledge-recall/`, `assets/skills/knowledge-enrich/`, `crates/executors/src/knowledge_skills.rs`, `crates/server/src/routes/sessions/agent_run.rs`           |
| Pipeline persistence and default card context                               | `crates/services/src/services/pipelines.rs`, `packages/web-core/src/features/pipeline/model/cardContext.ts`                                                                                                   |
| Repository settings and Wiki display                                        | `packages/web-core/src/shared/dialogs/settings/settings/RepositoryMemorySettings.tsx`, `crates/server/src/routes/workspaces/wiki.rs`, `crates/services/src/services/wiki/`, existing frontend Wiki components |

The inspected implementation already collects pending events and associated Workspace Memory into reconciliation hints. It also contains source-publication recovery, integration records, repository ownership and Wiki publication. Extend and verify these mechanisms instead of introducing parallel services.

An inspected safety gap needs explicit attention: ordinary Sync's `completion_proof` selects the latest audit stream, whereas Bootstrap's proof aggregates all durable attempts and validates their operation history. Section 8 requires a minimal shared contract, not a second proof implementation.

The OpenWiki Viewer adapter currently reuses types and helpers from its parent Wiki module. Removing that module wholesale would also remove non-legacy functionality. Follow consumers before deleting code.

## 4. Retire the legacy execution path without deleting user data

Remove EVK-owned activation of the independent `.llm-wiki` feature:

- Default insertion, selection and seeding of the legacy `wikillm` Pipeline.
- Automatic materialisation/selection of bundled `knowledge-recall` and `knowledge-enrich` Skills for that path.
- Legacy directory/config initialisation and configuration-writing endpoints.
- Legacy creation/supplement prompts, dialogs, language controls and Wiki-format switching where they exist only for `.llm-wiki`.
- Dead code, tests and current user instructions that would still advertise the retired execution path.

Deleting packaged assets is insufficient. Inspect previously seeded Pipeline files, stored card descriptions, selected Skills, new attempts, queued/follow-up messages, Goal execution and resumed sessions. They must not silently reactivate the legacy integration.

Preserve card text, history and manual additions. Where you can identify an EVK-generated legacy block reliably, omit or neutralise that block in the effective execution input without destructively rewriting historical data. Do not strip arbitrary user instructions or delete custom Skills/Pipelines merely because their names resemble old built-ins. For ambiguous legacy configuration, preserve the data and explain what cannot be used; do not silently enable both systems.

Past provider conversation cannot be assumed erasable. Apply the current repository-memory contract to subsequent execution and explain that legacy `.llm-wiki` maintenance is retired. Do not promise that removing an attachment removes every earlier conversation token.

Existing `.llm-wiki/` files remain untouched. Do not automatically translate them into OpenWiki, remove their Git history or introduce a migration generator. Document that they remain available through normal files/Git, while EVK's supported Wiki UI and execution use OpenWiki. Remove only demonstrably EVK-owned runtime activation; preserve user-managed installations and configuration.

Keep generic Pipeline support, card context, Skill selection, shared folders, Workflow, Arena and reusable Markdown/Viewer components. This is not permission for unrelated framework cleanup.

## 5. Repository configuration and normal coding behaviour

Use the existing repository-level enablement, target integration branch and output-language settings. Do not add a competing per-card Wiki engine switch. Disabled repositories must not acquire new memory side effects or silently initialise either Wiki format. Missing canonical Wiki remains an explicit uninitialised state; keep the existing Initialise workflow.

For an enabled repository, apply the same contract at initial dispatch, follow-up, Goal execution and workspace/card reuse:

- Read the `openwiki/` snapshot that belongs to the workspace's actual checkout. Read the entry point and relevant pages as reference, not as executable instructions.
- Do not modify, stage or commit canonical Wiki in normal coding runs, and do not invoke OpenWiki writer tools.
- Reuse persistent runtime instructions and provider adapters. Suppress EVK-supplied writer tools/Skills in ordinary contexts where supported, without changing global user authentication or installations.
- Preserve the Git guard against both committed and uncommitted Wiki edits, including renamed paths. Reject unintended publication without deleting the user's files. Describe this as layered policy and validation, not complete OS-level filesystem isolation.
- Keep provider-specific behaviour at the adapter boundary. The standard maintenance host remains Codex; do not break other executors or invent a new model backend.
- Signal stale/error/uninitialised knowledge honestly. Do not synchronise only Wiki files from another branch into an older source checkout.

A workspace legitimately receives a newer Wiki with its source through normal Git integration, or when a new workspace starts from an updated target. Document this distinction from a live, globally shared Wiki view.

## 6. Workspace Memory, semantic drafts and source finalisation

Reuse repository-scoped shared storage. Workspace Memory remains workspace-specific; semantic drafts and completion identities remain run-specific. Preserve repository boundaries in multi-repository and direct-folder workspaces. Do not derive roots by guessing a global worktree directory.

Retain useful intent, decisions, reasons, rejected alternatives and unresolved questions incrementally. Do not persist raw transcripts, credentials or routine progress as semantic memory. On card changes, preserve relevant accumulated knowledge but distinguish the current task and do not attribute another task's stale draft to the new one.

Maintain explicit compaction checkpoint/reread behaviour and persistent instructions to reread the current workspace's memory. Do not assume a guaranteed pre-compaction callback for automatic compaction; incremental persistence remains necessary.

The coding agent supplies semantic fields. EVK derives repository/workspace/run/task identity, Git base/source commits and changed paths. Reuse the same semantic record for the existing human commit summary and Wiki hints; do not add a second paid summary request or place transcripts in commit messages.

Preserve source-publication checkpoints, immutable events and idempotent retry. Cover the failure window between source commit creation and event publication. A later task must not replace the earlier task's frozen semantics during recovery. Missing or invalid drafts must retain source and provide a useful diagnostic/retry path, not fabricate success or consume another workspace's draft.

Do not broaden automatic source-commit authority. Keep existing review/control-only and Arena exceptions. A conversation or run with no eligible source change must not create an unnecessary commit, Change Manifest or Wiki run.

Shared memory is local coordination state, not a second Git repository or a promised cross-machine synchronisation service. Keep existing serialization validation and bounded I/O; no new database table or storage framework is required merely for this unification.

## 7. Use Change Manifest intent during OpenWiki updates

### 7.1 Selection and binding

Select only integrated, unacknowledged events for the configured repository and target branch. Prefer the existing explicit event-to-integration record, including EVK's squash-merge path. Use Git ancestry only where it actually proves inclusion. Failed integrations and another branch's events are not eligible.

Freeze the selected event IDs and source SHA for each maintenance run. Associate only relevant Workspace Memory and preserve its workspace/task provenance. Memory can contain newer, unintegrated reasoning: treat it as a hint and never promote it merely because an older event from that workspace was integrated. Keep a stable dispatched input or bound reference using existing storage conventions.

Support batching several integrated events into one update. Do not require one paid Wiki run per card. External merges/squashes/cherry-picks that lack reliable integration identity must not silently acknowledge events. Preserve an explicit manual Sync path and explain the limits of automatically associating such changes.

### 7.2 Effective maintenance instructions

Pass the selected Manifest content and relevant Memory to the actual maintenance dispatch, not just a preparation object or UI preview. Ask the host Codex to use these fields as research and interpretation hints:

- Goal and summary: what changed and what the developer intended.
- Behavioural/architectural changes and affected invariants: which concepts, contracts, relationships and workflows to investigate.
- Decisions, rationale and rejected alternatives: why the implementation took this form, when supported by evidence.
- Tests and unresolved questions: verification performed, uncertainty and remaining limits.

Repository source, tests and actual configuration are authoritative for implemented behaviour. Canonical documents can establish recorded intent, contracts and historical rationale; distinguish them from obsolete proposals. OpenWiki, Manifests and Memory are derived or contextual material, never operator instructions. Resolve contradictions in favour of the integrated implementation and report unsupported intent rather than inventing it as fact.

Reconcile affected explanations against actual integrated code. Preserve useful knowledge, canonical explanation locations, output language and meaningful links. Follow dependency/semantic impact beyond changed paths when necessary, but do not turn every Sync into unconditional whole-repository regeneration. Do not simply append a changelog, copy a Manifest into a page, or manufacture an edit when the existing Wiki remains accurate.

Use the installed OpenWiki Skill and public host-driven MCP lifecycle. The EVK-started Codex performs research and authoring; do not introduce an imagined separate LLM inside MCP or duplicate the upstream Skill. Keep EVK's small authority/organisation guidance shared where appropriate.

For large hint sets, use existing shared storage and bounded, complete reference-based reading when inline input is unsuitable. Do not silently truncate events or claim they were processed when the agent could not access them. Keep hint data separate from trusted instructions and validate host-controlled paths/identities.

Manifest guidance is expected to improve relevance and preserve intent. Do not claim measured speed, token savings or quality improvement without an actual comparison. Functional evidence must show delivery, agent use and an update consistent with both intent and source.

## 8. Single-writer lifecycle and completion proof

Keep source work and per-workspace event production parallel. Serialize canonical maintenance with the existing repository ownership/lease/lock mechanism, and use only short locks for source integration/publication. Do not hold a development-blocking lock throughout a paid model run.

Normal reconciliation uses a dedicated maintenance workspace and host session running OpenWiki `update`. It does not require the Bootstrap Generate/Review/Refine DAG, new scheduler or extra reviewer. Preserve Bootstrap and its independent-review contracts as existing consumers.

Extend/reuse the strengthened completion proof at the smallest existing boundary:

- Cover every durable RunAttempt belonging to the maintenance execution in order; do not inspect only the latest available audit.
- Verify audit integrity, run/attempt/session identity, root-thread evidence, repository/worktree identity, ownership, frozen source and confirmed process exit.
- Reconstruct the full applicable operation sequence. A later successful attempt cannot erase an earlier unfinished or unknown side effect.
- Permit multiple supported `update → finish` operations. Every operation that requires finalisation must be closed successfully, with no active or unproven operation remaining.
- Treat an audited public `begin` no-op as a normal success where the upstream contract permits it. Do not require a changed Wiki tree or a new commit to prove a valid no-op.
- Reject bypasses such as a custom shell/stdio MCP bridge that cannot supply the registered native MCP evidence. Do not weaken the proof to accept a previously failed smoke test.
- Preserve Generate's init/optional self-correction sequence and Refine's supported no-change dispositions. Share validation mechanics without imposing another phase's success rules on Sync.

Do not add a new operation-history store if existing Audit and runtime records provide the proof. Treat bounded process-exit registration delays according to the existing runtime lifecycle rather than immediately declaring corruption.

Only verified maintenance and publication may acknowledge selected events. A failed receipt leaves its event pending. Preserve source changes on maintenance failure. Report a stale/error state instead of falsely reporting current knowledge.

## 9. Publication, retry and user-visible operation

Retain setup isolation/restoration, user-authored `openwiki/INSTRUCTIONS.md`, source/HEAD safety checks, Wiki-only commit selection and publication idempotency. Temporary OpenWiki AGENTS/CLAUDE/setup outputs must not enter source publication. Do not manually edit OpenWiki private state, Claims or provenance to force success.

Keep the maintenance worktree at its frozen source HEAD until the existing final publication step. Do not add intermediate commits or node checkpoints. If the target advances, do not publish stale output against a different source. Retain events and diagnostics and make a fresh reconciliation against the new integrated source possible. Preserve durable publication recovery so restarting EVK cannot create duplicate commits or acknowledgements.

Reuse the existing automatic trigger for eligible integrated events and manual Sync for recovery or source changes without a Manifest. Initialisation remains explicit. An existing `openwiki/` directory alone does not prove EVK has established a successful maintenance baseline; verify normal configuration and Sync behaviour, including imported Wiki snapshots, without fabricating state records.

Do not create an unbounded automatic paid retry loop. After an error, reuse the existing explicit retry policy unless a documented bounded recovery is already safe. Stop and failure handling must preserve audited cancellation, confirm process termination before releasing writer ownership and retain enough evidence for diagnosis.

Keep repository settings/status, active maintenance navigation, pending/completion errors and the read-only OpenWiki Viewer usable. Report repository and actual display source clearly. Preserve the main-column article/tree experience, page navigation, Markdown/internal links, reload and repository/workspace isolation; do not redesign unrelated UI.

## 10. Non-goals and permissions

Do not introduce:

- Another Wiki engine, external Wiki Git repository, vector database, generic artifact store or scheduler.
- A DAG for every update, automatic PR/merge to a remote repository, or per-card canonical Wiki writers.
- OpenWiki source forks, vendoring, patches, internal-module imports or copied upstream prompt stacks.
- A separate OpenAI API-key/Responses API path, global credential changes, or new privilege grants.
- Automatic `.llm-wiki` conversion, destructive database/history cleanup, or broad runtime rearchitecture.
- Bootstrap content-quality redesign, arbitrary minimum page counts, or a generic evaluation platform.

The implementation goal authorises local development compilation, normal UI use and real calls through the existing authenticated Codex/OpenWiki path for the acceptance trial. It does not authorise release builds, package publication, pushes, PR creation or GitHub Actions. Local Wiki publication to explicitly isolated test targets is authorised in section 12 and is distinct from package publication.

Do not automatically commit implementation changes, stash user changes or write generated Wiki to `main` or the development branch. Preserve unrelated uncommitted files. Do not stop an existing user run to make testing convenient.

## 11. Automated tests and quality gates

Add or extend tests at production connection points, not only string snapshots. Automated suites must not require paid model calls.

| Area                 | Required coverage                                                                                                                                                                                                  |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Legacy retirement    | New cards, old generated blocks, persisted seeds/settings, initial/follow-up/Goal input, Skill selection, preservation of manual text/custom assets/old Wiki files                                                 |
| Normal coding        | Disabled/enabled repositories, missing/stale Wiki, writer restrictions and committed/uncommitted Wiki guards, no-source-change runs, control-only/Arena exceptions                                                 |
| Memory and manifests | Repository/workspace/run isolation, direct-folder/multi-repository cases, task reuse, compaction continuity, host-derived identities, missing/invalid drafts, source-finalisation retry and immutable event replay |
| Integration          | Two parallel worktrees, explicit squash mapping, unmerged/unrelated event exclusion, batch selection, external-unproven integration, concurrent source advance                                                     |
| Prompt/data path     | Actual dispatch receives the selected hints/references; preserved rationale and authority rules; no silent truncation or stale task substitution                                                                   |
| Proof and recovery   | All-attempt replay, unfinished earlier operation, missing/corrupt audit, wrong root/owner/source, multiple completed updates, legitimate no-op, process-exit handoff, registered-MCP enforcement                   |
| Publication/status   | Wiki-only output, instruction restoration, target drift refusal, failed events remain pending, bounded retry, Stop/cleanup/ownership release, crash recovery and duplicate prevention                              |
| Existing consumers   | Bootstrap PASS/REFINE, Refine no-change, normal sessions/Goals/Workflow/Arena, shared helpers and Viewer navigation/reload                                                                                         |

Follow applicable repository gates, including `pnpm run format`, relevant Rust unit/integration tests, related Vitest, required Rust-to-TypeScript generation/consistency checking, `pnpm run check`, `pnpm run lint` and necessary development compilation. Do not hand-edit generated types. Run the applicable root workspace tests and report their actual scope.

The separate remote backend's private billing dependency is outside routine public-fork gates under `AGENTS.md`. Do not repeatedly fetch it, install SSH or rewrite manifests to bypass it. Retain remote-web checks and remote Rust formatting. If a change affects remote contracts, explicitly report unavailable remote verification.

Review the complete implementation diff, fix confirmed findings and rerun affected checks. Do not treat a skipped or failing required local gate as passed.

## 12. Mandatory Chrome DevTools MCP acceptance

### 12.1 Fix the execution and data baselines

Run the changed backend/frontend at `http://localhost:4020`, accessible from outside the sandbox. Confirm which executable and frontend source are serving it. Record EVK HEAD plus its uncommitted diff separately from the repository source SHA and Wiki baseline used as test data.

Prefer an isolated checkout of an EVK snapshot with an existing canonical OpenWiki. Create collision-free `test/openwiki-update-*` local branches and a separate test repository registration/shared-state identity where necessary. Verify both the maintenance worktree and the target of source/Wiki integration before launching agents. Do not redirect the user's operational repository settings or alter its production target.

Local test source commits, EVK source integration and Wiki-only publication to those isolated test targets are permitted. They do not authorise committing development changes or writing to `main`. Keep test branches and evidence for final inspection; do not delete them automatically.

Use normal UI configuration and, when required, a successful manual baseline Sync to establish readiness for automatic updates. Do not rewrite `last_success`, receipts or ownership records. Do not delete an existing Wiki to force initialisation. If the preferred snapshot is unsuitable, document a safe alternative. A small fixture demonstrates lifecycle correctness, not EVK-wide content quality.

### 12.2 Exercise normal parallel development

Through Chrome DevTools MCP, perform the normal UI operations:

1. Open repository settings and verify enablement, target test branch, language and healthy baseline.
2. Create two independent cards/workspaces A and B from the test baseline. Ask authenticated Codex to make small, meaningful source changes with independently checkable intent and tests. Keep the two development worktrees separate and permit their development to overlap.
3. Verify their normal execution reads the workspace Wiki without writing it, records useful Memory, and produces run-specific semantic drafts and host-finalised Manifests. Do not fabricate Manifests to substitute for this path.
4. Integrate A through EVK's UI while B remains unintegrated. Observe the automatic maintenance run and its publication. Confirm B's event is not acknowledged and B-only behaviour has not become canonical Wiki knowledge.
5. Integrate B and observe another update against the then-current target and Wiki. Confirm A's still-valid knowledge survives and B's intended implemented change is covered. Resolve test-only source conflicts through normal Git operations if necessary; never resolve them by copying branch-local Wiki output.
6. Exercise manual Sync through the UI. Accept a correctly proven no-op when no relevant source changed; do not manufacture a Wiki edit for the test.
7. Exercise a legacy-card follow-up and a fresh normal request in isolated test data. Verify no legacy `.llm-wiki` initialisation, automatic Skill attachment or writer execution reappears. Preserve historical/manual card content.

Do not force paid runs to exhibit every failure or every batching outcome. Cover deterministic adverse cases with automated tests and distinguish those results from the observed real-agent path. Wait for actual run/audit progress before treating a temporarily quiet UI as a hang. Avoid duplicate clicks/launches when an existing run may be active.

### 12.3 Verify actual use and publication

Collect evidence beyond prompt presence and an agent's final statement:

- Repository/workspace/run/event identity, source integration and selected event IDs.
- Memory/draft/Manifest contents with secrets excluded, and the actual maintenance dispatch.
- Audit evidence that the maintenance host accessed and used the supplied semantic hints, either inline or by reading their bound references.
- Root-thread registered-MCP operations, complete attempt proof, terminal state, setup restoration, publication result and corresponding receipts.
- Source changes retained independently from Wiki success; no Wiki update on the development branch.
- A before/after Wiki comparison against source for the specific test changes, including rationale where evidence supports it. Explain unsupported or unchanged hints rather than requiring every Manifest field to become prose.

Open the resulting Wiki in the EVK Viewer through MCP. Confirm repository and source, navigate several relevant pages and representative internal links, then reload and repeat an affected navigation. Do not accidentally demonstrate an older Wiki or another run's worktree.

Use a new workspace based on the updated target, or a legitimate source-and-Wiki Git integration, and confirm a subsequent normal agent can read the new knowledge. Do not inject Wiki files alone into an older checkout.

Record run IDs, timestamps/duration, source and publication commits, output locations, audit references, MCP operation records or screenshots, and the content-check results. API/CLI inspection may supplement the evidence but cannot replace starting and exercising the user-facing operations through MCP.

## 13. Related defect investigation and repair loop

Fix defects discovered during implementation, review, automated tests or MCP acceptance when either:

1. This change caused the regression; or
2. A confirmed pre-existing defect directly impairs this specification's start/continuation, memory/Manifest flow, integration, permissions, maintenance, progress/error UI, cancellation/cleanup, proof, publication or result viewing.

Being in the same module is not sufficient. Record the affected contract and the causal path or reproduction evidence. Unrelated defects and optional improvements are recorded only. Do not expand into general performance tuning, cosmetic redesign or unbounded bug hunting.

For each confirmed related defect:

1. Preserve the failing run/evidence and establish the cause through reproduction, audit or decisive code evidence.
2. Inspect adjacent execution paths sharing that cause, rather than fixing just the first visible symptom.
3. Add a regression test that demonstrates failure before the fix where practical. If automation is unsuitable, record why and retain a repeatable manual procedure.
4. Implement the smallest safe fix using existing abstractions. Test other affected consumers of shared runtime code.
5. Rerun relevant automated gates and repeat the original browser operation with MCP. Correct API/DB state alone does not prove a UI fix.
6. If the fix affects dispatch, hint delivery, run control, proof or publication, run a fresh UI-started end-to-end update through publication and Viewer on the corrected EVK. Do not reuse an earlier binary's success as evidence.
7. For a display-only fix that cannot affect generation/publication, reusing existing successful output is acceptable if you explain why, rerun the affected MCP operations and pass regression tests.

Record each retry's reason. Do not repeat unchanged trials until one happens to pass. Do not hand-edit generated content, receipts, audits or OpenWiki private state to conceal a failure. A small page count or a stylistic preference alone is not a proven integration defect; inspect its relation to the actual contract.

Keep a concise defect ledger in the implementation record: regression/pre-existing, symptom, evidence, cause, contract impact, fix, tests and MCP recheck. Related defects are not a reason to stop while an in-scope safe fix remains possible. They are also not acceptable leftover notes under a claim of completion.

## 14. Acceptance criteria

Mark the implementation goal complete only when all of these are evidenced:

1. EVK no longer automatically activates the legacy Wiki path, including persisted cards/settings and follow-up execution. User data and generic shared capabilities remain intact.
2. Enabled normal coding runs use the OpenWiki read-only/Memory/Manifest contract with correct identity, source finalisation and compaction continuity.
3. Integrated Manifest intent reaches and informs the real maintenance host, while source/tests/config remain authoritative and unintegrated events are excluded.
4. Parallel development remains possible; one canonical writer handles batching, retries, no-op and source drift safely using the existing lifecycle.
5. Ordinary Sync uses a full, verified completion contract without weakening Bootstrap or Refine, and publication/receipts are safe and idempotent.
6. Required local tests and quality gates pass; confirmed review findings are fixed.
7. MCP-started real Codex development on two isolated workspaces proceeds through sequential integration, automatic OpenWiki updates, valid publication, manual Sync and Viewer verification. A subsequent normal agent can access the updated Wiki snapshot.
8. Confirmed directly related defects found in this work are fixed, regression-tested and rechecked through the affected UI path; verification remains valid for the final code state.
9. Implementation, old-data handling, operation, recovery, dependencies and verified limits are documented. Development branches and existing user changes are protected.

This verifies the update integration and selected content changes. It does not prove complete Wiki coverage, a global quality improvement or lower token usage. Such claims require separate evidence and are not added completion gates here.

## 15. Progress, blockers and final handover

Work in checkpoints and report progress, verified results and remaining work concisely. Continue after a failed attempt when an in-scope repair is available. Stop after the acceptance criteria are met; do not continue unrelated optimisation.

If credentials, available usage, Chrome MCP, an actually required Docker connection or another external prerequisite prevents the mandatory trial, exhaust safe checks/alternatives and finish the implementation and local verification that remain possible. Report **blocked / acceptance incomplete**, not success. An unavailable private remote backend excluded by `AGENTS.md` is not by itself such a blocker.

Do not change credentials, security policy or external service settings without authority. Do not classify an unfixed implementation bug as an environmental limit. If a safe fix needs a material new permission or design decision, explain the evidence, completed work and smallest decision needed.

The final handover must include:

- Architecture, principal files, removed legacy entry points and preservation of existing data.
- The exact Manifest-to-update path, memory/compaction behaviour and integration/publication contracts.
- Commands, results and exclusions for automated checks, with the final EVK revision/diff to which they apply.
- Test repository/branches, source SHA, workspace/session/AgentRun IDs, timing and retained test assets.
- Actual Manifest use, OpenWiki operation proof, publication commits/receipts, Wiki differences and MCP Viewer evidence.
- Checked content questions and unresolved limits, without unmeasured quality/performance claims.
- The related-defect ledger and before/after verification, including why any earlier results remain applicable.
- Unrelated issues left untouched, final completion/blocker status, minimal restart instructions if blocked, and optional cleanup instructions without deleting assets.
