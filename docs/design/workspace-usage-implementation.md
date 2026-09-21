---
title: "Workspace usage contract — implementation record"
description: "Architecture decisions, checkpoints and acceptance evidence for interactive and execution-only workspaces."
---

## Scope and baseline

Normative specification: [Workspace usage contract](workspace-usage-contract.md).

- Started on `feat/workspace-attrib`, HEAD `7d1b0685d4ed22dea2e4c4b3ef65d9bec25d0ccd`, identical to local `main`.
- The only pre-existing uncommitted file was `docs/design/workspace-usage-contract.md`; preserve it. No implementation commit, stash, push or release is authorised.
- Root, documentation and local-web guides and the full specification have been read. The remote backend's private billing dependency is outside routine validation scope.
- Initial environment: frontend 4020/backend 4021 from this checkout; authenticated Codex via ChatGPT; installed OpenWiki 0.5.1. Plain CLI help also emits an Ink non-TTY warning; use the supported host-driven adapter for acceptance, not the interactive CLI.
- Chrome was absent. Started an isolated headless profile `/tmp/evk-workspace-usage-chrome.sCy8NH` on loopback port 9222. Chrome DevTools MCP opened the existing EVK UI successfully. This is connectivity evidence, not acceptance of the new implementation.

## Architecture decisions

1. Add a persistent Workspace usage enum independent of physical kind, container ownership, archive and Integration reservations. Keep ordinary creation interactive. Internal creation writes execution-only immediately, before owner preparation or public observation.
2. Keep a small durable execution-owner reference on the Workspace. Owner kind is extensible; unknown owners are inspectable but cannot dispatch. Existing Workflow/Integration state remains authoritative. Only missing maintenance/preparation outcomes need a small retained result, not another scheduler or event store.
3. Public mutation policy and owner-authorised execution are separate. Reuse Workspace/Session middleware and existing dispatch guards. Extend endpoint-specific boundaries where a Workspace is reached by AgentRun, terminal, queue or an adoption request. A client-supplied owner ID is not authority.
4. Add a read-existing-only path for inspection; do not use container creation to inspect execution history. Reuse the current files, diff, Wiki and canonical timeline UI.
5. Preserve generic list consumers. Normal-work defaults filter interactive entries; an explicit execution/history scope shows internal entries with owner status and control links. Keep ID-based inspection independent of sidebar membership.
6. Backfill only from host-owned durable identities, never names. Older Sync records need repository shared-storage evidence because the latest repository pointer overwrites older maintenance IDs. Report unprovable historical outcomes as unknown.
7. History is a projection of owner outcomes, not `archived=true`; no new automatic archive scripts, deletion or retention policy.

## Implementation checkpoints

- [x] Model, migration/backfill, owner identity and creation integration.
- [x] Public mutation/dispatch guards, legitimate owner dispatch and audited cancellation.
- [x] Passive file/Wiki/diff inspection, cleanup guards and other list consumers.
- [x] Normal/execution-history UI, owner detail and read-only navigation.
- [x] Relevant Rust/Vitest tests, generated types, format/check/lint/development compilation.
- [x] Diff review, confirmed related fixes and regression re-runs.
- [x] MCP acceptance with a new isolated owner execution to completion and another stopped through UI.
- [x] User documentation and final evidence/limitations.

## Confirmed integration risks to cover

- `get_workspace_file_tree` and `resolve_workspace_repo_root` call `ensure_container_exists`; Wiki shares this path. GET is not currently a guarantee of passive inspection.
- `archive_workspace` runs scripts and stops dev servers; do not use it to implement history.
- `WorkflowSource::System` describes template provenance, not workspace usage.
- Integration reservations include source workspaces; do not reclassify them.
- `CreateModeProvider` seeds defaults from the newest workspace; changing only the visible sidebar would leave an internal-workspace default leak.
- Sync's latest repository status cannot establish earlier maintenance publication success. Keep new owner identity/results and use unknown for missing old evidence.

## Acceptance evidence

### Code map and operation boundaries

