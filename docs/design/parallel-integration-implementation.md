---
title: "Parallel context and formal Integration implementation"
description: "Implementation checkpoints, safety decisions and acceptance evidence for parallel development and local Integration."
---

## Baseline and authorised scope

Implementation started on 17 September 2026 on `feat/parallel-context-bootstrap`.
HEAD, local main and the local origin/main reference were
`4f916e6b655324bc9ef0fdc1f25e0d73febc3d04` (commit time
`2026-09-17T21:20:05+09:00`). Their commit differences were empty. Remote
freshness was not re-established during this check. No tracked changes existed.
Four pre-existing, untracked `docs/design/*:Zone.Identifier` files are retained.
The earlier review's `9ad6b9cc` baseline is historical, not this implementation.

The implementation goal resolves the scope choices in specifications 01 and 02:

- Formal Integration is a local Board operation, for one registered repository
  and one local target, with one explicitly adopted Workspace per Card.
- Reject Cards with other unintegrated repository results. Preserve ordinary
  multi-repository development and repository-specific context.
- One EVK service, with parallel Workspaces. Canonical Git common-directory
  identity prevents duplicate registrations bypassing publication exclusion.
- Cooperative local execution within existing permissions. Worktrees and
  prompt restrictions are not OS-level access control.
- Apply context to fresh Sessions within an existing Workspace as well as its
  first Session; preserve CURRENT identity on continuations.
- Chrome DevTools MCP acceptance with authenticated Codex/OpenWiki and isolated
  test targets is mandatory. No implementation commits, push, release or PR.

## Existing owners and implementation plan

| Responsibility | Existing owner and planned adaptation |
| --- | --- |
| Saved context | `cardContext.ts`: retain marker/idempotency and edited historical text; extend new Shared directories guidance |
| Runtime context | `LocalAgentRunPort::execution_env`, provider adapters and Codex resume: small current header and explicit fresh-session bootstrap; no peer transcript injection |
| Discovery | Existing Workspace/WorkspaceRepo/Session/AgentRun and local Issue links, plus RepositoryMemoryStore immutable events; read-only observations, timestamps and per-record errors |
| Manifest authority | Retain strict readers/publication; add a tolerant discovery view separately. Never publish peer private Workspace Memory |
| Preview | Existing Git object reading/worktree management, fixed source OIDs, temporary test environment; no source/target mutation or ordinary source finalisation |
| Formal execution | Existing Orchestration plan/outbox/inbox and AgentRun dispatch. Add Integration-specific durable state and reservations, not another scheduler or DAG |
| Source/target | Existing Git crate: exact-R/expected-B promotion, canonical storage guard shared with manual merge and Wiki publication; no squash/source rewrite |
| Validation | Existing Script/ExecutionProcess and logs, host-frozen plan and before/after OIDs; invalidate on tracked/untracked source changes |
| Completion | Conditional local Issue updates with frozen requirement/link/status expectations and durable per-Card outcomes; never link one Workspace to every Card |
| Recovery/cleanup | Durable publish intent and business reservations, independent of disposable dispatcher leases; no blind reset, remerge or reservation expiry |
| Wiki update | Existing Memory integrations/pending events/Sync, with explicit frozen source event set and integration-specific semantic draft |
| UI | Existing local Board, dialogs, Workspace/Session views and Viewer; explicit selection, progress, cancellation and separate Git/Done/Wiki outcomes |

The current manual `merge_workspace` calls `merge_changes`, which squashes and
rewrites the source ref, then records the merge and may archive the source. It
cannot publish a previously validated R. Normal terminal handling finalises
Memory before consuming queued work; preview and formal Integration must be
distinguished at this boundary without disabling normal development finalisation.
Orchestration dispatcher leases are not durable business reservations.

## Checkpoints

1. Implemented: context, paginated manifest/activity discovery and fresh Session
   input. Follow-ups retain CURRENT identity without repeating saved policy.
2. Implemented: pinned Git object reads and detached combination trials. Trials
   live beside, not inside, the ordinary orphan-cleanup workspace root.
3. Implemented and exercised through the UI: persistent reservations, one
   Orchestration Agent node, host Script validation, exact publication,
   conditional Done receipts, cancellation and publication reconciliation.
4. Implemented and exercised through real Codex/OpenWiki: fixed source event IDs plus
   integration-specific semantics enter the existing Memory integration outbox.
5. Completed acceptance audit: the corrected-binary A/B/C run, automatic Wiki
   publication, Viewer and subsequent reader passed. Additional UI trials prove
   dirty-source and secondary-repository rejection, queued cancellation and a
   host-validated no-op. The requirement matrix below distinguishes deterministic
   tests, live operations, sampled decisions and inspected control flow.

All checkpoints are complete within the authorised local/cooperative scope.
Semantic fixture results are not guarantees for arbitrary repositories. The
first checkpoints alone were not treated as completion; the final gates and
MCP evidence below are the acceptance record.

## Early environment checks

- Chrome initially had no debugger listener. Started the existing Chrome wrapper
  on port 9222; Chrome DevTools MCP successfully listed the EVK page and read its
  accessibility snapshot on localhost:4020. No user run was stopped.
- Existing Vite listens on `0.0.0.0:4020`; the backend executable and cwd resolve
  to this checkout. It is the baseline binary, not evidence for modified code.
- `codex login status`: authenticated through ChatGPT (no credentials printed).
- Installed OpenWiki is 0.5.1, confirmed by its CLI help. A non-TTY invocation
  also prints an Ink raw-mode diagnostic; host-driven readiness must be checked
  through the existing adapter before acceptance, not inferred from this help.
- Docker CLI exists; this container has no `/var/run/docker.sock`. The installed
  supported OpenWiki CLI path is available; no Docker-in-Docker or new credentials
  are introduced merely to obtain a daemon.

## Verification and defect ledger

The following checks have completed during implementation. Later changes require
affected checks to be repeated; this is not a final all-green release statement.

| Check | Observed result |
| --- | --- |
| `cargo test -p git publication --lib` | 7 passed, including ignored-file preservation and recovery |
| `cargo test -p db models::integration --lib` | 9 passed, including cancellation versus late Agent admission |
| `cargo test -p server --lib routes::integrations` | 6 passed, including real exact Git publication followed by an injected DB receipt failure, modern Board repository discovery and storage-identity target queue reservations |
| `cargo test -p services parallel_context --lib` | 2 passed, including manifestless activity, current Card policy and fresh-session instructions |
| `cargo test -p git snapshot --lib` | 1 passed: full OIDs, no checkout, symlink/submodule boundaries, traversal and bounded reads |
| Detached combination worktree test | Passed; source and peer refs/checkouts remain unchanged |
| `pnpm --filter @vibe/web-core exec vitest run src/shared/lib/integrationApi.test.ts src/features/pipeline/model/cardContext.test.ts` | 13 passed, including cancellation/Done presentation |
| `pnpm run generate-types` | Passed; generated files, never hand-edited |
| `pnpm run check` | Passed after the latest runtime and UI additions (local-web, remote-web, web-core, UI and root Rust workspace) |
| `pnpm run format` | Passed after the latest runtime and UI additions; documentation updates continue |
| `pnpm run lint` | Passed after fixing three new Clippy conversion/borrow warnings |
| `cargo test --workspace --no-fail-fast` | Passed for the root workspace; remote backend remains outside the agreed scope. One pre-existing doctest ignored. |

