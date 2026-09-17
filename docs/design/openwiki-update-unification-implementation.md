---
title: "OpenWiki update unification implementation record"
description: "Architecture decisions, checkpoints, regression evidence and real-browser acceptance for retiring the legacy Wiki path."
---

## Baseline and scope

- Specification: [OpenWiki update unification](openwiki-update-unification.md).
- Started on 17 September 2026, branch `feat/llm-wiki-updates-flow`, HEAD `9ad6b9ccf7ab535e8d416eb12ed767619d9197ed`.
- Existing untracked files: the specification above and `openwiki-document-inventory-proposal.md:Zone.Identifier`. Preserve both; no implementation commit, stash, push or release is authorised.
- Chrome DevTools MCP connected to the running local EVK on port 4020. Codex reports authenticated ChatGPT access. The installed OpenWiki package is version 0.5.1; its CLI does not implement `--version` (existing EVK version detection uses its supported package boundary).
- Existing source/Manifest outbox, integration records, repository ownership, setup restoration and publication are retained. Ordinary Sync reuses the stronger Bootstrap all-attempt proof, with Sync-specific update/no-op success rules.

## Plan and checkpoints

1. Retire legacy Pipeline seeding/defaults, generated execution instructions, automatic Skills and initialisation. Preserve historical/custom data and generic features.
2. Remove legacy Wiki configuration/generation UI and endpoints; retain shared parsing/navigation helpers and make the Viewer OpenWiki-only.
3. Reuse all-attempt operation validation for normal Sync; preserve safe process exit, cleanup, ownership and publication. Improve Manifest guidance/delivery at existing seams where required.
4. Add production-path regression coverage, generate types, format, run applicable Rust/Vitest/check/lint gates, review and repair confirmed findings.
5. Use isolated test source/branches and normal Chrome MCP UI to exercise two development workspaces, their Manifests, integration, automatic updates, manual Sync and Viewer; diagnose and retest directly related defects.
6. Record final-state evidence and update user documentation. Do not declare completion before mandatory real acceptance.

## Verification status

Implementation and mandatory real-browser acceptance completed on 17 September 2026. Two overlapping normal Codex workspaces produced separate source commits and Manifests; sequential UI integration triggered verified automatic updates. Manual Sync, a subsequent normal reader, Viewer links/search/reload and the directly related defect rechecks also passed. The acceptance matrix and revision-specific evidence below delimit this claim: it does not establish whole-Wiki coverage or reduced token usage.

## Implemented connections

- Retired EVK-owned Pipeline seed/selection, default card context, Knowledge Skill materialisation, legacy initialisation/configuration endpoints, and create/supplement/format-switch UI. Generic Pipelines and user-managed Skills remain. Old generated blocks are filtered at both request creation and the frozen runtime launch boundary; ambiguous edited blocks return a diagnostic without modifying stored cards. Persistent provider guidance supersedes old conversation instructions. Existing `.llm-wiki/` files are neither removed nor migrated.
- Ordinary Codex threads disable the OpenWiki writer MCP through thread-scoped configuration. Maintenance and independent Reviewer retain their existing adapters and permissions. No global Codex configuration or upstream package is patched.
- Existing coding Memory, semantic drafts, source outbox and explicit integration records remain the producers/selector. Each Sync freezes selected Manifests and associated Workspace Memory under the existing `knowledge/sync-inputs/<maintenance-workspace>/` shared namespace. Numbered bounded JSON chunks and a manifest avoid prompt truncation. Host state binds the content digest, repository, source, target and selected event IDs; actual dispatch and pre-publication validate that input. Old in-flight Sync without the new optional digest retains its already-frozen inline input.
- One shared Native Audit proof now serves Bootstrap and ordinary Sync. Sync permits completed update sequences and audited upstream unforced begin-noop; it never accepts init, prose completion, missing/corrupt earlier attempts, unfinished operations or a shell/stdio MCP substitute. Bootstrap's distinct Generate/Refine policies remain.
- Cleanup checks every child's process registry, not just terminal projection. Unconfirmed exit/restoration retains ownership and diagnostics. A failed unspawned reservation is cleanup evidence only, never a successful operation proof. Publication continues using the existing Wiki-only Git guard/checkpoint/receipts; no schema migration or new scheduler was needed.
- Viewer uses only `openwiki/`, retaining the main article, tree, internal links, search, reload and repository/workspace-scoped response handling. Language and maintenance controls remain repository settings.

## Related defect ledger