| Responsibility | Existing mechanism extended |
| --- | --- |
| Persistent usage/owner | `crates/db/src/models/workspace_usage.rs`, `Workspace::create_with_owner`, migration `20260921000000_workspace_usage.sql`. SQLx metadata and TypeScript are generated. No new execution table. |
| Evidence backfill, dispatch, cleanup | `crates/services/src/services/workspace_usage.rs`, LocalDeployment startup and existing container cleanup. |
| Atomic creation-time classification | `workspaces/create.rs` helper, `openwiki::prepare_run`, `integrations/runtime::prepare`. Ordinary creation still calls the interactive wrapper. |
| Public mutations | Workspace/Session middleware, Session creation, AgentRun controls, terminal, link/adoption/Integration admission. `/seen` and passive display preferences remain allowed. |
| Actual execution | LocalAgentRunPort admission, launch environment and controls; ContainerService script admission and actual spawn. Revalidate durable owner, repository, Session and child identity, not a client flag. |
| Owner Sessions | Bootstrap reservation uses `ensure_repository_owner_sessions`; ordinary Workflow adoption uses `ensure_agent_node_sessions`. Both reuse the same private Session creation implementation. |
| Stop and outcomes | `workspaces/usage.rs` delegates to existing Integration cancellation, Bootstrap cleanup, or audited Sync AgentRun Cancel. Existing publication locks and irreversible boundaries stay authoritative. |
| Terminal follow-ups | Execution-only skips ordinary source commit/queue finalization. Owner-specific completion, restoration and publication remain unchanged. |
| Read-only files | `ContainerService::container_for_inspection` checks an existing execution root; file/Wiki/diff/attachment/path consumers retain repository and path validation. Editor-path GET is not a passive-file API: the relay consumes it to launch an IDE, so execution-only rejects it. |
| UI | `useWorkspaces`, sidebar scopes, `ExecutionInspectionPanel`, shared composer gate, action policy, passive Git/files/diff/notes and terminal guards. Workspace IDs remain directly addressable. |

Integration source workspaces retain interactive usage. Reservations remain a separate, temporary guard. Usage does not grant owner agents additional filesystem permissions or change Reviewer sandboxing.

### Automated validation

Commands used (from repository root unless stated):

- `pnpm run format`: root and remote Rust formatting, web-core/local-web/remote-web formatting. Changed UI package components also formatted with its Prettier.
- `cargo test -p db -p services -p local-deployment -p server --lib`: **377 passed** (DB 57, local-deployment 58, server 144, services 118); one installed-CLI test is opt-in/ignored in this command.
- `cargo test -p server --test workflow_routes`: **38 passed**, including ordinary Workflow, System templates, Arena, Session reuse and cancellation/retry.
- `cargo test -p services installed_openwiki_cli_version_probe -- --ignored`: installed OpenWiki 0.5.1 probe **passed** separately.
- `pnpm --filter @vibe/web-core exec vitest run`: **380 passed / 66 files**, including usage/action policy, host identity, session selection, inspection controls, Wiki/file navigation and existing workflow consumers.
- `pnpm run prepare-db`: generated SQLx metadata; unrelated previously committed cache entries were preserved rather than opportunistically removed.
- `pnpm run generate-types` and `pnpm run generate-types:check`: passed; generated TypeScript was not hand-edited.
- `pnpm run check`, `pnpm run lint`: passed (including remote-web and root Rust; not the separate private remote backend).
- `cargo build -p server --bin server`: development binary only; no release package build.

The public-route regression uses the production middleware with a real migrated SQLite DB and HTTP requests. It proves handlers are not entered for forbidden Workspace/Session operations, forged internal fields do not bypass admission, and no Session/AgentRun/process/link side effects occur. Owner tests independently exercise live ownership, terminal/released ownership, child/Session/repository mismatches, immutable binding, persisted validation scripts and missing-root inspection. Bootstrap integration fixtures now use actual execution-only workspaces, not interactive stand-ins.

The last editor-path admission addition is separately covered by the production middleware regression, followed by check/lint/build. The final session-selection additions are UI/read-selection only and covered by Vitest. No remote service or cross-host live execution was available for a real relay trial; explicit host identity and the relay's editor-admission endpoint are covered locally. Private remote backend compilation is outside the required validation scope.

### Migration on the development data