An additional, non-standard web-core ESLint invocation using local-web's config
reported existing cross-feature-import and unused-parameter findings, plus a
test file excluded by that config's TypeScript project. It is not a substitute
for the passing repository lint command. The new IntegrationPanel and runtime
API implementation had no findings in that invocation. Subsequent acceptance
fixes must rerun the affected gates before final completion.

Confirmed defects found and addressed in this implementation:

- Exact checkout could overwrite ignored local files. A failing real Git test
  reproduced it. Collision checks now precede checkout, and that test passes.
- Canonical Agent/Script completion precedes final process teardown. Validation
  now waits for process settlement and the persisted after-HEAD, rather than
  inventing that evidence or treating an ordinary hand-off as failure.
- A cancelled preparation could be redelivered after restart. Startup fences it
  before the generic outbox starts, and DB admission also checks phase/cancel
  identity. Reservations remain until writers are confirmed stopped.
- A linked Card with no description could resurrect an old Shared directories
  policy. Current empty policy now wins over the previous initial prompt.
- The new Integration prompt initially read the port file as plain text, although
  it contains JSON. It now uses the existing port-file reader; the dispatched
  URL/identity has a regression test.
- Card completion and its receipt now commit together. A later Integration's
  reservation and a user's reopened/edited Card cannot be overwritten by replay.

## Isolated MCP acceptance assets

The following is the initial fixture record; later subsections record formal
Integration, update and subsequent-reader outcomes on specific revisions:

- Root: `/var/tmp/evk-parallel-acceptance-9NGp68/repository`.
- Target: `test/parallel-integration-v1` (a separate disposable repository).
- Initial source: `d265ae7973aaed7aa195efc22ce634c99714679a`.
- Baseline `npm test`: 2 passed; `npm run check`: passed.
- Project created through Chrome MCP:
  `da9d7cbe-df8d-4b36-af6d-a71b9024c46c`.
- The repository has an ownership contract and an explicitly historical proposal;
  independent pagination/filtering tasks will share one implementation entry.
  This tests integration behaviour, not arbitrary-repository semantic accuracy.

The existing user repositories, Wiki, history and development branch have not
received trial commits. The isolated test commit is explicitly authorised by the
goal. Retain the trial repository and future run evidence for inspection.

### First live trial

The development backend was rebuilt and restarted only after confirming no
active AgentRun or Script. A SQLite backup is retained outside the repository at
`/var/tmp/evk-parallel-acceptance-9NGp68/pre-feature-db.sqlite`. Migration
`20260918000000` applied successfully. The server executable SHA-256 is
`a445f950aa86961f3928a521d5713ee4afe1abf1b8abe055a6bfe2cada759661`.
Subsequent changes before this trial were comments, generated type comments and
formatting, not runtime behaviour. Vite also required restarting: a direct
module response proved it had retained old Card-context text. Existing saved
Card instructions remain unchanged; runtime bootstrap still applies to them.

Through Chrome MCP, saved the test repository's memory settings with the explicit
test target, then clicked Initialize Wiki. Bootstrap
`93a33fdd-9e54-48dc-9bc7-9ce38612e31f` succeeded via Generate/Review/PASS/Publish
in Workspace `23df2bc6-0eeb-41af-812f-99e1b12898b0`. It published only `openwiki/`
as `01b6e928c2b4763e2cb778a70ae0ded9f3bbd65d`. Both development Workspaces
started from this same published baseline:

| Identity | A: pagination | B: category filter |
| --- | --- | --- |
| Card | `ff7a6a35-41ca-4a8e-a3a5-2b8090dcfdc7` | `4705dbf0-0de5-4786-a313-d001bcc84c1e` |
| Workspace | `99b51376-047a-42c5-a879-0b8bfb78a270` | `5d554ed1-271c-42a3-9e6a-497d92aa9ec5` |
| Session | `8c9c6e04-2670-47ed-8dbc-68c40403e157` | `2b5eb7d6-9c2c-4881-a645-b6720743c340` |
| Initial AgentRun | `10fa15c0-709a-45dd-bb65-8aa4fa7d39a0` | `f1d3b5f1-53d7-4ce9-9d97-e61bad02bf82` |

Native Audit provides actual successful command results, not just prompt text:
A sequences 54/56 read Wiki and repository discovery; B sequences 66/68/74 read
Wiki, discovery including A's active manifestless Workspace, and the pinned
source API. Audit paths remain discoverable by these AgentRun IDs in
`native_audit_streams`. MCP snapshots are retained under ignored
`dev_assets/parallel-integration-acceptance/`; the tool's direct file-save
permission rejected paths, so the returned snapshot was stored unchanged using
the normal file-edit tool. No tool permission was broadened.

The initial Wiki preserves the ownership contract and labels the HTTP proposal
unimplemented. It also contains two upstream broken Markdown-source line-anchor
warnings. This is recorded as a generated-content limitation, not silently
hand-repaired or a claim of perfect Wiki quality. Later update/Viewer acceptance
must distinguish source-document anchors from the Wiki's own navigation.

### Fresh-session detached combination and unselected source

A fresh Session `e3090c4f-5f9a-4455-a1f8-e36aaab0503a`, AgentRun
`1c995a1c-5355-4015-964b-7c6cebacf839`, was started from the ordinary Workspace
session menu through Chrome MCP. It actually read discovery (audit sequence 72),
the checked-out Wiki (76), both immutable manifests (78) and fixed Git snapshots
(80/91), then called the production preview route (170). Its retained detached
trial is `/var/tmp/vibe-kanban-dev/worktrees-previews/preview-84fd798b-8a1f-462c-a4d6-9af586dea5f3`.
After trial-only conflict resolution it ran 22 tests (15 original and seven
combination tests), all passing with no skips, plus `npm run check`. The result
is a tested, uncommitted trial, not a published Integration. The ordinary
finaliser recorded completed-without-change, not a trial source event.

Verified unchanged A `0782b5f4666ca5a75a277ec368d778b17ff82191`, B
`53dd34ea74554efbe5d130f97af329d2e2b9cc46`, and target `01b6e928...`, clean A/B
checkouts and Todo status. C was created through MCP as an explicitly unselected
counter feature: Card `135bd188-28c8-4358-8371-137e00f49922`, Workspace
`88c13289-e027-4a87-95bd-11111f725c39`, AgentRun
`b8803f88-8012-4323-a3be-53d39895073a`, source
`611bd41922a7e17894af956d040aa87c0957ea1a`. Its independent immutable event
allows the final update to prove exclusion, rather than testing an empty set of
unselected work.

### Acceptance finding: modern local Board repository discovery

The first attempt to open Formal Integration showed no repositories. The API
returned an empty list while Cards A/B/C had valid WorkspaceRepo associations.
Confirmed cause: this new endpoint and admission check used only the legacy
`project_repos` table. The modern UI saves project working defaults in scratch
and creates repository associations on Workspaces; it does not populate that
legacy table. No user DB associations were fabricated to make the trial pass.