| Finding | Cause and minimal repair | Automated evidence / real check |
| --- | --- | --- |
| Pre-existing ordinary Sync could overlook an earlier failed/unfinished attempt | Latest-audit selection replaced with the existing all-attempt Bootstrap proof and Sync-specific success policy | Shared proof tests and production database/audit gate cover adverse attempts; final-revision automatic Sync A passed the actual proof and publication |
| Pre-existing cleanup could restore/release on terminal projection before confirmed host exit | Shared registry-exit guard for Sync and Bootstrap; failed launch no longer clears reservation when waiting for host exit fails | Bootstrap cleanup and Sync handoff regression tests; real Sync A restored setup and released ownership after verified exit |
| Retirement compatibility could retain normalised EVK-owned Skill paths | Match both original asset path and its canonical root, without name-based removal or deleting files | Owned/custom Skill and frozen-launch tests |
| Pre-existing new-project settings showed an error for absent working-location defaults | The local scratch route returns `400 Scratch not found`; recognise this exact absence response alongside 404, preserving other errors | New regression failed before the repair and passed afterwards; MCP showed `Not configured`, then saved the isolated target normally |
| Retirement regression: an unchanged legacy card's new-attempt editor serialises blank lines and `output\_language` | Exact stage matching must tolerate the existing editor's lossless Markdown normalisation, without accepting changed stage semantics | Reproduced in MCP draft `35c5ddee-63c7-4a54-9914-e1f8850a7f44`; regression test passed, corrected-runtime A initial and follow-up reached Codex without legacy stages, stored/manual text preserved |
| Pre-existing cancellation could leave tool grandchildren running in separate Linux sessions | Immediate provider teardown denied native cleanup; a bounded native-interrupt grace alone still left a child `sleep`. Snapshot existing descendants before interrupt using Linux pidfds, then terminate/wait for survivors in addition to existing process-group cleanup | MCP reproduction `6b956b22-cfe9-4604-9ad9-6c41d26f5f8b`; process test checks detached descendants exit while an unrelated process survives. Final-revision A initial UI Stop removed the shell and its separate-group sleep without manual cleanup |
| Pre-existing resumed Codex context could expose an old draft destination despite updated resume configuration | Reassert supplied current developer instructions through acknowledged public `thread/inject_items` before continuation; keep the same native session and resume configuration | Transport test fails before the fix, then passes for Code/Plan/Goal/Plan-with-Goal and failure handling. Fresh A/B same-session follow-ups produced current-run drafts, host source commits and immutable events |
| Pre-existing Workspace sidebar could remain running after Sync completed | The legacy workspace patch stream watches workspaces/script processes, not AgentRun transitions. Expose the already-computed all-active-runs/scripts aggregate through existing periodic summaries and use it for sidebar grouping | Sync A remained running for over eight minutes although both canonical tables were `succeeded`; browser reload corrected it. Three new regression cases failed before the fix (terminal convergence and other-active-agent/script cases); older-host fallback passed. Final-server manual Sync moved out of running without browser reload while reader C remained running; all four tests passed |

## Earlier local validation checkpoint

- `pnpm run check`: passed (local-web, remote-web, web-core, UI and root Rust workspace).
- `pnpm --filter @vibe/web-core exec vitest run`: 58 files / 331 tests passed at this checkpoint; the final display-revision suite below supersedes this count.
- `cargo test --workspace --no-fail-fast --quiet`: passed, including 134 server tests and the production all-attempt/cleanup/Bootstrap gates. Explicitly ignored tests remain ignored, not passed.
- `pnpm run format`, `pnpm run check`, `pnpm run lint`: passed at this checkpoint and rerun after later repairs as recorded below.
- `cargo build -p server --bin server -p local-deployment --bin agent-process-host`: development build passed; backend restarted at 2026-09-16T20:44:50Z with this binary and the matching frontend.
- Rust tests initially exposed expectations for an absent developer instruction and case-sensitive prompt text; expectations were updated to the new contract. Earlier failing runs are not counted as passed.
- Type generation was run from Rust; generated TypeScript was not hand-edited.
- The separate private remote backend is not a required local validation gate. Remote-web typecheck and remote Rust formatting are retained.

## Real acceptance preparation

- Isolated local clone: `/var/tmp/evk-openwiki-update-73t9FajK/repository`.
- Target branch: `test/openwiki-update-20260917`, starting at `9ad6b9ccf7ab535e8d416eb12ed767619d9197ed`, with its existing tracked OpenWiki and Claims/metadata. No Wiki was removed to force initialisation.
- Separate UI-created project `88cfd6ec-6bad-406b-b29a-5d403e7b73ad`, repository/shared-state identity `006e42c0-2783-459c-be43-794af1cd941a`. Operational repository settings remain untouched.
- The backend watcher was paused for deterministic builds, then retired during server restart. Frontend was restarted explicitly with `--host 0.0.0.0 --port 4020 --strictPort` after its module graph retained a reference to a removed legacy component. That transient stale-module failure is not a successful UI check; acceptance uses the restarted frontend and freshly built backend.

### Baseline Sync

