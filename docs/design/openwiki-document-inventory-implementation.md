---
title: "OpenWiki document inventory implementation record"
description: "Implementation decisions, validation and acceptance evidence for shared Bootstrap document inventories."
---

# OpenWiki document inventory implementation record

## Starting point

- Branch: `feat/llm-wiki-initialization`.
- HEAD: `b2728882a6ea4b9ba5a76717ca5ffb7c7a1e2815`.
- Existing untracked user files: `docs/design/openwiki-document-inventory.md` and `docs/design/openwiki-document-inventory-proposal.md:Zone.Identifier`. Preserve both; do not stage/commit user changes.
- Normative specification: [Document inventory](./openwiki-document-inventory.md), including the later static MDX extension.

## Implementation plan and boundaries

1. Extend GitService with pinned tree-entry enumeration and bounded blob reads, keeping ignore selection ahead of document body reads.
2. Use pinned Rust `markdown` 1.0.0 (markdown-rs) AST parsing for Markdown and static MDX. Verify its MDX/ESM limitations with fixtures; do not evaluate scripts or load repository plugins. No Node sidecar, renderer, or OpenWiki modification.
3. Store bounded JSON inventory chunks using RepositoryMemoryStore's existing atomic I/O and persistent lifecycle. Keep a small identity/digest reference in the existing Workflow `input_text` (host DB); shared files alone are not trusted.
4. Generate once in Bootstrap start before dispatch. Reuse the exact reference for graph prompts and dispatch prompts. Validate identity/digests at all phase boundaries and before publication. Fresh read-only Review and existing result schemas remain unchanged.
5. Add parser, snapshot, isolation, chunking and runtime regression tests. Review/fix confirmed directly related defects, run quality gates, then use Chrome DevTools MCP to start a real Bootstrap on a disposable publication branch.

## Initial environment checks

- Chrome is listening on `127.0.0.1:9222`; UI/backend are listening on ports 4020/4021. Backend executable and cwd point at this checkout.
- Codex reports ChatGPT authentication present. Secrets were not printed.
- OpenWiki 0.5.1 is installed inside this development container; EVK invokes its CLI/MCP directly. The absence of `/var/run/docker.sock` is not yet evidence that this installed host integration cannot run. No Docker configuration change is authorised/needed merely to probe it.
- Chrome DevTools MCP is configured in Codex but not exposed in this turn's tool catalogue. Test its configured stdio MCP endpoint directly; do not substitute raw browser automation for the required MCP acceptance path.

## Verification status

Implementation, public-fork quality gates and real MCP acceptance passed. The real run used the REFINE branch, published only to the disposable test branch and was inspected in the Viewer. PASS was exercised by automated runtime tests, not forced in the real run. No A/B quality improvement is claimed.

## Implemented contracts and limitations