Regression coverage now reproduces a real local Board and linked Workspaces
without legacy bindings. The intended correction shares one repository lookup
between the selector and submission, retains legacy associations, deduplicates
multiple Workspaces and observes current Issue/Workspace links. The reproduction
snapshot is `dev_assets/parallel-integration-acceptance/03-empty-integration-repository-reproduction.txt`.
The regression failed before the correction (empty candidates rather than the
linked repository), then passed along with the other five Integration tests.
`pnpm run check`, `pnpm run lint` and the development server build passed after
the server correction. Restarted only after confirming zero active Agents and
Scripts. The v2 executable SHA-256 is
`b336d868525026c00f0491a83ed96fb3f971af2a1db5289edd004041d8d95417`.
The same MCP operation now lists the correct repository and accepts an explicit
A/B selection; no database association was added manually.

### Formal Integration v2: published, Wiki reconciled

Started from the Board through Chrome MCP at `2026-09-17T15:29:38Z`:

- Run: `04542f75-ee55-45d0-b2eb-ea26daae844a`.
- Workspace: `c5286775-59f4-4dfe-b9bd-891097b991b2`.
- Session: `ccf7c24d-41b7-4aef-baa2-c69c806d0b84`.
- AgentRun: `1a7fd2a7-bcd9-6c25-d531-ce0ce7ca8990`.
- Frozen B: `01b6e928c2b4763e2cb778a70ae0ded9f3bbd65d`.
- Adopted source OIDs and event IDs are exactly A/B from the first live trial.
  They remain valid immutable test inputs: the v2 correction changes discovery
  and formal admission, not their source, manifests or preview behaviour. No
  formal Integration had been started with the earlier executable.

Audit sequences 54/60/62/67/68 show repository rules, checked-out Wiki, discovery,
both fixed manifests and pinned source reads. Sequence 151 records real merge
conflicts in the dedicated Integration branch, not either source branch.
The Agent produced `d0ff88cba135fece139fca2a413692b034f8a5f5` in its own
branch, retaining both source OIDs as ancestors. The host independently ran
`npm test` (19 tests) and `npm run check` through ExecutionProcesses
`2ab61c26-64cf-4b43-83a3-7d9bc145e8a4` and
`ffcaad05-3a35-4c01-9e33-bd92c74af7e2`. Both completed with exit zero;
persisted before/after HEADs equal R. The checked-out target, index and files
were clean at exact R. Integration succeeded at `15:33:55Z`, about 4m18s after
submission. A/B are Done; C remains Todo. Source branches are unchanged.

Automatic Sync used AgentRun `84b9918c-9368-4978-a7c9-20466ff0abdc`, Session
`483e9143-73d0-4be7-8392-fdbaf31251f6`, Workspace
`247c6a47-4a61-4b13-ae0d-1e7374ec782f`. Audit sequences 61/67/71 show the
bound input manifest, all six chunk digests and actual hint reads. Sequence 144
describes source-confirmed filtering-before-pagination and option validation,
not a verbatim copy of semantic hints. The registered `openwiki_finish` returns
complete at sequence 382. Publication at `15:38:58Z` produced Wiki-only commit
`9851bae08567470248d958225801635389ad4fdd`. The three receipts cover exactly
A, B and the Integration event; C has neither an integration nor a receipt.
No source/test/config file entered this Wiki commit.

The updated contract and quickstart preserve caller ownership, object identity,
stable order and the historical/unimplemented HTTP proposal distinction. They
add category-before-pagination, explicit-undefined validation and source/test
routing. No count feature appears. This checks this small fixture's behaviour,
not general semantic integration accuracy or Wiki completeness.

While A/B held the same target, queued C as a separate cancellation trial via UI
at `15:30:11Z`, then clicked Cancel Integration. Run
`a437f2de-73db-4f48-8f2d-82eebe41a833` became `cancelled` without creating a
Workspace/Session or publishing source. C remains outside the A/B selection.
Evidence: `04-formal-ab-selection-v2.txt` and
`05-cancel-queued-c-v2.txt` under the ignored acceptance directory.

This exposed a new presentation defect: cancelled, unpublished work still said
`Done pending`. The UI now says `Done not applied`, and distinguishes uncertain
publication recovery as `Done awaiting reconciliation`. The API/control plane
is unchanged. Thirteen related Vitests, format, check and lint passed. MCP
confirmed the corrected label (`06-cancelled-done-label-fixed.txt`). The
Agent's pre-validation summary is also explicitly labelled as a proposal so
its historical "host validation pending" sentence does not impersonate current
host state. This display-only correction does not invalidate v2 Agent/proof.

### Subsequent reader and Viewer on the v2 publication

Through MCP created LOCAL-4, Card `867f2a73-182b-451e-bcd9-17acf8eca886`, on
the updated test target. Workspace `54661069-05e7-4efa-bb16-b45e5224adab`,
Session `838fb65e-004b-4773-b22f-2cce0f929b7b`, AgentRun
`6163ddb7-0b63-441e-9c8b-98ed1f7bd614` read discovery (71), the actual Wiki
and its publication metadata (73), relevant manifests (78), source/docs/tests
(79), old A/B pinned snapshots (153) and integration/publication receipts (180).
It correctly distinguished A/B present in its own HEAD from unintegrated C,
and old A/B Workspaces that retained their older source and Wiki despite Done.
It completed at `15:44:40Z` without source or Wiki changes; HEAD remains
`9851bae...`. An exploratory guessed API returned the app shell; the Agent
did not treat it as data and subsequently read the real public records. This
is recorded as exploratory overhead, not proof of an existing endpoint.

MCP opened the Wiki Reader in this Workspace, navigated index → quickstart →
the concept's call-lifecycle anchor, reloaded Wiki files, reloaded the browser,
and reopened the concept successfully. The UI identifies Current workspace,
repository and `vk/5466-acceptance-d-rea`. Evidence files 07–09 under
`dev_assets/parallel-integration-acceptance/` retain the Integration evidence,
article and post-reload views. Generated Wiki was not hand-edited.

### Review finding: disabled Memory after formal publication

The new formal semantic-outbox path inspected an existing store's enablement
only after writing its event/integration records. A disabled repository could
therefore acquire new Memory side effects or fail post-processing because of an
old missing semantic input. A production-helper regression first failed with
`disabled Memory must not acquire new events`; the early disabled return then
passed. Git/Done and frozen semantics remain durable in Integration itself.
The helper is extracted only to exercise this production boundary, not to add
another store or scheduler. Added coverage also checks immutable frozen-event
selection, later/other-workspace exclusion, target identity and strict failures.

This backend correction required a new UI-started formal Integration/Sync trial
on the corrected binary. That v3 trial is recorded below. The v2 evidence above
is retained as its own revision's result. A/B immutable inputs and the successful
Wiki baseline were reused: their earlier generation is unaffected by the
disabled-store branch. The cancelled-run label correction is display-only.

### Corrected-binary A/B/C trial (v3)

The UI explicitly selected A, B and C and their unchanged full source OIDs.
The target remained the isolated `test/parallel-integration-v1`; neither the
implementation branch nor main received trial commits.