UI-started workspace `5b7a2e2e-c798-4be4-9ec4-0865187fd09c`, session `1ffac545-00e6-4af7-b05d-92abc19eb122`, AgentRun `82d8417e-465f-4f9e-92f2-47371ca04a7d`, attempt `b6a3081c-7b0c-4890-a673-e0469622ef6a`.
Run started 20:45:14Z, final response 20:49:07Z, successful publication recorded 20:49:21Z (16 September UTC). Target advanced from `9ad6b9cc` to `af39363eded87aa4de12caae0349a21da011d2b1` with only two OpenWiki metadata files changed; existing prose was preserved. Native Audit confirms the frozen manifest read/hash, registered root-thread `openwiki_begin(update)` and `openwiki_finish(complete)`. Repository status became `current`, ownership cleared, no coding errors. This establishes the baseline, not yet the two-workspace acceptance.

### Preliminary coding runs (not final-revision acceptance)

UI-created cards A `97a7e708-949d-4b21-b2ba-4baec99faf50` and B `1a1f561d-6eab-44d2-abc6-f9692a1f585d` began overlapping Codex development at 20:50:51Z and 20:52:35Z. Workspaces are `7ca26038-dab7-4c5c-981b-a8338b4105d9` and `f0bd1938-490f-4fed-990e-d70d446a0718`; AgentRuns are `77f9bd3c-09eb-4df8-a084-9cb93ff915de` and `1cb8665d-7f7f-424d-99e3-dd6c47ec73a2`.

A covers Unicode NFC search, preserving AND/metadata behaviour and rejecting NFKC compatibility folding. B covers `Dockerfile.<nonempty suffix>` syntax inference, retaining explicit backend language and binary/unsupported precedence. Audits show both reading the checkout's Wiki and writing separate Workspace Memory. Dependency setup initially lacked offline tarballs; tests distinguish this from genuine red-test failures and subsequent installed-dependency checks.

The retired preset can no longer be selected in the UI. Its exact historical generated block plus a manual note was seeded into the isolated A card through the existing local issue API after normal UI card creation; no operational/user card was touched. Agent starts, follow-ups and integration remain UI operations. A new-attempt UI draft exposed the real Markdown round-trip mismatch above, before a provider launch. These preliminary runs must not be presented as final-revision acceptance after correcting execution-input compatibility.

Both preliminary coding runs were stopped via the UI so the finalisation test can run on the corrected runtime. Their source changes and memory were preserved. Four test-only check/lint process groups survived the old immediate cancellation; after confirming their worktree identity, they were explicitly terminated. Interrupted checks are not passed checks.

The subsequent legacy-card follow-up `6b956b22-cfe9-4604-9ad9-6c41d26f5f8b` successfully reached real Codex with only the normal request and `Manual acceptance note: preserve search AND semantics.` in its audited canonical/user input. Generated Recall/Enrich stages were omitted, while the stored card was unchanged. Its shell cancellation reproduced the separate-session grandchild leak despite the first native-grace fix; this trial is negative cancellation evidence, not final acceptance.

The Linux descendant snapshot is bounded to processes belonging to the provider when cancellation begins. pidfds avoid signalling recycled PIDs; existing non-Linux process-group cleanup remains unchanged. It is not a new sandbox and does not claim to catch already-daemonised children or children spawned after the snapshot.

### Corrected-runtime acceptance baseline

Backend and matching process host were rebuilt and restarted at `2026-09-16T21:19:31Z`. Runtime source delta fingerprint (sorted changed/new paths under crates/packages/shared/assets, path + NUL + bytes/deletion marker + NUL, SHA-256) is `1c8097564b16bc48456d3f0fd0694c7cf4c47dff488e895e2d2bd34c7071c194`, against the baseline HEAD above. This excludes evolving evidence documentation, not runtime code.

- MCP cancellation rerun `04086ee4-887a-426a-99f6-3fe1c05d8e09`: start 21:19:45Z, terminal `cancelled` 21:20:43Z. The real Codex shell `sh -c 'sleep 120 & wait'` created a separate process group. UI Stop terminated both shell and child sleep (PIDs 201889/201904 absent afterwards); no manual cleanup was needed. The owned legacy block was again removed at dispatch while manual text survived.
- UI follow-up coding runs reused the preserved separate worktrees: A `6328db1b-52d5-475d-b278-7c2215fce3d3` at 21:21:06Z, B `c3024632-7c33-4c4a-9a06-878b1664eb9e` at 21:22:00Z. They overlapped and re-read their Wiki/source, then attempted host finalisation. Their source base remained the isolated `af39363e` snapshot. These runs failed finalisation as described below and are not successful acceptance evidence.

These follow-ups exposed a second pre-existing runtime integration defect and did **not** complete source finalisation. Both wrote to their previous run's draft path despite new identities being present in the frozen launch environment. The host rejected missing current-run drafts and retained source changes. The corresponding `completion-errors/` records remain available; they were not erased or rewritten as successes.