- `crates/git/src/snapshot.rs`: pinned git2 tree metadata enumeration and bounded regular-blob reads. No symlink or gitlink traversal; no untracked inputs. Ignore policy is checked before candidate bodies.
- `crates/services/src/services/openwiki/inventory.rs`: version-1 manifest/chunks, stable paths/headings, identity and length-framed aggregate SHA-256, compact role guidance. Maximum source body: 512 KiB; heading entry: 24 KiB; chunk entry budget: 32 KiB (JSON wrapping is additional, still bounded by the shared store's 128 KiB record limit). Oversized entries become reasoned file-only records, not dropped files.
- Rust `markdown = "=1.0.0"` adds only `unicode-id` transitively and compiles into the existing server binary. GFM/MDX AST positions handle LF/CRLF, Unicode and leading YAML/TOML frontmatter without executing code. MDX static JSX child Markdown headings work. ESM and JavaScript expressions are explicitly rejected into `file_only`; without the ESM callback markdown-rs could otherwise misread ESM as ordinary Markdown. No full JavaScript parser/renderer or frontend runtime dependency was added.
- `RepositoryMemoryStore` reuses atomic immutable JSON publishing and persistent retention. Workflow `input_text` freezes only identity/counts/chunk count/digest in SQLite, with no new table or public type. Empty input remains compatible with pre-feature runs; malformed non-empty input fails closed.
- Bootstrap generates once before child dispatch, uses the same host reference in the saved graph and regenerated dispatch prompts, validates at every phase start/end and immediately before publication. Reviewer remains fresh, ReadOnly, without OpenWiki writer Skill/MCP or upstream context. Refiner output and normal Sync contracts are unchanged.
- Parsed counts include successfully parsed headless files, so they can overlap `file_only`; `instruction_only` is separate. `problems` counts limits/unsupported/read failures, not normal headless/other-format/instruction-only entries.

## Confirmed related defects and fixes

1. **Existing branch selection/status defect.** Previous branch success caused a fresh target branch to stay `stale` and display Sync Wiki. A draft branch could also be displayed whilst Run used the previous saved branch. New state and UI regressions failed before the fix (`/tmp/evk-inventory-state-before.log`, `/tmp/evk-inventory-ui-before.log`). Derived status now checks absence of the target Wiki before stale status; Run stays disabled for unsaved settings. MCP verified the disabled button and explanatory text before Save, then `uninitialized` and Initialize Wiki on the new branch (`unsaved-branch.txt/.png`, `saved-branch.txt`, `before-initialize.png` in the evidence directory below).
2. **Existing shared-record special-file risk exposed by inventory validation.** `File::open` could wait indefinitely on an agent-replaced FIFO before the regular-file check. Shared reads now use nonblocking/no-follow flags on Unix before the existing type/size check. A real FIFO test returns an error without a writer; shared memory/outbox/symlink tests also pass. No extra storage or OS isolation was introduced.
3. **Existing Workflow phase identification defect.** In the real Bootstrap Canvas, the cockpit's non-shrinking action toolbar reduced the selected phase title to 0 CSS pixels. The phase, model and state chips became unreadable (before: `workflow-running.png`). Stack identity above wrapping controls in `WorkflowNodeSessionPanel.tsx`; keep actions, execution state and event processing unchanged. Three header regressions failed before the fix (`/tmp/evk-inventory-cockpit-before.log`) and pass afterwards. MCP measured a 453-pixel title and verified the same Workflow again after reloading (`cockpit-fixed.png`, `cockpit-after-reload.txt/.png`). This is a display-only fix, so the ongoing run's generation/publication evidence remains valid; no paid restart is justified.
4. **Existing OpenWiki Viewer integration gap.** The only Wiki Viewer loaded `.llm-wiki/` through `workspaces/wiki.rs -> wiki::load_snapshot`; it could not display this run's `openwiki/` output. The new read-only `wiki::openwiki` layout adapter reuses bounded/symlink-checked Markdown reads and the existing response type. The existing GET route accepts an explicit `openwiki=true`, preserving repository resolution and the legacy default; PUT remains legacy-only. The UI offers a separate format, hides legacy write controls for OpenWiki, routes nested/absolute OpenWiki page links through the available-page allowlist, and remembers the choice per workspace in sessionStorage. Tests prove nested OKF parsing, no writes, no legacy-config dependency, unsafe/oversized rejection and format/link isolation. Production changes are restricted to viewer reads/navigation; no Agent dispatch, inventory, phase, completion or publication code changes after run start. The server was restarted only after the run finished. MCP then verified the actual completed output on the final reader build, including links and browser reload (evidence below).
5. **New Viewer directory-link regression caught against real output.** OpenWiki's generated root index links to `architecture/`, not only `.md` files. The first adapter's link resolver returned no target. A regression failed before correction (`/tmp/evk-inventory-directory-link-before.log`); directory references now resolve to their allowlisted `index.md`, including nested/absolute forms, without changing legacy routing or allowing root escapes. MCP verified root index → directory index → content after the server update. This change is frontend navigation only and cannot alter the generated Wiki or publication.

## Local validation checkpoint

- `pnpm run format`: passed.
- `cargo test -p server -p services -p utils -p git -p workflow --lib`: 291 passed, 1 existing ignored test (Git 2, server 135, services 98, utils 14, workflow 42).
- `pnpm --dir packages/web-core exec vitest run`: cockpit, OpenWiki Viewer and directory-link fixes included, 56 files / 315 tests passed.
- `pnpm run check` and `pnpm run lint`: passed after runtime/UI fixes; public-fork scope only, including remote-web but not the private remote backend Cargo workspace.
- `cargo build -p server --bin server -p local-deployment --bin agent-process-host`: debug development build passed. No release package was built.
- `cargo test --workspace`: final reader build, 858 passed, 4 existing ignored, 0 failed. The separate private remote backend workspace was not run. This includes cleanup before reservation and OpenWiki Viewer bounds/layout tests added during acceptance.
- `pnpm run generate-types:check`: passed. Generated files have not been manually edited.
- Format, full root Rust tests, `pnpm run check`, `pnpm run lint` and the debug server/process-host build were repeated successfully after the final read-only Viewer change (`/tmp/evk-inventory-*-final.log`). Complete web-core Vitest and generated-type checking also passed. The Bootstrap execution path did not change after the real run started; the later Rust production changes only read Wiki content for the Viewer.
- Runner tests cover real reservation/dispatch planning, fresh sessions, PASS and REFINE (including refutation/no-change), publication once, and input corruption at phase completion and before publication. Parser tests cover zero/other-format/MDX-only repositories, 650 documents across chunks, missing/foreign/tampered input, ignore and path boundaries.

## Real MCP acceptance run

- UI started at 2026-09-13 20:53:09 UTC (2026-09-14 05:53:09 JST). Workflow created 20:53:15.203 UTC.
- Runtime checkout: original HEAD plus this uncommitted implementation. Tracked implementation diff SHA-256 before documentation-only updates: `e9e31575f4cb3a4200fd6a79eb561a64d08b6efc0ee48f9483df5181d0e04f9c`.
- Untracked implementation hashes: snapshot.rs `b63961152541c7fff780d59e3fac51822b62377106d7e4d5dcba96718f1d8297`; inventory.rs `54aaa45bf597c8adcf12b5c15fbd032ab22128359d6f7478ae72db52bb7462be`; inventory/tests.rs `f08527aa38b170dc3ae74f25a5c75f2bb3fd1ba755df33a7f900cc5de0766aa2`.
- Server SHA-256: `9e4b0642df16a657d91cb2536729770f89065ef7a1e46b60b42495b7c445bcf2`; process host: `158a0a3f5b1b4aefb64e43b9c11e319deeb8c7550cd46e9f217115cb04462d15`. Backend PID 921278, cwd this checkout, listening 0.0.0.0:4021 behind Vite 0.0.0.0:4020. No active user AgentRun existed before replacing the old backend.
- Disposable publication branch: `test/openwiki-document-inventory-20260914`, created at source `b2728882a6ea4b9ba5a76717ca5ffb7c7a1e2815`, without checking it out. No `openwiki/` or `.openwikiignore` exists in that source. Server changes and untracked specification are not part of the target source snapshot.
- Project `fbb5d611-5ebd-48e0-a33a-cbafda48ea7d`; repository `12b6186f-0781-4e73-a844-178dd9646aa1`.
- Workflow `8af58c68-d8c9-4659-94f9-b4e656271449`; workspace `991dde86-63bb-4854-8a59-a3288cd0a9c3`; worktree `/var/tmp/vibe-kanban-dev/worktrees/991d-openwiki-easy-vi/easy-vibe-kanban`.
- Inventory available: 136 candidates, Markdown parsed 61, MDX parsed 36, file-only 39, instruction-only 6, problems 31, 7 chunks. Digest `sha256:69d100175618ada58a8aa8043ba085a7eb0635402e612143fb1bf253c0bfb355`.
- Generator Session `d73fd080-d84e-44e0-930c-82a2e8c777b7`, AgentRun `72e926b9-5bfe-fb75-c562-91510202c218`, attempt `dd307426-e273-220c-b4ed-bd51c117f7e2`. Native Audit frame 2 contains the dispatch reference; frame 85 reads the manifest; frames 101/105/190 successfully read and print paths/headings across chunks 0–2 / 3–4 / 5–6. Frame 91's missing `python` executable was recovered using `python3`; it was not counted as a successful read. This proves reads, not complete understanding.
- Reviewer Session `582484d6-1393-4fd3-a555-bc12d1851dfc`, AgentRun `1d5fc4c8-ce65-ee9b-a57b-0a51f249b365`, attempt `509ff162-0ddc-5213-4959-283328e8be8c`, started 21:21:30 UTC after Generate succeeded. Audit frame 2 contains the same inventory reference and no selected Skills. Frame 6 reports a fresh non-forked thread with no prior turns; frame 10 reports `sandboxPolicy.type=readOnly`, network disabled and approval `never`. Frame 73 reads the manifest; frames 87/91/95 successfully read and print all seven chunks using `python3`. Frame 77's failed `python` call is excluded. The read-only sandbox can read the shared input without additional write privileges. Raw outputs have no truncation marker, but this does not prove every heading was understood or exclude downstream context compression.
- MCP uses the configured Chrome DevTools server's stdio JSON-RPC tools, pinned to the installed 1.9.0 for this run. Its tools were absent from the current Codex catalogue, so `/tmp/evk-inventory-mcp.HAQFUL/call.mjs` transports actual `tools/call` requests to the same MCP. No API-only or raw CDP generation substitute was used. Evidence directory: `/tmp/evk-inventory-mcp.HAQFUL/` (snapshots, screenshots, read-only audit summaries).
- Preselected small content checks: product/task/worktree lifecycle (`README.md`, `docs/core-features/new-task-attempts.mdx`); Git integration (`docs/core-features/completing-a-task.mdx`); MCP contracts (`docs/integrations/vibe-kanban-mcp-server.mdx`); design versus implementation (`docs/future/ai-arena/spec.md` and the actual Arena implementation); Bootstrap ownership/independent review (`docs/design/openwiki-bootstrap-workflow-v2.md` and current runtime). These do not constitute whole-repository coverage or an A/B quality comparison.
- Unrelated startup observation: an old terminal AgentRun `e315d933-880a-4240-b4f2-53a32ffb2c2d` has a pending interrupt rejected during startup reconciliation. No current Bootstrap relationship or impact was observed; no unrelated cancellation/runtime redesign was attempted.

### Completion and publication

| Phase              | Started (UTC, 2026-09-13) | Finished     | Result                                            |
| ------------------ | ------------------------- | ------------ | ------------------------------------------------- |
| Generate           | 20:53:15.623              | 21:21:27.350 | Succeeded; one init operation                     |
| Independent Review | 21:21:27.409              | 21:26:09.725 | Three material findings, `needs_refinement`       |
| Refine             | 21:26:09.773              | 21:32:42.651 | Three findings fixed, four existing pages updated |
| Publish            | 21:32:42.925              | 21:32:42.925 | Succeeded; Wiki-only integration                  |

- Workflow `succeeded` at 21:32:42.933 UTC: **39 min 27.730 sec** from Workflow creation. The PASS arm was correctly skipped; its fake-provider tests remain separate evidence.
- Refiner Session `99a03210-3fc9-4bbe-93e2-4e1c01795fdb`, AgentRun `2d7883cd-47b9-0a6a-cbab-c1a7abd58a9c`, attempt `54a65116-75c4-1c04-0177-780df9108dac`. All three phases had distinct fresh, non-forked provider threads and the same workspace cwd. The stored graph has `include_workflow_context=false` for all three; only Generate/Review receive inventory guidance.
- Generate audit frame 1462 records successful finish of init `10cd29d7-06fc-4110-afcf-4d33974cd6e0`. Refine frames 87/302 record forced update `7bacffdb-b9d8-48e5-a5de-8480b1e7f5c8` and complete finish. After discovering citation line offsets, Refiner performed a second forced update `1abafeb9-0005-4a28-98e4-6ed4c5e1ca1f` (frames 370/406), also complete. The existing phase sequence proof accepted both closed operations, not merely the latest operation.
- All three Native Audit manifests are `complete`; frame counts are 1,582 / 1,424 / 1,099. Independent checks found no payload-checksum or sequence mismatches, and each whole-stream SHA-256 matches its manifest. Full audit paths are under `dev_assets/runtime/native-audit/v1/sessions/{first-two-session-characters}/{session}/agent-runs/{AgentRun}/attempts/{attempt}/`.
- The inventory's final length-framed SHA-256 still matches the frozen host input. All 136 records and seven chunks remain present: **1,472 headings, including 280 MDX headings**. Production phase-start/end and pre-publication validation passed; this is boundary integrity, not proof of continuous OS-level immutability.
- The development branch, test target and maintenance HEAD were all observed at the starting SHA while Refine was active. After publication, only the test and maintenance refs moved to **`40a256a526f9bc703a8beb07c0102026864c5f92`**, whose sole parent is the source SHA. The existing publication journal records maintenance commit `93a3848698353d8ae17e302b2d32d34c0d050a4d`; it has the same tree as the final integrated commit. These are the existing final publication's commit/integration operations, not phase checkpoints.
- The final commit contains only `openwiki/` paths. Development HEAD remains `b2728882a6ea4b9ba5a76717ca5ffb7c7a1e2815`; no source commit, stash, push or PR was performed. AGENTS/CLAUDE changes were restored. OpenWiki's untracked `.github/workflows/openwiki-update.yml` remains only in the maintenance worktree, as allowed by the existing setup-byproduct/publication policy; it is not committed or integrated.
- Repository state is `current`, error is null, bootstrap owner and active run/source are cleared. Publication receipt and repository state are in the existing `persistent/knowledge/publications/` and `state.json`; no new lifecycle store was introduced.

### Final Viewer build and MCP evidence

After all AgentRuns were terminal, the old backend was gracefully stopped and the final debug build started at 21:33:43 UTC (PID 1046203). Vite and backend listen on `0.0.0.0:4020` and `0.0.0.0:4021`. Final server SHA-256 is `a2d000a98488eb0eee22f345ea34df8f87a8ffd490ed3822105b44f6ce937d66`; process host is `cac1ac76fd8532e852aecee20b917af73d96dea296a2c49d2c459e604f4ec3a0`.

This is intentionally distinct from the generation binary above. The intervening production changes only add bounded Viewer reads, format selection and link routing, plus the cockpit layout. They do not change Bootstrap start/dispatch, inventory, ownership, phase validation, completion proof or publication. Therefore the successful generated output remains valid, while final Viewer evidence is taken on the new binary. Full root Rust tests, web tests, check and lint cover the final production code. No older run is presented as testing a changed execution path.

MCP opened **Open workspace → LLM Wiki → Wiki format: OpenWiki** and verified:

- Correct workspace UUID, repository name, test target branch and `Current workspace · openwiki/` indicator.
- Root index → `architecture/` index → architecture page, then a relative cross-directory link to the Runtime page.
- Wiki file reload, full browser reload with OpenWiki format retained, and opening the overview page after the reload.
- Full-text search for `Archive` finds the relevant workspaces page as well as other actual matches.
- Clearing the search using normal keyboard input restores all 19 visible pages. The final MCP snapshot shows an empty query, `openwiki` format, this workspace's repository and the overview heading. The MCP empty-string fill left the filter state intact; keyboard clear succeeded without any application code change.
- Existing Markdown rendering and frontmatter title/description/tags; no legacy initialise/language-write controls in OpenWiki mode.

Evidence in `/tmp/evk-inventory-mcp.HAQFUL/`: `workflow-complete.txt/.png`, `viewer-openwiki-index.txt`, `viewer-index.png`, `viewer-directory.txt`, `viewer-architecture-page.txt/.png`, `viewer-internal-link.txt`, `viewer-reloaded-files.txt`, `viewer-browser-reloaded.txt`, `viewer-overview-after-reload.txt/.png`, `viewer-search.txt` and `viewer-final.txt`. The MCP select tool expects the visible option label; an initial attempt using the underlying value failed, and selection by label succeeded. This was a tool-invocation correction, not a hidden application retry or new Bootstrap.

### Content spot-checks and limits

The final output contains ten authored knowledge pages plus nine root/directory indices (19 visible Markdown files, 92,782 bytes); `INSTRUCTIONS.md` and private Claims are not Viewer pages. A lightweight read-only link check found 257 source links and 64 Wiki/directory links, with no missing target files. This check is not a full Markdown anchor or claim-validity proof. No generated Wiki was edited by the implementing agent.

- **Product/task/workspace scope:** overview and architecture explain product purpose and separate Issue, Workspace, Session, AgentRun and Workflow responsibilities. They route users to current operations documentation rather than copying all UI instructions. Compare README, task-attempt MDX, models and execution routes; individual subtask behaviours and every Cloud API remain outside this small check.
- **Git and storage:** the workspaces page describes squash integration, staged-change rejection, preservation of unrelated edits, shared-folder lifetime and setup gates. These agree with the inspected merge implementation/tests and relevant MDX operations. It does not claim a clean Git tree is required for every operation.
- **MCP and MDX-derived knowledge:** the operations page distinguishes the external EVK MCP server from MCP servers made available to coding agents, with routes to the MCP MDX guides and actual CLI/config code. Static MDX headings are present in the mechanical inventory; 31 unsupported-expression files are explicitly file-only, not absent.
- **Historical/proposed material:** the Arena page identifies `spec-v2.md` as Draft while separately checking implemented mode/lifecycle/API behaviour. The overview and Runtime pages avoid treating all `docs/future/` material as unimplemented. Neither filename nor a proposal alone is promoted to current behaviour.
- **OpenWiki boundaries:** the memory page matches the inspected ownership, fresh independent review, forced-update, all-attempt completion and final-publication paths at the target source SHA. It intentionally does not document this uncommitted inventory implementation, which is not part of that source snapshot.
- **Reviewer findings:** final pages now describe parent/child thread isolation, the archive retention-documentation conflict, and the limited schema guard/Windows checksum fallback. The original docs and actual SQL/adapter/migration code support these distinctions. The latter two are recorded as current behaviour versus documented intent, not silently rewritten as an agreed future policy. Changing general archive/migration policy is outside this feature; no such behaviour was exercised or changed in this Bootstrap.

Within these inspected areas, no additional material content error was confirmed. This is not a whole-repository coverage certification. All provider/OS paths, complete Cloud business APIs, Desktop/review CLI details, all MDX images and external services were not exhaustively verified. Primary `repo://` citations remain inspectable in Markdown/source files; this Viewer does not introduce a new source-file navigation API for that scheme. Existing sidebar layout remains compact; no unrelated visual redesign was included.

No same-source inventory-off/on A/B was run. To study quality later, use the same source SHA, model and repository instruction configuration, two disposable publication targets, and these preselected areas. Compare omissions, source routing and historical/current distinctions; do not use page count or this single success as evidence of improvement. Do not remove ignore files to manufacture an eligible test.

## Retained assets and hand-off

- Implementation remains uncommitted on the original development branch; the two initial user files remain untouched/untracked. No generated TypeScript was manually changed, and no private remote dependency was fetched.
- Keep `test/openwiki-document-inventory-20260914`, the maintenance branch/workspace and publication commit for inspection. The current repository-memory setting points to this test branch, not main or the development branch.
- The server remains available on port 4020 with expired-worktree cleanup disabled. The test worktree still contains OpenWiki's untracked setup workflow as noted above. No tests/history/Wiki were deleted.
- Audit and shared inventory/publication records use their existing persistent lifecycle. MCP screenshots and summaries are in the container's temporary directory and may be lost on container recreation; copy them to your preferred durable evidence location before that if required. No automatic inventory garbage collection is claimed.
- When no longer needed, remove the test workspace through normal EVK workspace management after reviewing/exporting the audit; remove only the explicitly identified disposable branches if you also choose to discard their Git evidence. Do not merge this test Wiki into the development branch merely to clean up.
- OpenWiki's installed Skill was used as the lifecycle reference for this acceptance verification. EVK's three Codex sessions performed the actual writer/reviewer work; no competing standalone Wiki run was started.