| Evidence | Value |
| --- | --- |
| EVK revision | `4f916e6b655324bc9ef0fdc1f25e0d73febc3d04` plus this uncommitted implementation |
| Server executable SHA-256 | `59ca9937f439a33b84e3e602d22dd5bf3b48520781493743737ab015dc92b7fa` |
| Integration | `341eed24-0fca-4801-a96a-3966d2f1b664` |
| Integration Workspace / Session | `a1ccb4fb-0d9b-4a95-a90d-08db2950573b` / `b2bd35ea-ada2-4622-a7a0-91f72e813bc0` |
| AgentRun / attempt | `cc1a25f4-3a81-5d5e-aa6c-d9e584d733db` / `51e53248-7347-99ca-cd14-616ed806bdf6` |
| Start / success (UTC) | `2026-09-17 15:54:33.344` / `15:58:44.560` (about 4m 11s) |
| B | `9851bae08567470248d958225801635389ad4fdd` |
| Exact validated and published R | `07cbc7dacc2baad85edb2b16fc271f6c264f2d7e` |
| Wiki publication | `b16ffa0de86dcde24f5b2d7de3c067a8d29f5ebd` |

All three Cards have durable `done` receipts. There is one Integration Workspace
and Session, not one per source. All selected source OIDs remain unchanged.
A/B were already ancestors of B; the Agent retained them, merged C and added a
count/selection composition regression without weakening existing tests.

Host validation, not the Agent's success statement, executed four commands at R:

- `npm test`: process `553dde95-a0ad-48fc-a3a8-1b3faf8ec43c`.
- `npm run check`: `c58dfc78-92a3-4e9e-ac27-a0bad38e35ba`.
- Direct syntax checks for the counting implementation and tests:
  `cc356b3d-f273-4ba0-9585-4de719b8f565`.
- `git diff --check` from frozen B: `f912f743-8c75-42b5-b0d6-6083646780e7`.

Each required command has exit zero, a persisted `passed` result and the
before/after-HEAD evidence checked by the publication path. The full Node suite
contains 24 tests; no source acceptance tests were skipped or removed.

Automatic Sync ran in Workspace `6e648c8d-18ef-44a5-a971-a6dd88408b28`, Session
`6fa1ebea-5257-45d9-a013-a7b753f4cd8b`, AgentRun
`a87db25c-0458-4149-a3c1-34508371876c`, attempt
`e6372056-ad46-47ed-a3b1-123a21579cb9`. Its real native audit records reading the
frozen manifest, verifying its digest and all four chunks, reading source,
canonical documentation and existing Wiki, and calling registered OpenWiki MCP
`begin → submit_plan → page jobs → finish`. The Sync input digest is
`0eca11e0895d8aafd5884cb1f986c1dedf5213c810ffbd1da9273d72abfa9cde`.
Its pending set is C's event and this Integration's semantic event; A/B events
were already acknowledged by v2, not lost or consumed from a later source.

Sync began at `15:58:54.704Z`; the Agent succeeded at `16:02:02.733Z` and
publication completed at `16:02:06.420Z`. The Wiki-only commit changes eight
`openwiki/` files and no source/tests/configuration. State is `current`, with
source R, the recorded Wiki commit and no error or active maintenance run.

### Final Viewer, subsequent reader and decision samples (18 September)

After the environment interruption, a development rebuild produced the same
v3 server SHA-256. The restarted server listens on 4021 behind Vite on
`0.0.0.0:4020`; Chrome DevTools MCP is connected on 9222. No new production
behaviour was introduced between v3 and these checks. The additional
`integration_owner_disables_normal_commit_without_changing_ordinary_policy`
regression is test-only.

Supplementary evaluator E had lost its host during the environment interruption.
Its UI Stop operation ended as `audit_failed`, explicitly reporting the absent
terminal Native Audit manifest rather than claiming clean cancellation. E is
not counted as a completed evaluation. No user run was stopped or audit repaired.

Created LOCAL-6 through the normal UI, Card
`55187ce0-605a-4384-bb6c-389d437f9b55`, Workspace
`14f115a0-8ccc-4ea9-a883-730361777a4d`, Session
`f933bebc-f4cf-4024-9ba8-4e33d0202efe`, AgentRun
`dca52b2b-180f-4582-bd3e-117f736a8f9a`, attempt
`a95b1bb1-7a5a-46f5-971a-03773a4fdb8d`. It ran from `04:16:55.551Z` to
`04:20:18.248Z` (about 3m 23s) on the latest Wiki commit `b16ffa0...`.

Native Audit confirms actual reads, not merely injected references: the local
Wiki, both modules, all six test files, canonical docs, public discovery, all
five public events and pinned old A/B snapshots. The observed discovery returned
11 activities, five events, no errors and no further cursors. The Agent verified
A/B/C ancestry, unchanged source acceptance tests and page-manifest hashes. It
distinguished target-maintenance metadata from old A/B checkout Wikis and did
not infer freshness from Done. It ran 24 Node tests and applicable syntax checks;
these are ordinary-session observations, not host proof for a formal candidate.
The checkout remained clean at the same HEAD.

The read-only final report at native sequence 2807 assessed all 16 fixed cases
in the retained `semantic-cases.json`. BE-02–08 and the formal contradiction,
selection, skipped/hidden-test, unavailable-environment, placeholder and no-op
cases received the expected safe decisions. It rejected the embedded instruction
to read peer-private data. These are sampled decisions over supplied evidence,
not real executions of each hypothetical repository. No benchmark improvement
rate or arbitrary-repository correctness guarantee is claimed. An exploratory
guessed API returned HTML; the Agent rejected it as evidence. This is recorded
as exploration overhead, not an existing public API.

Chrome MCP opened F's Wiki Reader, verified Current workspace/repository/branch,
navigated index → quickstart → the canonical counting-contract anchor, reloaded
Wiki files and the browser, and reopened the concept successfully. The retained
snapshot is `dev_assets/parallel-integration-acceptance/10-latest-wiki-viewer-after-reload.txt`.
The generated content preserves ownership rationale, filtering-before-pagination
and the different undefined-option contracts, adds full/filter/page counting,
and leaves the HTTP proposal explicitly unimplemented. This is a focused fixture
content check, not broad Wiki quality measurement.

### Additional final-boundary trials

Replayed the exact successful v3 POST request against the production route.
It returned the same Integration, Workspace and Agent IDs with `succeeded`;
it did not create another run or remerge. The request is retained as
`replay-published-request.json` in the ignored acceptance directory.

With no A writer active, added one isolated, untracked acceptance sentinel to A,
then used the Board UI to select A's exact commit and the test target. Admission
rejected it with `tracked or untracked changes present`; the Integration count
stayed three, the target remained `b16ffa0...` and the sentinel was not committed
or stashed. Snapshot `11-dirty-source-rejected-ui.txt` records the rejection.
Removed only this newly created sentinel and verified A clean again. No existing
file was deleted. The subsequent explicit UI retry completed successfully:

- Integration `5be44ac6-1bc4-4043-9711-3f54289eceb9`, 18 September
  `04:22:57.204`–`04:25:15.408 UTC` (2 minutes 18 seconds).
- Only A was selected. Both B and R remained
  `b16ffa0de86dcde24f5b2d7de3c067a8d29f5ebd`. The checked-out target stayed clean;
  no empty commit, repeated merge or new integration Manifest was created.