The pinned Codex source's `build_settings_update_items` does not replay arbitrary changed developer instructions into resumed model-visible history (it also documents incomplete configuration diffs). Setting `ThreadResumeParams.developer_instructions` therefore was not sufficient. EVK now uses the supported app-server `thread/inject_items` after a successful resume to reassert supplied developer context, awaiting acknowledgement before chat, Goal, review or compaction proceeds. Resume configuration remains set for full-context reconstruction. There is no extra model turn, no separate Responses API/key path, and no expansion of the Goal objective. Unsupported/failed injection fails closed. Current Memory guidance explicitly supersedes historical run IDs/draft paths.

The transport regression failed before the change (no injection request), then passed for Code, Plan, Goal, Plan with Goal and injection-error cases. Existing OpenWiki preflight fixtures were extended to acknowledge the new public request without relaxing writer/Reviewer gates. New acceptance workspaces were used below; the failed drafts were not copied/fabricated to manufacture a successful Manifest.

### Fresh acceptance after resumed-context repair

Backend/process-host development build passed and backend restarted at `2026-09-17T00:12:26Z`. Runtime delta fingerprint is `5d94a7b0adc37fa8f225b806f1d5268c4d2a9cd98719ee263fcb3888b71bf5ee` (same algorithm and HEAD as above). `pnpm run format`, `pnpm run check`, `pnpm run lint` passed on this revision. The Codex suite passed 99 tests with six explicit fixture ignores; the full root workspace rerun also passed, as recorded below. Earlier frontend tests remained applicable until the subsequent display-only change.

The existing isolated cards now have new UI-created attempts, both based on the unchanged `af39363e` test target. Earlier failed finalisations and their source/drafts/errors remain untouched.

| Trial | Workspace / Session | AgentRun | Observation |
| --- | --- | --- | --- |
| A initial legacy-card input and Stop | `254ba32d-c916-4e69-a5d4-1dcbabf60562` / `5e473d54-eb5a-4d7a-8af2-cbc33b646269` | `2ca9420d-56dc-455a-a68d-7d728875b392` | Wiki read, legacy stages omitted and manual note preserved; UI Stop reached `cancelled`, shell/sleep PIDs 265060/265079 disappeared without manual cleanup |
| B initial read-only preparation | `bd485982-2257-48bb-89b9-f5fcca5e4c1c` / `c16ba06b-6e83-4591-acfc-a51ea2ada028` | `978daf20-4c08-4db8-8b29-95d186b29e05` | Read quickstart and Workspace inspection, reported original identity, no source/draft/Wiki changes |
| A real coding follow-up | Same A Session | `42784387-f7b7-4596-8836-d7ebea25dc6b` | 00:16:54Z–00:25:26Z, overlaps B; host source commit and Manifest succeeded |
| B real coding follow-up | Same B Session | `e02961b0-1575-42ac-a385-19440cc11da5` | 00:15:46Z–00:22:36Z, overlaps A; host source commit and Manifest succeeded |

Codex's own persisted response items confirm current developer context was injected at 00:15:52Z for B and 00:16:57Z for A, with the respective new run IDs, in the same native threads as their initial turns. B finished at 00:22:36Z; EVK finalised source commit `52c99494aec4a0de5707c9cb701eb3a61a9f7a4e` and immutable event `e02961b0-1575-42ac-a385-19440cc11da5` at 00:22:37Z. Its 25 targeted tests, formatting and lint passed; the pre-fix five failing cases are recorded separately. Only its two source/test files changed. Target remained `af39363e`, no receipt or canonical Wiki change occurred before integration. No current-run completion error was produced.

`cargo test --workspace --no-fail-fast --quiet` completed successfully on this runtime revision: notably 267 executor tests (six explicit ignores), 57 local-deployment tests, 134 server tests and 110 services tests (one explicit ignore), plus the remaining root unit/integration/doc tests. Slow integration fixtures finished successfully rather than being skipped.

A completed at 00:25:26Z, with 24 targeted tests passing after 12 expected red cases, plus changed-file format/lint. EVK created source commit `a4714dc845cda651f3659a3010f386197ca04a0e` and event `42784387-f7b7-4596-8836-d7ebea25dc6b`; its canonical Wiki remained untouched. UI Git → Merge → confirmation squash-integrated A as `e9bfaebf260735a930e107519053e9121e70a3b5`. Explicit integration record `3e437dd4-b260-4425-82d0-dc5aed8a7da2` identifies only A's event.

Automatic Sync A started at 00:26:25Z, AgentRun `e90f5ca0-3e3e-4912-b9fd-dfab893ff777`, Session `78a351aa-d2b5-4a2c-a0fc-448dca5bbfd5`, maintenance Workspace `a8602cdb-7870-4d4e-9781-16c87934545b`. Its frozen source is `e9bfaebf`, manifest digest `29be9adc6691f69e8d6bab732fc4cc6fad59709077298cdfa0798bfef764996a`, and its two chunks contain only A's Manifest and A's Memory. B's independently finalised event was still unintegrated, with no receipt, throughout A's publication below.