Initial database: 38 workspaces. Startup classified **16 execution-only** (6 Bootstrap, 7 Sync, 3 Integration), with **0 conflicting owners**. The other **22** had no internal-execution evidence and stayed interactive; this count includes ordinary workspaces, not 22 known-invalid records. At least one name beginning `OpenWiki:` remains interactive because a name alone is not evidence.

Subsequent startup classified 0 additional records: idempotency verified. IDs, sessions, audits, archive/pin flags and source files were retained. Historical Sync outcomes without a retained receipt remain **unknown** even if another Sync for that repository later succeeded. After the three acceptance attempts, there are 41 workspaces: 19 execution-only and the same 22 interactive.

### Real Chrome DevTools MCP acceptance

Environment: Vite on `0.0.0.0:4020`, backend 4021, preview proxy 4022, from this dirty development checkout. Authenticated Codex and installed **OpenWiki 0.5.1** used the existing registered host-driven MCP integration. Docker CLI exists but no daemon is available here; this did not prevent the installed pinned host adapter from completing. No API-key alternative was introduced.

Isolated fixture:

- Repository `/var/tmp/evk-workspace-usage-BdRqCu`, repository ID `8513c35a-8ae3-4962-a72b-11d352c3cd4b`.
- Project `Workspace Usage Acceptance 20260921`, ID `4eaa87ff-6845-4d10-b3d8-abd076a57f19`, created and configured through the UI.
- Frozen source **`a3f2776674615f26f2d230c8ffb6f9f8e5097282`**, four source/docs/test files, three Node tests. No baseline Wiki or ignore file. Two Markdown inventory candidates.
- Targets `test/workspace-usage-complete` and `test/workspace-usage-stop`, created only in this isolated repository. No development-branch source commit or Wiki publication.

| Attempt | Evidence and outcome (UTC) |
| --- | --- |
| Initial regression reproduction | UI Initialize at 21:30:50. Workspace `ffd4cc89-d410-43c7-888f-1a85fd49a6a9`, reserved owner `ac3bf3d8-990d-4723-9cbb-6e2cbb5009f3`. New generic Session guard incorrectly rejected legitimate Bootstrap preparation; no agent started and no publication occurred. Failure/cleanup retained. Fixed before the successful run below. |
| Successful Bootstrap | UI Initialize at 21:36:47; Workflow `d711623d-e6aa-48b5-a050-9e74fab3e3ec`, workspace `be10ea97-0148-45cd-b03a-38bae26f2388`. Workflow started 21:36:53.733, succeeded 21:42:39.852 (**5m46s**). Generate → Review PASS → Publish; Refine correctly skipped. |
| Separate Stop | UI Initialize at 21:48:27 against the untouched stop branch. Workflow `049c9b8f-ce25-4e50-80cd-bf381efc860a`, workspace `398a0d22-8532-4da6-a602-c4d2674bf756`; started 21:48:32.012. Clicked **Stop execution** at 21:49:09; observed cleaning-up then **canceled** at 21:49:18.937. No publication; target stayed at the source SHA. Active run/bootstrap ownership cleared, instruction setup restored, incomplete generated files retained for inspection. |

Successful Generator: Session `7f678fe7-a626-4c06-a770-47ca7d0ccb82`, AgentRun `85866565-9056-36b9-e450-2198f7507b2b`. Independent Reviewer: Session `a91dec01-16ac-43fe-8603-3b43e6ee102c`, AgentRun `8efac7af-ff82-3acf-8246-7846460de035`. Both Native Audits closed **complete**. Stop run: Session `66c41709-1f9a-4df4-b518-aa5ffbad16cc`, AgentRun `3b3beed4-8c61-f301-4824-696446f8b170`; audit closed **complete**, not forcibly discarded.

Audit locations follow `dev_assets/runtime/native-audit/v1/sessions/{prefix}/{session}/agent-runs/{run}/attempts/{attempt}/manifest.json`. Attempt IDs are `73303462-2ec8-cbd0-ffc5-e5c05722f952` (Generate), `7fab27d7-7f44-8031-fcc9-41ddd3135498` (Review), `407bd0a7-2904-87cc-66d3-a8046944d044` (Stop).