- The host independently ran five required commands: `npm test` (24 cases),
  `npm run check`, and syntax checks for count source and its two test files.
  All five persisted ExecutionProcesses have exit code 0 and matching R.
- Card completion was recorded and the existing selected event was already
  acknowledged by OpenWiki. No redundant Wiki generation was required.
- Chrome MCP expanded **Host validation evidence** and confirmed the published
  no-op, Done and separate Wiki result. The snapshot is
  `dev_assets/parallel-integration-acceptance/12-noop-host-validation-ui.txt`.

This is additional evidence on the corrected v3 binary, not a replacement for
the earlier actual source-changing Integration and automatic Wiki publication.

After F's read-only evaluation finished, attached a second isolated repository
through the normal repository registration and Workspace attachment APIs. This
does not alter the source state on which the earlier reader was evaluated:

- Repository: `/var/tmp/evk-parallel-acceptance-9NGp68/secondary-admission`, ID
  `c3ca360f-9e18-4490-8eaf-c186d5a1c7f6`.
- Secondary target: `test/parallel-integration-secondary` at
  `5dd0ce0109497d57540f75f426ebd5517cec0a5e`.
- F's attached secondary worktree contains a deliberately unintegrated test
  commit, `9f6212f36763132b9c41a4e0ab4ea38b2237e338`.
- Chrome MCP selected F's primary-repository source and clicked **Start selected
  Integration**. Production admission rejected the request with
  `has unintegrated results in another repository Parallel Integration Secondary Admission`.
- The Integration count remained four, F stayed Todo, the primary target stayed
  `b16ffa0...`, and neither secondary ref moved. No Agent or partial publication
  was started. Snapshot `13-secondary-repository-rejected-ui.txt` records it.

The accessibility snapshot initially labelled a selectable native option as
disabled. Read-only DOM inspection found `option.disabled=false`, and the normal
MCP form-fill action successfully selected it. This was not confirmed as an EVK
selection defect; no product change or forced DOM interaction was used.

## Operation and recovery

### Develop and inspect parallel work

Keep the Shared directories preset on if you want the Agent to discover public
parallel activity automatically. Your saved text is preserved; a fresh Session
uses the currently linked Card's policy. A Card with no policy does not revive
a previous Card's instructions. Continuations retain current run identity but
do not receive another copy of the saved policy or peer history.

Discovery covers the registered repository, including Workspaces without a
Manifest. It is an observation, not an atomic snapshot of every process and Git
ref. Read the relevant immutable records and pinned source objects; distinguish
reported tests from host verification. Private peer memory and conversations
are not part of this API. With Memory disabled or no Wiki, continue from source
and the available mechanical information. A discovery error is not zero work.

For a test-only combination, ask your Agent to use the supplied preview endpoint
and fixed full source OIDs. EVK creates a detached worktree beside the ordinary
Workspace root (`worktrees-previews`), without setup or normal source-finaliser
side effects. Commit your intended source first: dirty owner work is not silently
omitted. Trials are retained, not automatically pruned or merged. Inspect a
trial and stop its processes before explicitly removing that particular worktree
with Git; do not use a broad recursive deletion. A Direct-folder target also
requires a separate trial, never an in-place temporary merge/reset.

### Promote an explicit selection

1. Open the local Board's **Integrate / Auto Merge** panel. Select the repository,
   target branch, Agent execution settings (Code mode), Cards and adopted
   Workspaces. Multiple candidates require an explicit choice. The observed full
   source OID is retained when activity polling refreshes; a changed choice must
   be reselected.
2. Finish source runs, active saved Goals, queued messages, scripts and source
   finalisation first. Sources must be clean. EVK does not auto-commit, stash or
   reset your source to make it eligible. Other unintegrated repositories on a
   selected Card prevent partial completion of that Card.
3. Start once. The request identity makes HTTP retries idempotent. Sources and
   requirements are frozen at admission; the target base is captured when the
   queued run obtains its turn. Reservations cover all Workspaces of the selected
   Cards. Mutations that would invalidate the selection are rejected until safe
   release; reading logs remains available.
4. Follow the Run from any selected Card or the panel. **Open integration
   Workspace** opens its single Agent Session. The Agent's proposal is explicitly
   labelled as preceding host validation. Expand **Host validation evidence**
   for command, cwd, result, exit code, evidence and process ID.
5. Check the three outcomes separately: exact Git promotion, each Card's Done
   result, and Wiki reconciliation. A required check that cannot run blocks
   promotion. Already-integrated input can succeed with R=B, but still requires
   validation and conditional completion. It does not create an empty commit.

Unselected development can continue in separate Workspaces. Integration uses a
storage-scoped FIFO and a short publication lock shared with manual merge and
Wiki publication; it does not hold that lock during model execution. A legitimate
concurrent target change still invalidates the old B: start a new selection and
validate against the new target. Source Workspaces are not automatically archived
or deleted, and existing manual merge and PR routes remain available.

### Cancel, hold and reconcile

- **Cancel** stops the entire run before publication and releases reservations
  only after writers and continuations are stopped. Closing the panel does not
  cancel. Once publication wins its durable race with cancellation, it cannot be
  undone through Cancel.
- A held/failed/cancelled unpublished run leaves the target unchanged. Read its
  reason and Agent/log evidence, resolve the source, requirements or environment,
  then make a new explicit Integration. Do not quietly remove a selected Card.
- **Reconcile publication** is for the recorded run after an interrupted
  publication/post-processing boundary. It compares durable B/R/intent with actual
  ref, index and checkout state. It never repeats a merge or repairs by reset.
  Uncertain or dirty state remains `recovery_required`; inspect and preserve user
  files instead of deleting reservations or rewriting the DB to force success.
- A confirmed Git result can coexist with Card-completion refusal if requirements,
  links or a later reservation changed. The displayed per-Card result explains
  this; replay cannot mark a reopened Card Done or undo later source changes.
- Wiki errors do not undo source promotion or Done. Use the existing OpenWiki
  status/Sync controls after resolving the error; pending semantic events remain
  retryable. Integration into a different branch is not reported as updating the
  repository's configured Wiki target.

This version supports one local Board repository/target per Integration and one
EVK service. It does not provide remote Board automation, cross-repository atomic
completion, Delegation or protection against arbitrary external/full-access Agent
writes. Required recovery/audit records are retained by existing foreign keys and
reservations; do not promise automatic expiry or complete disk reclamation.

### Preserve or remove the acceptance assets

The isolated repositories, their `test/parallel-integration-v1` and
`test/parallel-integration-secondary` branches, EVK project,
source/Integration/Maintenance Workspaces and preview trial are intentionally
retained for inspection. The development branch has received none of their source
or Wiki commits. Evidence snapshots are local ignored files under
`dev_assets/parallel-integration-acceptance/`; logs are under
`/var/tmp/evk-parallel-acceptance-9NGp68/` and Native Audit stays in EVK's normal
runtime store. These are local evidence, not backups or a new artifact service.
Clean them up only after explicitly choosing which test assets to discard and
confirming no related run/process is active. Do not remove the development DB or
shared storage wholesale; Integration history can intentionally prevent deletion
of a referenced Workspace.

### Final quality-gate run