The actual Sync A audit records successful reads and SHA-256 checks of the manifest and both chunks, followed by the writer's commentary identifying NFC as the change and investigating the relevant source/tests/Viewer wiring. Independent host-side recomputation matched the bound manifest and every chunk. `pnpm run generate-types:check` also passed on the corrected runtime; generated schemas remain unchanged.

Sync A's registered root MCP operation `bc6d635f-d4f5-4452-a086-5dde8285cd64` completed through `openwiki_finish(complete)`. EVK verified/restored/published at 00:34:15Z (approximately 7m51s from AgentRun creation). Wiki commit `0e7b62e14717ad9ef01319c77dc63652c4c8408f` changes only the inspection page, its Claims and two OpenWiki metadata files. Setup instruction changes were restored; maintenance worktree is clean. Receipt A is `updated`, B still has no receipt. The target's source difference contains only A's two files: B's behaviour and rationale are absent from this publication. Runtime/development `openwiki/` remains unchanged.

Content inspection confirms a canonical search subsection explains both-side NFC, AND matching across fields within a page, whitespace-only queries, non-mutation, and the NFKC/accent-folding exclusions with implementation/test links. This Wiki describes the isolated source snapshot, which still contains legacy reader code; it is not a claim that legacy maintenance remains enabled in the changed EVK runtime.

### Sequential B integration and automatic update

The UI required Rebase because A's source and Wiki commits advanced the target. MCP selected Rebase against the same isolated target, then Merge and its confirmation. B's rebased source commit is `3ea2649c`; squash integration is `34e2020f213d87e8d8917496e2c94d46e1743056`. Integration record `69f66562-b59d-4385-908c-bb3be417b047` explicitly retains B's original event `e02961b0-1575-42ac-a385-19440cc11da5`, rather than relying on ancestry of its pre-rebase source commit. No Wiki files were copied into B independently of source.

Automatic Sync B began at 00:39:44Z: AgentRun `d84fffe4-a2f3-4ad2-9ca4-1f98adbb619e`, Session `d4d5b1fa-5e23-4a88-93f1-ede99b0996f0`, maintenance Workspace `d26fa5ca-eea4-436c-bbf5-5867efa86a3a`, attempt `5f6eb6b3-cb47-49cb-be03-41cc6939da04`. Its frozen source is `34e2020f`, containing A's published Wiki. Manifest digest `46a7227b358d01de2ed6236ec6cfcb26f3a2f4508c92827d2f1b339d19c41840` and both chunk hashes match the host-bound input. Only B's unacknowledged event and B's Memory are included. The actual audit/UI commentary confirms complete chunk reads, digest checking and source/caller research; this is more than prompt delivery alone.

During B's run, the sidebar convergence defect above was confirmed. Its repair changes only the summary response/display, uses the existing 15-second polling interval, and adds neither a database query nor a runtime control transition. No agent launch, semantic input, source finalisation, completion proof or publication code changes after runtime fingerprint `5d94a7b0…`. The A/B generation evidence remains applicable under the specification's display-only exception; manual Sync and subsequent-agent/Viewer acceptance used the rebuilt final server below. The response remains backwards-compatible at the reader boundary through the existing stream fallback. Polling is eventual display convergence, not instantaneous lifecycle notification.

Sync B passed registered `openwiki_finish(complete)` and host verification/publication at `2026-09-17T00:47:06.370740220Z` (approximately 7m22s). Wiki-only commit `a545913987dfd455ca2447ef28666af9cbd7454a` changes the same inspection page, its Claims and two OpenWiki metadata files. Receipt B is `updated`, ownership is cleared, and setup files/worktree are clean. Receipt A remains unchanged.

The B prose diff adds filename fallback and view-kind precedence without altering A's search subsection. It explains nonempty case-insensitive Dockerfile suffixes, backend-language precedence, binary/unsupported handling and the absence of content sniffing. It independently distinguishes `Dockerfile.yaml` with a null-language unit fixture from the actual backend's `yaml` response, and language labels from unimplemented syntax highlighting. These are useful qualifications, not blind copies of the Manifest. The writer executed nine classification examples and checked 89 source references. The parent inspected the diff and relevant source/tests; this is a scoped integration/content check, not a whole-Wiki quality or token-efficiency comparison.

MCP opened B's published maintenance workspace → Wiki index → quickstart → its Workspace inspection link → the page's Workspace concept link. The main article and sidebar display repository `repository`, `Current workspace · openwiki/` and branch `vk/d26f-openwiki-reposit`. Browser text shows both the new Dockerfile subsection and preserved NFC subsection. An inline MCP screenshot records the actual article/tree. Final-revision reload and the remaining checks are recorded below.