Publication commit **`d20b75c89e0f60750b5413b0729057aea0efa734`** changed only nine `openwiki/` files on the complete target (two authored pages plus indexes/managed metadata). Worktree `/var/tmp/vibe-kanban-dev/worktrees/be10-openwiki-evk-wor/evk-workspace-usage-BdRqCu`. The source files and test branch's non-Wiki contents were unchanged. A quality comparison or large-repository coverage claim is not part of this trial.

MCP evidence is retained in **`/tmp/evk-workspace-usage-evidence-ywo4qs`**:

- `01`–`04`: target settings, rejected first preparation, new owner progress and actual Generator logs.
- `05-workflow-succeeded.txt`: Workflow Canvas with successful Generate/Review/Publish and skipped Refine.
- `06-wiki-internal-link.png`, `07-wiki-reloaded.txt`: Wiki index → quickstart → contract internal link, reload, correct current workspace/branch.
- `08-file-inspection.txt`, `17-diff-inspection.txt`: files and real diff inspected. Sessions remained 3 and AgentRuns remained 2 after inspection, with no new execution.
- `09`–`13`: isolated stop target, enabled Stop, stopping/terminal result and execution/history scope.
- `14-integration-history.txt`: existing Integration `5be44ac6-1bc4-4043-9711-3f54289eceb9`, workspace `6b934ba8-3ba6-40eb-ae81-7c1895f51faa`, succeeded/published without a composer.
- `15-old-sync-unknown.txt`: existing Sync workspace `5b7a2e2e-c798-4be4-9ec4-0865187fd09c`, unknown publication honestly retained, no arbitrary Stop/chat.
- `16-interactive-draft.txt`, `18-draft-preserved.txt`: ordinary workspace `c92658a3-b466-448f-a163-57cf614a4066` retained the test draft across an execution-only visit; Code/Send returned. The test text was removed without sending and its AgentRun count stayed 1.
- `19-diff-passive-controls.txt`, `20-file-passive-controls.txt`: repeated original operations after hiding IDE/edit affordances. No editable composer or IDE controls in execution-only views.
- `21-final-server-inspection.png`, `22-final-file-and-generator.txt`: final-server reload, saved Generator Session selection and rendered `ledger.mjs` source after reopening the file; no editor action or editable composer. A focus/refetch retained the manually selected Generator log.

Server binaries used for real owner execution were built from the unchanged HEAD plus this implementation: SHA256 `ef2a5a62fe49d4fd1cc0ab510211ff0541ff1eebee722473f7e4b328f8e11cf1` for the successful run, and `7d789fe15cf6cf0307513de61c6d4f2dcd65f8520982050e00aa49de236713d0` for Stop. The intervening backend change only corrects saved-Session read ordering; owner launch/dispatch/cancel/publication code is identical. Later changes concern inspection controls, explicit host request scoping, Session selection and rejection of the relay's editor-path GET. They do not change owner execution/completion; those controls are rechecked against the final server/UI using these retained results rather than paying for another identical generation.

Final server restarted at **2026-09-20 21:59:39 UTC**, SHA256 **`231900032105cc897f3f774dc6c8457761d173d9bd910f11f168a9349429e8f0`**. Confirmed no active AgentRun before restarting. Startup classified 0 additional records; successful/published and canceled/unpublished owner results survived. The relay editor-path GET returns HTTP 409 on execution-only, while files and saved logs remain readable. Final format, 380 Vitest tests, web-core typecheck and generated-type check were re-run after the last UI selection adjustment; complete check/lint and the updated middleware test had passed with the final backend.

### Confirmed related findings and fixes