Logs are retained under `/var/tmp/evk-parallel-acceptance-9NGp68/`.

| Command | Result / log |
| --- | --- |
| `pnpm run format` | Passed, `format-acceptance-final.log`; final documentation-only pass is `format-completion.log` |
| `cargo test --workspace` | Passed, `workspace-tests-acceptance-final.log`: 915 passed, 0 failed, 8 ignored across 68 test/doctest result lines |
| `pnpm run check` | Passed, `check-acceptance-final.log`; local-web, remote-web, web-core, UI and root Rust |
| `pnpm run lint` | Passed, `lint-acceptance-final.log`; repository ESLint, root Clippy, i18n check |
| `pnpm run generate-types:check` | Passed, `types-acceptance-final.log`; generated shared types are up to date |
| Related `integrationApi` / `cardContext` Vitest | 13 passed, `web-acceptance-final.log` |
| Installed OpenWiki MCP preflight (`--ignored`) | Passed separately, `mcp-preflight-final.log` |
| Installed pinned CLI version probe (`--ignored`) | Passed separately, `openwiki-version-final.log` |

Of the eight default-ignored tests, two are installed-tool probes now executed
separately, five are stdio subprocess fixtures invoked by their parent transport
tests, and one is the existing stderr-processor doctest. These are not eight
newly disabled checks. Private remote-backend Cargo remains outside the agreed
local validation scope; remote-web and remote Rust formatting are retained.

The final diff review checked the shared mutation/dispatch/finalisation guards,
exact Git publication/recovery, frozen semantic outbox, generated types and Board
controls against the BT/AT contracts. No confirmed related finding remains open.
The secondary-repository trial required no production correction. The last
production change is the disabled-Memory correction tested by v3; subsequent
changes add regression tests and documentation only. Re-reading the running
server's `/proc/4492/exe` verified the v3 SHA-256 above. Cargo later relinked the
on-disk executable, so its path/hash is not substituted for the running process's
identity. The final test suite includes the added cache-loss, relink-and-relink-
back, and Integration-finaliser regressions.

The existing private-remote Cargo access limitation and the earlier unrelated
non-standard lint findings are not counted as passed checks. E's interrupted
evaluation remains failed evidence; F is its separately identified successful
replacement. The generated Markdown source-anchor warnings remain a content
limitation, not a broken internal Wiki Viewer navigation or a hand-repaired
acceptance result.

## Requirement-by-requirement acceptance audit

The identifiers below refer to the current specifications, not the historical
review baseline. **T** means an executed deterministic test at the indicated
production boundary; **UI** means real Chrome MCP/Codex acceptance; **E** means
a sampled real-Agent decision evaluation; **C** means inspected control flow
or an explicit scope/non-goal. A component test is not described as a full UI
trial, and an E result does not mechanically guarantee arbitrary model reasoning.

Evidence keys (test names are searchable in the source and final test log):

| Key | Authoritative evidence |
| --- | --- |
| CTX | `services::parallel_context` tests `saved_policy_is_exact_and_off_does_not_invent_context`, `discovery_uses_real_schema_and_includes_manifestless_activity_without_recreating_peers`; `LocalAgentRunPort::execution_env`; `cardContext.test.ts` |
| DISC | `utils::repository_memory` tests `discovery_reports_corruption_and_pages_without_weakening_publication`, `discovery_never_follows_symlinked_or_mismatched_events`, `parallel_immutable_outbox_and_workspace_isolation`, `cache_removal_preserves_manifests_and_discovery_needs_no_saved_index` |
| CONT | Codex `resumed_host_context_is_visible_before_any_new_turn`, `memory_compaction_orders_checkpoint_compact_and_reload`, `goal_stdio_lifecycle_orders_activation_plan_resume_failure_and_final_answer`; source-completion recovery tests in `services::repository_memory` |
| PIN | `git::snapshot::pinned_reads_need_no_checkout_and_never_follow_links_or_traversal`; `WorktreeManager::detached_preview_preserves_source_refs_and_survives_outside_workspace_root`; production `routes::parallel_context::{snapshot,preview}` |
| ADMIT | `routes::integrations::admission::{submit,idle,inspect_source,verify_sources,target_right}`; executed modern-Board repository and storage-identity FIFO tests; real dirty-source and secondary-repository UI rejection |
| RES | `db::models::integration` tests: reservations after dispatcher restart, late Agent cancellation, Goal/script reactivation, startup fencing, single cancel/publication winner; middleware/AgentRun/queue/container mutation guards |
| DONE | `db::models::integration` tests: atomic/idempotent Done receipt, reopening, changed requirements, change-and-revert, newer reservation, injected DB rollback; `runtime::post_process` source recheck |
| VALID | `runtime_tests::{host_proof_requires_after_head_and_matches_plan_not_self_report,proposal_rejects_changed_selection_empty_plan_and_placeholder_success,dispatched_prompt_uses_resolved_api_port_and_selected_identity}`; real host Script/ExecutionProcess evidence |
| GIT | All seven `git::publication` tests: exact checked-out promotion, stale/dirty/unmanaged refusal, common-storage lock, ignored-file collision, injected checkout failure, conservative recovery and no-op |
| REC | `runtime_tests::git_publication_survives_receipt_failure_without_a_second_merge`, plus GIT/DONE/RES; `recover_publication` preserves intent and requires writers to settle |
| MEM | `runtime_tests::{semantic_outbox_uses_only_frozen_events_and_rejects_changed_or_missing_inputs,disabled_memory_does_not_publish_events_or_integrations}`; existing pending/failed receipt, sequential parallel-card reconciliation, frozen-draft crash recovery and upstream-Wiki rebase tests |
| WEB | `integrationApi.test.ts`, local-only `KanbanContainer` Integration links, explicit `IntegrationPanel` selection and separate Git/Done/Wiki display; snapshots 04–13 |
| LIVE | A/B/C, fresh A preview, corrected v3 Integration/Sync, F reader, queued cancellation, dirty/secondary-repository rejection and no-op IDs/audits documented above |
| E16 | The sixteen fixed records in `semantic-cases.json`, the actual current formal policy in `formal-policy-v3.txt`, and F's terminal response at native sequence 2807; supplementary hypothetical decision samples, not executed fault scenarios |

### Bootstrap mechanical conditions