### Final display revision and applicable gates

The final backend restarted at `2026-09-17T00:53:02Z` with no active AgentRuns. Runtime delta fingerprint is `2d4c5b074d112b515dbb09f142324e75cbc75d7f1743de7042d57a24a58bccb1`; development server SHA-256 is `3baee4e0f15a2eeaf8ccbce8936e80e1f68c93048e6307840e6e92bf8fe57cd1`. Relative to A/B's `5d94a7b0…` runtime, only Workspace summary presentation, generated response typing and its four frontend tests changed. Generation, semantic handoff, proof and publication are identical. The full root test run remains applicable to those unchanged modules; server tests and frontend/gates were rerun for the display change.

- `pnpm run format`, `pnpm run check`, `pnpm run lint`: passed after the display repair.
- `pnpm --filter @vibe/web-core exec vitest run`: 59 files / 335 tests passed, including the four sidebar tests. The first test harness attempt required an absent DOM dependency; it was replaced with the repository's existing server-rendered React testing pattern without adding a dependency. The resulting three intended red cases were confirmed before fixing the display.
- `cargo test -p server --quiet`: 134 unit tests and 38 integration tests passed. Root workspace tests from the same execution implementation passed as recorded above; no private remote backend gate was attempted.
- `pnpm run generate-types`: regenerated from Rust. A concurrent consistency check during the final field-order adjustment saw the older output and failed; it was rerun after generation completed. `pnpm run generate-types:check` and `git diff --check` then passed. Generated schemas have no diff.
- `cargo build -p server --bin server -p local-deployment --bin agent-process-host`: passed. No release/package build or publication was performed.
- After the evidence-document update, `pnpm run format`, `git diff --check` and `pnpm run generate-types:check` passed again. Runtime delta and server binary hashes remained exactly those above; port 4020 returned HTTP 200. No further runtime changes invalidate the recorded acceptance.

Local issue links also confirm A's final workspace belongs to card `97a7e708-949d-4b21-b2ba-4baec99faf50` and B's to `1a1f561d-6eab-44d2-abc6-f9692a1f585d`. Their existing nullable legacy `task_id` stays null; modern Issue UUIDs were not fabricated as legacy Task identities. Repository/workspace/run identities and semantic intent are frozen in their Manifests, and the existing local workspace-link table retains card navigation.

### Final-server manual Sync: content unchanged, metadata published

MCP opened the isolated project's working-location settings, expanded Local execution configuration and selected **Sync Wiki**. AgentRun `8645a1ed-cfc6-43a5-a47f-9bf7850ac51c`, Session `7389ca5f-bf84-422e-8d0e-0cfb64f61fd6`, maintenance Workspace `490359b7-55f1-474b-92a3-338f16e3a493` and attempt `3c754996-7f16-42bf-a913-fff5e5642093` started at `00:54:09.541Z` on the final server. Source SHA was `a545913987dfd455ca2447ef28666af9cbd7454a`, including both integrated changes and both published knowledge updates.

The input manifest digest is `3434e021d9192a6829a35f4daea18aba8e6df5f772b952236acd5196e073f009`. Selected events and chunks are empty: both integrated events already have receipts. The audit confirms an actual manifest read and registered OpenWiki operation `6502453a-8878-4496-b82b-e2d6cd6d7bf1`. Upstream returned an active update, not a strict `begin` no-op. After checking the existing code and Wiki, the agent submitted an empty page plan, reached completed page processing and received `openwiki_finish(complete)`.

Agent execution ended at `00:57:47.093877Z`; verified publication completed at `00:57:55.167363215Z`, approximately 3m46s after creation. Commit `57994964d276ba2d9a56075935b7491ac90f9bb5` changes only `openwiki/.last-update.json` and `openwiki/.page-manifest.json`. The publication record correctly has `no_op: false`, because those metadata changes form a Git commit. This is a successful **content-no-change** update, not evidence that this paid run exercised the separate strict begin-noop branch. No prose was invented to force a change. A/B receipts were unchanged, repository status became `current`, ownership cleared and the restored maintenance worktree was clean.

While the browser remained on reader C's workspace, the sidebar moved this Sync out of running without a browser reload and kept C in running until its own completion. This directly rechecks the repaired sidebar path, not just its API/database result.

### Subsequent normal agent reads the integrated Wiki

MCP created card `7e92255c-1fc6-4b5a-8759-21dea4d14f5b` (LOCAL-3), **Update acceptance C: read published Wiki (no source changes)**. Its ordinary Code-mode workspace was created from the same test target at `a545913987dfd455ca2447ef28666af9cbd7454a`, without selecting a Wiki Pipeline. It ran in parallel with the manual metadata-only Sync; its source and Wiki prose match that final target.