1. **New regression — legitimate Bootstrap Session creation blocked.** Shared ordinary-Workspace admission was too broad. Split public adoption from a private owner-verified wrapper, reusing the same creation function. Wrong/unbound ownership rejects before creating sessions; PASS/REFINE fixtures use real execution-only ownership. The subsequent new MCP run completed publication.
2. **Existing Session display omission.** DB ordering considered legacy scripts but not canonical AgentRuns, opening the unused Refine session. Include canonical activity in both Session-list/latest queries. Regression test uses migrated DB and confirms read ordering without Session creation; MCP reload opens Review's actual logs.
3. **Existing refetch selection reset.** Refetch could replace the chosen log/new-session selection. Preserve valid selection within a host/workspace while honouring a new explicit Session link. Pure tests cover switching scopes, new drafts and URL-originated manual selection; MCP verifies normal draft preservation and saved log selection.
4. **New UI-policy gaps.** Shared command filtering alone did not hide GitPanel branch/PR/push, terminal, notes, setup-script suggestions or file/diff IDE affordances. Gate their own components and the shared editor hook, including loading states. SSR tests cover ordinary vs execution-only controls; the original MCP operations were repeated. Diff comment composition is disabled in inspection.
5. **New relay-policy gap found by code review.** A GET returning an editor path feeds the remote IDE launcher. Reject that capability endpoint before the handler for execution-only, without disabling safe file reads. Production-middleware test proves no handler side effects; local direct GET validates the final restriction. This is not a claim of an end-to-end remote SSH trial.
6. **New host-capture race found by code review.** A pending local-owner request could inherit a newly selected remote host. Requests now explicitly carry the captured host (including local/null); test covers both read and Stop after host navigation.
7. **Existing Bootstrap cancellation reported as failure.** Persist cancelling intent and keep ownership until child exit/restoration, then mark the owner canceled. Failure remains failure. Automated cleanup tests and the separate MCP Stop confirm the distinction.

No confirmed related finding is intentionally left unresolved. An old pending terminal `InterruptTurn` startup warning is unrelated to these new owner executions and was not modified; no historical audit/command record was deleted to silence it.

### Acceptance mapping and limits

| Criterion | Evidence |
| --- | --- |
| AC01 | Persistent usage/owner model, independent physical ownership/archive/reservations; unknown owners fail closed. Model/migration tests. |
| AC02 | All three owner creation sites call atomic owned creation. Model owner-kind tests, Bootstrap runner integration, Integration/Sync owner policy tests; normal Workflow/Arena routes and direct-folder/multi-repository regressions pass. Real Bootstrap creation observed. |
| AC03 | Evidence backfill tests, conflicting/unknown evidence, preserved old rows, 16 classified + idempotent restart on real DB. |
| AC04 | Work vs executions/history, attention/running/history, existing Integration/Sync and new Bootstrap inspected via links and URLs. |
| AC05 | Middleware HTTP rejection before effects, owner spoof/dispatch/script tests, shared UI gates and final passive controls. |
| AC06 | Bootstrap PASS/REFINE integration, owner policy, existing Sync/publication and Integration regressions; actual Bootstrap publication and separate audited Stop. Actual approval/input waiting was not induced; existing control paths and inspection response UI are covered automatically. |
| AC07 | Existing-only root used by production readers, missing-root/no-effects test, existing traversal/symlink tests, actual files/Wiki/diff inspection without additional Sessions/runs. No user worktree was deleted to stage this test. |
| AC08 | Owner-based outcomes, retained Sync/preparation result, canceled vs failed cleanup, unknown older Sync, publication distinguished from Agent completion. |
| AC09 | Interactive list consumers/defaults, peer filtering/adoption rejection, ordinary Workflow/Arena tests; normal draft/session UI restored after browsing history. |
| AC10 | Commands above, generated artifacts, development build and post-finding targeted regression runs. |
| AC11 | New small owner run to publication and another to audited cancellation; MCP logs/files/diff/Wiki/internal links/reload and normal-workspace return. |
| AC12 | This record, user guides, failure evidence and retained test assets. |

Read-only is a cooperative product-operation contract, **not OS isolation**: users with direct filesystem access can still edit files outside EVK, and legitimate writer agents retain their existing permissions. Unknown historical Sync outcomes cannot be reconstructed reliably from the latest repository state. History has no new retention guarantee and does not automatically archive/delete; normal cleanup still protects active ownership and unconfirmed processes.

The real trial exercised the PASS branch and Bootstrap Stop, not new real Integration/Sync/REFINE executions. Those paths use automated regression coverage and available historical inspection; this specification does not require three new paid generation runs. No fresh quality comparison was performed.

All isolated repositories, branches, worktrees, DB/audit records and MCP evidence are left for review. Future disposal must distinguish those exact test assets from user projects; no broad cleanup command is prescribed or executed. The development branch and its pre-existing specification remain uncommitted and unchanged in identity, with no push, release, GitHub Action or external PR.