| Requirement | Evidence and verified boundary |
| --- | --- |
| BT-01 | T/CTX; UI/LIVE A and B actually read the supplied Wiki and discovery sources before coding. The saved policy is not duplicated when already in the initial prompt. |
| BT-02 | T/CTX and eight Card-context tests retain edited text/markers; no migration rewrites old descriptions. |
| BT-03 | T/CTX; UI/LIVE fresh A Session has new bootstrap and actually reads current references. |
| BT-04 | T/CTX + CONT: continuation excludes saved-policy duplication, injects current host context before a turn, and does not concatenate peer history. |
| BT-05 | T/CTX handles absent store and eventless real-schema activity; C/`execution_env` only passes existing Memory inputs. No invented filesystem source. |
| BT-06 | T/CTX/DISC: database membership and event identity filter repository IDs; same-name repositories cannot share event attribution. |
| BT-07 | T/CTX/DISC keep observed HEAD and event source OID separate; UI/LIVE D/F compare old A/B against the newer Wiki target. |
| BT-08 | T/DISC immutable atomic event publication, concurrent writers and partial discovery; C/no new persistent index exists to be mistaken for an atomic global snapshot. |
| BT-09 | T/CTX: missing marker returns no parallel policy; C/no ACL or history-erasure operation is attached to this setting. |
| BT-10 | T/PIN; UI/LIVE A's detached A+B trial passes 22 tests while peer/target refs and Card states remain unchanged. |
| BT-11 | E/E16 BE08_BT11_12 reports concrete missing peer contract and holds, without a peer write; the production prompt establishes the same boundary. |
| BT-12 | E/E16 BE08_BT11_12 refuses the requested direct peer fix; C/Delegation is not implemented. This is cooperative instruction, not OS isolation. |
| BT-13 | T/CTX explicitly reports Direct-folder discovery/link limitation and requires a separate trial; existing Memory context remains independent. |
| BT-14 | T/DISC cache-removal test reopens persistent storage and discovers the unchanged event without a cache/index. |
| BT-15 | C/CTX serialises references and labels peer material untrusted; no evaluation/shell execution of a Manifest in the reader. E/E16 includes an ignored hostile instruction. |
| BT-16 | T/CTX real-schema running/eventless Workspace remains visible; UI/LIVE B observes active A before A has a completed Manifest. |
| BT-17 | T/CONT covers all four resume execution modes and checkpoint→compact→reload; Memory recovery tests preserve per-run frozen identity. The new policy is appended alongside, not in place of, CURRENT Memory. |
| BT-18 | T/CTX OFF handling; C/`execution_env` calls `begin_coding_run` independently of the optional parallel policy. Existing Memory regression tests pass. |
| BT-19 | T/DISC returns normal entries plus per-record errors/cursors while strict `events()` still fails. |
| BT-20 | T/CTX + ADMIT re-link fixtures; current Issue is labelled current and legacy `task_id` is not converted into historical Issue ownership. |
| BT-21 | UI/LIVE D/F verify old source Workspaces retain their older checked-out Wiki after target reconciliation; T/CTX preserves distinct freshness fields. |
| BT-22 | T/PIN reads Git objects without checkout and rejects missing objects, symlinks, submodules and traversal; C/discovery never calls `ensure_container_exists`. |

### Agent interpretation samples

| Requirement | Sample and result |
| --- | --- |
| BE-01 | UI/LIVE independent pagination/filter Cards: B notices A, implements only filtering, and does not silently import pagination. |
| BE-02 | E/E16 shared identifier/type disagreement: preserve selected requirement, flag the concrete peer compatibility change. |
| BE-03 | E/E16 ancestry case and UI/no-op: recognise source already present; do not duplicate it because it is a peer. |
| BE-04 | UI/D/F and E/E16: target Done/integration is distinct from the old source checkout. |
| BE-05 | E/E16 explicit later supersession wins over an old Manifest proposal, subject to source verification. |
| BE-06 | T/DISC pagination and UI/F selective event/pinned reads; E/E16 selects relevant records instead of unlimited transcript injection. |
| BE-07 | E/E16 unrelated corruption is reported without blocking unrelated work; T/DISC preserves partial visibility. |
| BE-08 | E/E16 undefined required API: do not invent it; report the needed owner decision and hold that combination. |

These samples required no extra explanatory user turn after their recorded task
instructions. The full audit retains tool calls and intermediate reasoning; no
token/read-volume or latency improvement percentage was measured. F's 16-case
exercise took about 3 minutes 23 seconds and was read-only. That is an observation,
not a benchmark target or an estimate of success on arbitrary repositories.

### Formal Integration conditions