- Workspace `4dfcdeb8-099c-4006-813b-cebde7afbf8f`, branch `vk/4dfc-update-acceptanc`.
- Session `d3680cb1-ec2d-4b37-bca7-416fb175b3be`, AgentRun `953dd9c1-fb63-4ec6-a490-99c610e2d1bf`, attempt `abc8ff4f-6d82-464d-8b88-b3d1a32cf4d2`.
- `00:55:15.186Z` to `succeeded` at `00:58:40.894901815Z`, approximately 3m26s.
- Checkout: `/var/tmp/vibe-kanban-dev/worktrees/4dfc-update-acceptanc/repository`.

Native Audit records actual reads of `openwiki/quickstart.md` and `openwiki/operations/workspace-inspection.md`, bounded rereads, then the relevant implementation/tests/backend renderer. The final Japanese answer correctly distinguishes NFC from NFKC/accent stripping, per-page AND matching from cross-page matching, Dockerfile suffix fallback from authoritative backend language, and a language label from syntax highlighting. It explicitly distinguishes reading test definitions from executing tests or proving browser/remote behaviour.

Only this reader's Workspace Memory changed. The checkout has no source/Wiki difference, source commit, semantic draft, immutable change event or completion error. This confirms a normal read-only request does not create an artificial source change or maintenance event. It also confirms the next agent can discover both A and B knowledge through normal checked-out Wiki access, rather than receiving hand-copied Wiki files.

### Final Viewer verification and audit locations

On the final server, MCP opened manual Sync's workspace at `/workspaces/490359b7-55f1-474b-92a3-338f16e3a493`, selected **Open Wiki**, then opened the index and Workspace inspection page. It clicked **Reload Wiki files**, performed a full browser reload, searched for `Dockerfile` and reopened the affected page. Both **Dockerfile の補完条件** and **Wiki 検索の一致条件** / NFC remained visible. Body links to **Workspace** and then **Session** opened the corresponding concept pages within the Wiki reader. The browser was finally left on the inspection page.

The header identifies `repository`, `Current workspace · openwiki/ · Read-only`, branch `vk/4903-openwiki-reposit` and the selected page. Independent Git inspection confirms that this maintenance checkout and the isolated target are clean at publication `57994964…`. This is not a stale prior run's article. MCP snapshots, successful click/reload/search records and inline screenshots in the implementation conversation are the UI evidence. File-path screenshot export was unavailable at the MCP server boundary; no screenshot file is claimed.

Native Audit is retained under `dev_assets/runtime/native-audit/v1/sessions/<session-prefix>/<session-id>/agent-runs/<run-id>/attempts/<attempt-id>/frames.jsonl`, with its existing manifest/integrity records. Session prefix is the first two characters of its UUID. A's automatic Sync attempt is `ebc2add9-afc7-4e60-af86-936652f3ebc1`; the other final maintenance/reader attempts are identified above. Root registered-MCP messages and completed command output, rather than terminal prose alone, provide the access/protocol evidence.

The isolated repository's persistent semantic records are under `/var/tmp/vibe-kanban-dev/shared/repository-006e42c0-2783-459c-be43-794af1cd941a/persistent/knowledge/`: existing events, integrations, receipts, publication/state, Workspace Memory and the new `sync-inputs/<workspace-id>/manifest.json` with numbered chunks. The actual writers read the full frozen manifests/chunks and checked their digests; EVK checked the same bound input before dispatch/publication. No receipts or audits were manually edited.

## Acceptance matrix

| Specification criterion | Evidence and verified scope |
| --- | --- |
| 1. Legacy activation retired, data preserved | Seed/UI/endpoint removal, precise launch/Skill filtering tests, real unchanged legacy-card initial/follow-up input; stored/manual text and both Wiki directories on the development branch untouched |
| 2. Normal read-only Wiki, Memory, Manifest and current identity | Real A/B same-session follow-ups with acknowledged current instructions; separate Memory/current-run drafts, source commits/events; C reads without source finalisation. Existing continuation/compaction dispatch contracts and tests retained; no forced real compaction experiment |
| 3. Integrated semantic intent actually used | A and B audit reads plus host-bound hashes; source/test investigation and matching focused prose. B's event absent from A input/publication; both later retained |
| 4. Parallel development, one writer and safe selection | Overlapping real A/B development, sequential UI rebase/merge/automatic Sync; existing locking, batch, retry, source-drift and outbox crash/idempotency tests pass |
| 5. Full proof and safe publication | Real A/B/manual registered root operations, successful host proof, restoration, Wiki-only commits and receipts. Automated prior-attempt, unfinished/unknown operation, begin-noop, no-change Refine and cleanup gates pass |
| 6. Local quality gates and review | Full root Rust tests, final server tests, 335 frontend tests, format/check/lint/type consistency and development builds passed as scoped above; confirmed findings repaired |
| 7. Required real UI flow | A/B development → Manifests → sequential integration → automatic Sync/publication; final manual Sync, Viewer links/search/reloads and new normal reader C passed through MCP |
| 8. Related defects repaired and rechecked | Ledger records failing evidence, cause, repairs, red/green tests and browser rechecks. New A/B acceptance followed execution repairs; only the explicitly documented display-only change reuses their unchanged generation/proof evidence |
| 9. Documentation and data protection | This record and both user guides updated; original branch/HEAD and user files preserved, no implementation commit/stash/push/release, only authorised isolated source and Wiki commits created |