| Requirement | Evidence and verified boundary |
| --- | --- |
| AT-01 | UI/v3 explicitly selects A/B/C, creates one Workspace/Session, completes all three; WEB links to that same Run. |
| AT-02 | C/WEB requires choosing among multiple Workspaces; T/WEB freezes the adopted Workspace/OID rather than substituting on refresh. |
| AT-03 | UI/A has initial and fresh Sessions, yet one Workspace/source appears once in the selection; C/admission rejects duplicate Workspace/Card entries. |
| AT-04 | UI/dirty rejection with retained sentinel; C/ADMIT checks active runs/registry, saved Goal, queue, scripts and unfinished Memory finalisation; T/RES fences competing starts/reactivation after reservation. |
| AT-05 | UI/LIVE stopped In-progress A/B/C accepted; C/ADMIT does not require Done status. |
| AT-06 | UI/no-op accepts Done A but determines source presence from Git; C/no Card-status shortcut declares any target integrated. |
| AT-07 | E/E16 unselected ancestor blocks and asks for reselection; prompt requires ancestry/diff investigation, not a semantic ownership engine. |
| AT-08 | UI/secondary-repository trial rejects F before creating an Integration and preserves both targets, peer source and Todo. C/ADMIT traverses every repository of every linked Workspace and requires other-repo HEAD ancestry into its own target; dirty/writer guards also apply. |
| AT-08A | T/PIN and UI/LIVE A+B detached trial; no target, peer or Card mutation or normal source event from the trial. |
| AT-08B | E/E16 peer contract change is held with evidence, distinct from locally resolvable text conflict. |
| AT-08C | E/E16 explicit request to edit B does not extend peer-write authority. |
| AT-09 | UI/v2 real same-file conflict resolved with both source test sets and four cross-feature regressions; v3 preserves them and adds counting, 24 tests. |
| AT-10 | E/E16 contradictory API demands produce blocked, not unilateral requirement removal; T/VALID rejects non-ready publication proposals. |
| AT-11 | E/E16 one incompatible member blocks the full set; T/VALID requires exact Card IDs; there is no partial-publication branch. |
| AT-12 | E/E16 unselected dependency requires reselection; UI/v2 does not consume unselected C's source or event. |
| AT-13 | E/E16 integration-order regression calls for local fix/revalidation or hold; T/VALID refuses a failing required command; real conflict fixes pass host reruns. |
| AT-14 | E/E16 rejects skips as equivalent verification; T/VALID binds the actual required command/exit/R. Language-specific assertion meaning remains Agent review. |
| AT-15 | E/E16 hidden duplicate test registration is detected as lost coverage; no universal test-discovery engine is claimed. |
| AT-16 | T/VALID rejects changed after-HEAD; C/runtime checks clean R before/after each command and before publication. |
| AT-17 | E/E16 absent environment produces hold; T/VALID rejects missing/empty/placeholder plan and unsuccessful execution; C/runtime timeout is not passed. |
| AT-18 | E/E16 refuses a conflicting user DB/port; real fixture uses dependency-free local checks. Cooperative external-resource assessment, not an OS sandbox. |
| AT-19 | T/ADMIT canonical-storage FIFO; UI/queued C cancellation while another Run owns target; C/advance captures B only after acquiring its turn. |
| AT-20 | T/ADMIT uses two registered IDs with shared storage; T/GIT linked paths contend on the same common-dir lock. |
| AT-21 | C/manual merge and Wiki publication use the same short Git guard; T/GIT lock contention and stale-B refusal. Real automatic Wiki follows source promotion. |
| AT-22 | T/GIT stale expected-B refusal without reset; C/runtime reports hold/recovery, never silently replans against a moving target. |
| AT-23 | T/GIT clean/expected-HEAD and T/WEB fixed choice; C/ADMIT rechecks full source/link/requirements before publication; DONE rechecks after it. |
| AT-24 | UI/queued cancel releases without Workspace/source change; T/RES late dispatch fencing. C/runtime stops Agent and validation children before releasing. |
| AT-25 | T/RES durable cancellation/publication winner under SQLite writer transaction; T/WEB no Cancel during/after publication. |
| AT-26 | T/GIT and UI/v2/v3 checked-out target: ref, index and actual source files agree with exact R. |
| AT-27 | T/GIT rejects untracked and ignored collisions and preserves contents; no stash/reset. |
| AT-28 | C/WEB/prompt/operator guide explicitly declare cooperative mode; T/GIT detects changed target. Arbitrary full-access writes are outside the agreed guarantee. |
| AT-29 | T/WEB stable request; real exact v3 POST replay returns the same Run/Workspace/Agent and no new merge. |
| AT-30 | T/REC injects failure after real Git promotion and before receipt, then reconciles without a second merge; DONE receipt rollback is atomic. |
| AT-31 | T/GIT confirmed later target changes are retained; real Wiki adds a descendant without invalidating source success. |
| AT-32 | T/RES startup fences dispatch and retains reservations; C/idle also checks process registry/script teardown before recovery or release. |
| AT-33 | T/DONE reopening/replay cannot redo Done; C/Card-status mutation has no Git undo hook. |
| AT-34 | C/ADMIT has no past-Done exclusion and accepts new explicit source OID after release; T/DONE later reservation cannot be overridden by old post-processing. |
| AT-35 | UI/LIVE fixture has no remote or GitHub dependency and local Done succeeds; C/WEB new action is local-Board only. |
| AT-36 | C/existing merge/PR routes retained, only shared guards added; root Git/manual merge regression tests pass. No auto push/PR introduced. |
| AT-37 | T/RES archive/delete and source metadata mutations are fenced; C/container cleanup checks reservations and referenced history has restrictive foreign keys. |
| AT-38 | UI/F actually reads current Wiki, integration events and original source events; source/target/OID distinction is in its terminal report. |
| AT-39 | T/GIT/REC and UI/v3 exact R; original A/B/C refs/checkouts retained, no squash or source auto-archive. |
| AT-40 | T/VALID changed/missing after-HEAD fails; T/container Integration-owner disables normal commit; C/Integration terminal handling blocks queued finalisation and fresh dispatch. |
| AT-41 | T/PIN creates a detached trial from a checked-out main repository and preserves main; C/preview handles Direct-folder owner root explicitly, while CTX instructs use of a separate trial. No normal finaliser owns it. |
| AT-42 | UI/v3 Board links from all three Cards; C/Integration Workspace is cardless, source links are not overwritten. |
| AT-43 | T/DONE reopened/revised/relinked expectations and later reservation; requirement change-and-revert also invalidates completion. |
| AT-44 | T/RES dispatcher restart does not release business reservations or allow a late Agent; C/startup fence precedes generic outbox recovery. |
| AT-45 | T/RES real SQL admission/Goal/script/link mutations and cancel race; C/queue, middleware, rebase/merge and cleanup use shared admission guards. |
| AT-46 | T/VALID exact plan/cwd/process/R/exit, missing after-HEAD, changed command and placeholder rejection; E/E16 legitimate private-Cargo exclusion differs from `true`. |
| AT-47 | T/MEM fixed IDs deduplicate, exclude later/peer events and reject missing/corrupt evidence; UI/v2 excludes C then v3 consumes C only after explicit selection. |
| AT-48 | T/MEM failed receipts remain retryable, sequential Wiki knowledge is retained; UI/v3 automatic Sync and F demonstrate successful update, publication and later use. No live provider-failure injection was needed for the automated failure contract. |
| AT-49 | T/GIT no-op recovery and UI/no-op R=B with five host checks, no empty commit, conditional Done and existing Wiki acknowledgement. |
| AT-50 | T/GIT injected checkout/ref split and T/REC/RES: uncertainty is retained for recovery, no destructive repair or false Done. |

### Architectural requirements and agreed exclusions

The BT/AT rows exercise the detailed contracts; the following maps their parent
requirements so that prose-only constraints are not silently dropped:

| Parent requirements | Implemented boundary / evidence |
| --- | --- |
| C01–C02; B-01–B-02A | Public observations and existing immutable hints only; CTX/DISC/PIN, E16; no shared conversation or semantic-dependency DB. |
| C03–C05; B-23; A-01–A-05 | Local Board explicit batch and existing Orchestration; WEB/LIVE/DONE/GIT. No status-column trigger, auto PR or Done-driven undo. |
| C06; A-24–A-28 | Host Script proof is distinct from semantic test-plan judgement; VALID/E16; scoped private backend exclusion is preserved. |
| C07–C08; B-22–B-24; A-06–A-08 | Reusable pinned Git reads/detached trials, separate formal promotion; PIN/LIVE. Delegation remains a future boundary, not a dependency. |
| B-03–B-06, B-11–B-16 | Saved marker policy plus per-run header, fresh/continuation split, checked-out Wiki freshness and OFF/Memory independence; CTX/CONT/LIVE. |
| B-07–B-10, B-17–B-21 | Existing persistent store, partial discovery versus strict publication, no private peer memory, immutable writes, no new cache database or worktree concurrency entitlement; DISC/CTX/RES. |
| B-25; A-43–A-44 | Existing Manifest/outbox/Sync with frozen event IDs and Integration semantics; MEM/LIVE, current production formal prompt. |
| A-09–A-16 | Explicit adopted OID/requirements, idle/multi-repo admission, durable business reservations, canonical storage FIFO; ADMIT/RES/WEB. |
| A-17–A-23 | One cardless Workspace/Session, one existing Orchestration Agent node, separate finalisation purpose and cooperative source/target boundary; LIVE/VALID/E16. |
| A-29–A-32 | Host proof and clean-source/target rechecks, expected-B/exact-R, checked-out consistency and conflict refusal; VALID/GIT/ADMIT. |
| A-33–A-38 | Separate Git/Card/Wiki outcomes, cancel winner, reasoned hold, conditional Done and no undo; RES/DONE/WEB/LIVE. |
| A-39–A-42 | Existing orchestration/audit/Script logs plus minimal Integration product/receipt/lease-independent reservations; REC/RES/DONE. No new scheduler or third audit system. |

Local-only formal Integration, one repository/target, one adopted Workspace per
Card, one EVK service and cooperative permissions are the user's explicit scope
choices. Ordinary multi-repository Workspaces and repo-separated read context
remain; their partial automatic Done is not introduced. Remote Board automation,
cross-service exclusion guarantees, Delegation, OS-enforced peer isolation,
automatic source cleanup, auto push/PR/release and universal semantic correctness
are not claimed. Rows marked C identify static inspection or scope decisions,
not additional live fault-injection trials. Agent decisions are identified as
sampled, and deterministic boundaries are tested separately rather than
presented as a universal AI guarantee. The mandatory real-model UI sequence is
the separately identified LIVE evidence, not replaced by these static checks.