## Principal implementation files and limits

- `crates/executors/src/legacy_wiki.rs`, Codex/provider adapters and `crates/local-deployment/src/agent_run_port.rs`: precise retired-input handling and provider-independent guidance, including frozen launches.
- `crates/executors/src/executors/codex/client.rs`: acknowledged current developer context on resumed sessions; existing app-server transport, not an API-key integration.
- `crates/services/src/services/openwiki/sync_input.rs`, `openwiki.rs` and `crates/utils/src/repository_memory.rs`: frozen semantic input, identity/digests and existing shared-store bounds.
- `crates/services/src/services/openwiki/completion.rs`, server OpenWiki completion and Bootstrap runtime: shared all-attempt proof, safe exit/restore/publication.
- `crates/utils/src/process.rs` and `crates/local-deployment/src/process_host.rs`: bounded cancellation of captured Linux descendants, retaining existing group cleanup.
- Wiki routes/service, frontend controller/panel/surfaces and API/types: OpenWiki-only reader and removal of retired write controls. Existing Markdown/Mermaid/navigation helpers remain.
- Workspace summary route and `useWorkspaces.ts`: eventual sidebar convergence through existing polling. Generic Workspace/Workflow/Arena and shared Skill selection remain.

There are no new package dependencies, DB migrations, OpenWiki patches, schedulers or artifact stores. OpenWiki remains pinned at 0.5.1; the supplied OpenWiki Skill was read to preserve its public resumable operation contract, not copied/forked. The test script is now `pnpm --filter @vibe/web-core test:wiki`; generated TypeScript was regenerated from Rust.

Important limits remain explicit:

- The normal agent contract and Git publication guard do not create a new OS security boundary. Ordinary agents must not write canonical Wiki; maintenance remains the only accepted publisher. Historical/user `.llm-wiki/` files and old preset files on disk remain data, not automatically migrated OpenWiki.
- Memory is workspace-scoped working context and may describe unintegrated work; it is not canonical truth. Manifest selection uses integration evidence, and update instructions require verification against the frozen integrated source. Chunk completeness does not prove semantic understanding.
- A workspace reads its own Git snapshot, not a live canonical Wiki mount. A new source-and-Wiki checkout (as in C), or legitimate integration/rebase, is required to read newer published knowledge. Independent base-branch Viewer browsing is not introduced.
- Public strict begin-noop, adverse attempts, tampering, source drift and contention are exercised deterministically in automated tests, not forced through extra paid trials. The real manual Sync was metadata-only publication, as recorded above. No claim of forced real compaction, whole-Wiki completeness, quality improvement or token savings is made.
- Linux pidfd cancellation covers captured descendants, not processes already reparented before capture or arbitrary later forks; non-Linux retains existing process-group behaviour. Sidebar polling converges at its existing interval, not immediately.
- Failed preliminary runs and their completion diagnostics are deliberately retained. Their error records do not describe the successful corrected A/B runs. An unrelated old pending-cancellation warning for run `e315d933…` predates this trial and was not manipulated. No unrelated runtime/performance redesign was attempted.
- Separate private remote backend Cargo validation remains outside this local change's required gates. Remote-web checking and remote formatting passed. Explicit ignored tests are not counted as passing tests.

## Retained assets and handover

The development branch remains `feat/llm-wiki-updates-flow` at `9ad6b9ccf7ab535e8d416eb12ed767619d9197ed` with implementation changes uncommitted. Its `openwiki/` and `.llm-wiki/` have no diff, and the original two untracked user files remain. The isolated target is clean at `57994964d276ba2d9a56075935b7491ac90f9bb5`. No source or Wiki was pushed.

The development frontend remains available on `0.0.0.0:4020` (backend 4021). Open **OpenWiki Update Acceptance 20260917** to inspect retained cards, attempts and maintenance workspaces. The final article is also reachable through the workspace path above. Test branches, source/Wiki commits, worktrees, shared inputs and audit history are retained for inspection; none were deleted after acceptance. The empty screenshot-export directories are not evidence files.

After reviewing the results, you can archive the isolated test cards/workspaces through normal EVK operations. Shared persistent memory/audit retention is separate from workspace deletion; do not assume archiving removes those records. Any later deletion of the isolated clone, test branches or shared history should be a separate explicitly scoped cleanup, not part of this implementation handover.
