# EVK OpenWiki Repository Memory Integration
## Detailed Design Specification

Status: Implementation Ready  
Target: easy-vibe-kanban (EVK)  
Initial OpenWiki compatibility target: 0.5.1  
Canonical spec path: `docs/design/openwiki-repository-memory.md`

---

# 1. Purpose

EVKに、Coding Agent向けの永続的repository memoryを導入する。

目的は単なる自動ドキュメント生成ではない。

将来のCoding Agentが毎回repository全体を再探索せず、

- architecture
- component boundaries
- lifecycle
- invariants
- non-obvious dependencies
- failure semantics
- dangerous modification points
- current behavioral semantics

を短時間で理解できる状態を維持する。

Repository memory engineにはOpenWikiを利用する。

EVK自身でWiki generatorを再実装せず、OpenWikiをforkせず、公式CLI / Codex integrationを外部依存として利用する。

---

# 2. Core principles

## 2.1 Authority hierarchy

情報のauthorityは以下とする。

1. source code / tests / configuration
2. canonical repository documentation
3. OpenWiki
4. Change Manifest / Workspace Memory
5. conversation context

OpenWikiはsource of truthではない。

OpenWikiは、

> derived semantic cache of the repository

として扱う。

Wikiとコードが矛盾する場合、必ずコード・テストを優先する。

---

## 2.2 Separate short-, medium-, and long-term memory

知識を3層に分離する。

```text
Conversation Context
        │
        │ semantic persistence
        ▼
Workspace Memory
        │
        │ merge / reconciliation
        ▼
Canonical OpenWiki
```

### Conversation Context

現在のCodex threadの作業記憶。

compactによって失われる可能性がある。

### Workspace Memory

未merge workspace固有の意味的作業記憶。

主に、

- why
- decisions
- rejected alternatives
- constraints
- unresolved questions
- current direction

を保存する。

Git diffから復元可能な単純な実装情報は保存しない。

### OpenWiki

merge済みrepositoryに対するcanonical long-term memory。

---

# 3. Non-goals

以下は本機能の目的外とする。

- OpenWikiのfork
- OpenWiki内部実装へのpatch
- OpenAI Responses APIの利用
- OpenWiki用の別OpenAI API key要求
- workspaceごとの独立OpenWiki生成
- raw conversation全文の永続保存
- Markdown Wiki同士のLLM merge
- source codeの文章化だけを目的とするdocumentation generator
- Git commit messageへのconversation全文格納

---

# 4. OpenWiki dependency

OpenWikiはDocker imageに外部CLI依存として含める。

初期validated version:

```text
openwiki@0.5.1
```

Dockerfileではversionを明示的にpinすること。

例:

```dockerfile
ARG OPENWIKI_VERSION=0.5.1
RUN npm install -g openwiki@${OPENWIKI_VERSION}
ENV OPENWIKI_TELEMETRY_DISABLED=1
```

要件:

- `latest`を使用しない
- Node.js 22+を保証する
- `openwiki --version`または同等のhealth checkを可能にする
- OpenWiki sourceをEVK repositoryへvendorしない
- OpenWiki packageへpatchを当てない
- versionは一箇所で変更可能にする
- Docker build時にuser credentialを保存しない
- telemetryはEVK Docker distributionではdefault disabledとする

OpenWiki upgradeは明示的なdependency upgradeとして扱い、compatibility tests後にversionを上げる。

---

# 5. OpenWiki execution mode

EVKでは原則としてOpenWikiのCodex host-driven integrationを使用する。

```text
EVK
 │
 ├─ Codex CLI / app-server
 │      │
 │      └─ authenticated ChatGPT/Codex session
 │
 └─ OpenWiki
        │
        └─ host-driven MCP lifecycle
```

OpenWiki native providerを通常経路にはしない。

理由:

- EVKですでにCodex認証を行っている
- API Platformとの二重課金を避ける
- Codexがrepository toolsをすでに利用できる
- OpenWiki自身はClaims / page queue / validation / finalizationに集中できる

OpenWiki integrationはruntimeでidempotently利用可能にする。

概念上:

```bash
openwiki integrations install codex
```

を使用する。

複数worktreeから利用可能にするため、原則user-level Codex integrationを利用する。

既存EVK architectureとの整合上project scopeが適切な場合はproject scopeを使用してよいが、複数worktreeから同一repository integrationを利用できることを保証する。

---

# 6. Canonical Wiki location

OpenWiki標準に従い、canonical WikiはGit repository内:

```text
openwiki/
```

に保持する。

これはGit tracking対象とする。

通常のcoding workspaceでは:

```text
openwiki/
```

はread-only semantic contextとして扱う。

Coding Agentは参照してよいが、通常のcard/task executionでは変更してはならない。

指定されたWiki reconciliation runだけがcanonical OpenWikiを書き換える。

通常workspaceが誤って`openwiki/`を変更した場合、task completion時に検出する。

通常coding taskではその変更をcommitしてはならない。

---

# 7. OpenWiki instructions

repositoryにはuser-authored:

```text
openwiki/INSTRUCTIONS.md
```

を保持する。

最低限、以下の方針を記述する。

```md
This wiki is primarily persistent context for future coding agents.

Prioritize:
- architectural boundaries
- behavioral semantics
- invariants
- lifecycle rules
- non-obvious cross-component dependencies
- failure semantics
- dangerous modification points
- design rationale when supported by repository evidence

Avoid:
- file-by-file summaries
- class/function inventories
- facts trivially discoverable from source
- generic prose
- unsupported inference

Source code, tests, and configuration are authoritative.

Task intent, Change Manifests, and Workspace Memory may be used as
routing and interpretation hints, but must not override repository evidence.
```

このファイルはOpenWiki regenerationでも保持される前提で扱う。

---

# 8. Change Manifest

EVKの現在のcommit-summary generationを拡張し、内部的な中心表現を`Change Manifest`とする。

commit messageのためだけにsummaryを生成して捨ててはならない。

```text
Conversation
     +
Task / Goal
     +
Codex result
     +
git diff
     +
tests
       │
       ▼
Change Manifest
   /        \
  ▼          ▼
Commit     Wiki reconciliation
Message       Context
```

---

# 9. Change Manifest schema

実装時はrepository既存のserialization conventionsに合わせること。

概念schema:

```ts
interface ChangeManifest {
  version: 1;

  eventId: string;
  repositoryId: string;
  workspaceId: string;
  taskId?: string;

  createdAt: string;

  baseCommit: string;
  sourceCommit: string;
  targetBranch?: string;

  goal: string;
  summary: string;

  behavioralChanges: string[];
  architecturalChanges: string[];
  invariantsAffected: string[];

  decisions: Array<{
    decision: string;
    rationale?: string;
  }>;

  rejectedAlternatives: Array<{
    alternative: string;
    reason?: string;
  }>;

  unresolvedQuestions: string[];

  changedPaths: string[];

  tests: Array<{
    command?: string;
    result: "passed" | "failed" | "not-run";
    summary?: string;
  }>;
}
```

`changedPaths`, commit IDsなど機械的に取得可能な値はLLMに推測させず、Git/EVKからdeterministically取得する。

LLMは主としてsemantic fieldsを生成する。

---

# 10. Commit message generation

commit messageはChange Manifestから生成する。

raw conversationをcommit messageへ入れない。

commit messageは人間向けGit historyとして簡潔に保つ。

Change Manifestがsemantic sourceになる。

理想形:

```text
Change Manifest
      │
      ├── human projection → Git commit message
      │
      └── agent projection → OpenWiki reconciliation
```

可能な限り、現在のcommit summary生成とChange Manifest生成を二重のLLM callにしない。

既存summary generationをstructured resultへ発展させることを優先する。

---

# 11. Repository shared folder

既にEVKに存在するrepository-scoped shared folderを利用する。

別のrepository coordination storageを新設してはならない。

既存shared-folder abstractionの下にlogical namespaceを作る。

概念構造:

```text
<repository-shared>/
└── knowledge/
    ├── events/
    │   ├── <event-id>.json
    │   └── ...
    │
    ├── receipts/
    │   ├── <event-id>.json
    │   └── ...
    │
    ├── workspace-memory/
    │   ├── <workspace-id>.md
    │   └── ...
    │
    └── locks/
        └── wiki-reconcile.lock
```

実際のroot/path namingは既存EVK shared-folder conventionsに従ってよい。

---

# 12. Semantic Outbox

各workspaceはtask completion後、Change Manifestをrepository shared folderの`events/`へ出力する。

これはSemantic Outboxとして機能する。

要件:

- one event = one file
- event fileはimmutable
- filenameはcollision-free ID（ULID/UUID等）
- temp fileへwrite後atomic renameする
- 全workspaceが同じJSON fileへ追記する方式は禁止
- parallel workspace間でlockを要求しない設計にする

Workspace A/B/Cはそれぞれ独立してeventを書ける。

```text
Workspace A ──→ Event A
Workspace B ──→ Event B
Workspace C ──→ Event C
```

---

# 13. Workspace behavior

通常workspaceの責務:

1. canonical OpenWikiを読む
2. sourceを変更する
3. testsを実行する
4. Change Manifestを作る
5. source commitを作る
6. finalized Change Manifest eventをshared folderへ書く
7. Workspace Memoryを必要に応じて維持する

通常workspaceはcanonical OpenWikiを書かない。

---

# 14. Parallel workspace model

worktreeを並列で実行できるEVKの特性を維持する。

```text
             main
        code + Wiki W0
             │
   ┌─────────┼─────────┐
   ▼         ▼         ▼
Workspace A Workspace B Workspace C
   │         │         │
 source A   source B   source C
   │         │         │
 Event A    Event B    Event C
```

全workspaceは自身のbase commit時点のOpenWiki snapshotを参照する。

この状態は正しい。

古いsourceをbaseにしているworkspaceが、古いWikiを読むことは整合的である。

workspaceがmainをrebase/mergeした場合は、新しいsourceと一緒に新しいOpenWikiも入る。

---

# 15. Why Wiki is not updated in each workspace

OpenWikiはderived stateである。

parallel branchで個別にderived stateを変更してGit mergeする必要はない。

原則:

```text
parallel source transactions
          ↓
canonical source integration
          ↓
derived memory refresh
```

materialized viewと同様に扱う。

これにより:

- Markdown conflict
- Claims conflict
- OpenWiki run-state conflict
- branch-local stale Wiki merge

を避ける。

---

# 16. Wiki Reconciler

repositoryごとにlogical single writerとなるWiki Reconcilerを実装する。

同一repositoryで同時に複数Wiki reconciliationを走らせない。

既存EVK locking abstractionがある場合はそれを利用する。

なければrepository shared folder上のlockを使用する。

---

# 17. Merge and reconciliation lifecycle

標準flow:

```text
Workspace completed
       │
       ▼
Change Manifest emitted
       │
       ▼
source integration / merge
       │
       ▼
tests / existing merge validation
       │
       ▼
Wiki Reconciler
       │
       ├─ current canonical source
       ├─ current OpenWiki
       ├─ relevant Change Manifest(s)
       └─ relevant Workspace Memory
       │
       ▼
OpenWiki host-driven update
       │
       ▼
OpenWiki validation/finalization
       │
       ▼
separate Wiki commit
       │
       ▼
reconciliation receipt
```

---

# 18. Reconciliation uses final source state

Wiki同士をmergeしてはいけない。

OpenWiki reconciliationは必ずintegration後のfinal source treeを基準とする。

Change Manifestは:

```text
routing hint / semantic interpretation hint
```

であってsource of truthではない。

Conflict解決によりManifestの意図と最終コードが変わった場合、最終コードを優先する。

---

# 19. Batch reconciliation

複数cardを一度にmergeした場合、Wiki updateをcard数だけ実行する必要はない。

```text
Integrated Source = A + B + C

Context:
  Manifest A
  Manifest B
  Manifest C
```

として一度のreconciliationを実行してよい。

むしろ最終source状態を一度に解釈できるため望ましい。

---

# 20. Event selection

merge pipelineは、今回統合したworkspace/taskに対応するevent IDを明示的にWiki Reconcilerへ渡すことを第一選択とする。

Git ancestryによる自動推測だけに依存しない。

理由:

- squash merge
- cherry-pick
- conflict resolution
- rebased commit

ではoriginal workspace commitとtarget historyの関係が変化し得るため。

Commit ancestryはfallback / integrity checkとして利用してよい。

---

# 21. Reconciliation prompt contract

Wiki-maintenance Codex runには最低限以下を渡す。

```text
Update this repository's OpenWiki for the integrated source state.

The following EVK Change Manifests describe the intent of changes
included in this integration.

Use them only as routing and interpretation hints.
Repository source, tests, and configuration are authoritative.
Do not preserve a Manifest claim when the integrated repository
does not support it.

Use the installed OpenWiki integration and complete its durable
update/finalization lifecycle.
```

その後にManifest内容を付与する。

Workspace Memoryも同じくhintとして扱う。

---

# 22. Git commit policy

source changeとWiki reconciliationは別commitを推奨する。

例:

```text
abc123 feat(workspace): unify runtime cleanup lifecycle
def456 docs(openwiki): reconcile repository knowledge
```

メリット:

- sourceとderived artifactを分離できる
- OpenWikiだけrevertできる
- failure investigationが容易
- Git historyが明瞭

Wiki commitには可能ならreconciled source commit / event IDsをmetadataまたはbodyとして残す。

---

# 23. Wiki initialization

repositoryにOpenWikiが存在しない場合:

1. OpenWiki availabilityを確認
2. Codex integration availabilityを確認
3. `openwiki/INSTRUCTIONS.md`を準備
4. dedicated Wiki Codex runを開始
5. host-driven OpenWiki initializationを実行
6. validation/finalization完了
7. generated `openwiki/`をcommit
8. repository wiki statusをCurrentにする

Initializationは通常coding cardとは独立runとする。

---

# 24. Workspace Memory

長寿命workspaceでCodex context compact後に意味的情報が消失する問題を防ぐ。

Workspace Memoryはrepository shared folderに置く。

```text
<repository-shared>/
  knowledge/
    workspace-memory/
      <workspace-id>.md
```

これはGit管理しない。

---

# 25. Workspace Memory content

保存するべき内容:

- current goal
- important design decisions
- why a choice was made
- rejected alternatives and reason
- constraints not obvious from code
- product/user intent relevant to implementation
- known invalid hypotheses
- unresolved questions
- current next direction

保存しない内容:

- changed file lists
- function/class inventories
- generic progress logs
- facts trivially recoverable from Git/code
- raw conversation transcript

Principle:

```text
WHAT happened → source / Git
WHY it happened → Workspace Memory
WHAT is canonical → OpenWiki
```

---

# 26. Workspace Memory lifecycle

```text
conversation
    │
    ▼
workspace semantic decisions
    │
    ▼
Workspace Memory
    │
    ├─ survives context compaction
    │
    └─ informs Change Manifest/reconciliation
```

Codexがcompactされた後、新しいcontextには:

1. repository instructions
2. OpenWiki quickstart / relevant Wiki pages
3. current Workspace Memory

を再び読ませる。

`additionalContext`だけに依存せず、persistent fileとして保存する。

---

# 27. Workspace Memory update strategy

追加API課金を発生させない。

可能な限り現在実行中のCodex sessionを利用する。

Codexにpersistent instructionとして:

> semantic decision, rationale, rejected alternative, important unresolved questionなど、codeから復元不能な重要情報が生じた場合のみWorkspace Memoryを更新する

という責務を与える。

さらにEVKがCodex compaction lifecycleを観測可能な場合:

- explicit compaction前にmemory checkpoint
- compaction完了後にmemory reread

を行う。

auto-compaction前イベントが利用できない場合に備え、pre-compaction hookだけに依存してはならない。

実装時に現行Codex app-server protocolを調査し、public/available lifecycle eventを使用する。Codexをpatchしない。

---

# 28. Workspace Memory promotion

Workspace Memoryは自動的にcanonical truthにはしない。

merge時にChange Manifest作成およびWiki reconciliation contextとして利用する。

OpenWikiに残すmaterial factはrepository evidenceで裏付けられることを原則とする。

コードから裏付けられないhistorical rationaleを無理にGrounded Claimへ変換してはならない。

そのような情報は必要に応じて:

- Change Manifest
- Git commit body
-既存のcanonical design/ADR document

に保持する。

---

# 29. Reconciliation receipts

Wiki Reconcilerがeventを処理した後、shared folderへreceiptを書く。

概念schema:

```ts
interface WikiReconciliationReceipt {
  eventId: string;
  reconciledAt: string;
  targetCommit: string;
  wikiCommit?: string;
  result: "updated" | "no-op" | "failed";
  error?: string;
}
```

event本体は変更しない。

Receiptもone-event-one-fileとする。

---

# 30. Failure semantics

## Source integration failure

Wiki reconciliationを開始しない。

Eventは未処理のまま残す。

## OpenWiki failure

source integration自体を破棄する必要はない。

ただしrepository Wiki statusを`Stale`または`Error`にする。

該当eventsは未reconciledとして残す。

次回manual/automatic syncでretry可能にする。

## OpenWiki no-op

正常成功としてreceiptを書く。

## Source drift during OpenWiki run

OpenWikiのsource-drift semanticsに従う。

最新sourceへ対するreconciliationが完了するまでCurrent扱いにしない。

---

# 31. Stale Wiki protection

merge済みsourceに対して未reconciled eventが存在する場合、EVKはrepository WikiをCurrentとして扱ってはならない。

新しいCoding Agent run開始時に、必要に応じ:

```text
The repository OpenWiki may be stale relative to the current source.
Treat source/tests as authoritative and inspect the pending semantic
change context for affected areas.
```

という追加contextを渡す。

Wiki failureがCoding Agentへsilentに伝播しないこと。

---

# 32. Concurrency rules

- workspace event writes: fully parallel
- workspace memory writes: workspace-local
- source worktree development: fully parallel
- Wiki Reconciler: one per repository
- canonical OpenWiki writer: one
- event receipt writer: Wiki Reconciler

これによりEVKのparallel-card advantageを維持する。

---

# 33. Repository Wiki status

EVK内部で少なくとも以下のstateを表現する。

```text
Disabled
Uninitialized
Initializing
Current
Stale
Reconciling
Error
```

`Current`は少なくとも:

- OpenWiki exists
- no known merged/unreconciled events
- last reconciliation succeeded

を意味する。

---

# 34. Minimal UI

repository-level UIに最低限:

- Wiki status
- Initialize Wiki
- Reconcile / Sync now
- last successful reconciliation
- error state

を表示できるようにする。

Card UIへの大規模追加は必須ではない。

必要ならdebug/detail viewとして:

- semantic event queued
- event reconciled

を表示する。

---

# 35. OpenWikiAdapter

EVKコードにはOpenWiki固有処理を一箇所へ集約する。

概念interface:

```ts
interface OpenWikiAdapter {
  getVersion(): Promise<string | null>;
  isAvailable(): Promise<boolean>;
  ensureCodexIntegration(...): Promise<void>;

  getStatus(...): Promise<OpenWikiStatus>;

  initialize(...): Promise<OpenWikiRunResult>;
  reconcile(...): Promise<OpenWikiRunResult>;
}
```

OpenWiki internal TypeScript modulesを直接importしない。

public CLI / integration / MCP contractを境界として使用する。

将来別Wiki engineへ交換可能な設計を維持する。

---

# 36. Existing EVK abstractions first

実装前に必ず現在のEVKコードを調査する。

特に:

- repository model
- workspace/worktree lifecycle
- repository shared folder
- Codex runtime / app-server integration
- goal support
- current commit-message summary generation
- merge workflow
- Docker image
- locking/concurrency utilities
- task completion lifecycle

既存abstractionが存在する場合はそれを利用する。

この仕様のためだけに並行する別frameworkを作らない。

---

# 37. Security and privacy

- OpenWiki credentialをDocker imageへ焼き込まない
- OpenWiki native API keyを要求しない
- host-driven Codex authenticationを優先
- telemetry default disabled
- shared folderのWorkspace Memoryにはcredential/secretsを保存しない
- raw conversationをChange Manifestへ保存しない
- repositoryの既存secret filtering rulesを尊重する
- `.openwikiignore`が必要なrepositoryでは利用可能にする

---

# 38. Testing requirements

## Unit tests

最低限:

- Change Manifest schema
- deterministic fields
- commit message derivation
- atomic event write
- event/receipt matching
- reconciliation state
- Workspace Memory path isolation
- Wiki status derivation
- OpenWiki version detection
- OpenWiki error handling

## Parallel workspace tests

最低限2 worktreeを作り:

- A/Bが同時にeventを書ける
- event fileが競合しない
- A/Bの通常taskが`openwiki/`を変更しない
- A merge後にreconciliationできる
- B merge後に最新canonical Wikiからreconciliationできる

ことを確認する。

## Batch test

A/Bをintegrationした後、一回のreconciliationで複数Manifestを渡せること。

## Failure test

OpenWiki command/integrationをfailureさせ:

- source resultが破壊されない
- eventがpendingのまま
- Wiki statusがStale/Error
- retryで成功できる

こと。

## No-op test

OpenWikiがWiki変更不要と判断した場合もeventが正常reconciledになること。

## Docker test

built image内でOpenWiki CLIとversion pinを確認する。

## Existing quality gates

repositoryの既存:

- formatting
- lint
- typecheck
- unit tests
- integration tests

をすべて通す。

---

# 39. Manual smoke test

実際のCodex authenticationを利用するmanual integration smoke testを用意する。

最低シナリオ:

1. test repositoryでOpenWiki initialize
2. Wikiをcommit
3. EVK workspaceでsemantic code change
4. Change Manifest生成
5. source merge
6. host-driven OpenWiki reconciliation
7. relevant Wiki page更新
8. reconciliation receipt生成
9. 次のCodex workspaceが更新Wikiを読む

CIではreal model callを必須にしない。

---

# 40. Compatibility policy

OpenWikiは動きの速い外部dependencyである。

したがって:

- exact version pin
- adapter boundary
- feature detection
- compatibility test

を必須とする。

OpenWiki upgrade時にEVK source全体を変更する必要がない構造を維持する。

---

# 41. Implementation order

推奨順序:

### Phase 1 — Dependency
- Docker OpenWiki pin
- adapter skeleton
- availability/version test
- Codex integration setup

### Phase 2 — Change Manifest
- existing commit summary flow調査
- structured Change Manifest
- commit message projection
- Semantic Outbox

### Phase 3 — OpenWiki bootstrap
- repository status
- initialize flow
- INSTRUCTIONS.md
- canonical Wiki commit

### Phase 4 — Reconciliation
- single-writer lock
- merge integration
- manifest batching
- OpenWiki update
- receipts
- failure/stale handling

### Phase 5 — Workspace Memory
- shared-folder storage
- agent read/write integration
- compaction-aware reload
- Change Manifest integration

### Phase 6 — UX and hardening
- status UI
- manual sync
- parallel tests
- failure recovery
- documentation

Phases may be rearranged when existing EVK architecture makes another order clearly safer.

---

# 42. Acceptance criteria

Implementation is complete when all of the following are true.

1. OpenWiki is included in the EVK Docker image as an exact-version external dependency without forking it.
2. EVK does not require an additional OpenAI API key for the standard OpenWiki path.
3. OpenWiki can be initialized for a repository using Codex host-driven integration.
4. Generated canonical Wiki lives under `openwiki/` and is Git tracked.
5. Normal card/workspace execution reads but does not modify canonical OpenWiki.
6. Existing commit summary logic is evolved into a reusable Change Manifest model.
7. Commit messages are generated from the same semantic record.
8. Each workspace can emit immutable semantic events independently into the existing repository shared folder.
9. Parallel workspaces do not contend on Wiki files or semantic event files.
10. Source integration occurs before Wiki reconciliation.
11. Multiple merged cards can be reconciled in one OpenWiki run.
12. Change Manifest intent is treated as hint; integrated source/tests remain authoritative.
13. Wiki reconciliation has a repository-level single-writer guarantee.
14. Successful/no-op/failed reconciliation state is durable and retryable.
15. Failed Wiki maintenance cannot silently present stale Wiki as current.
16. Long-lived workspaces have persistent Workspace Memory for code-invisible semantic decisions.
17. Workspace Memory survives Codex context compaction and can be re-read afterward.
18. No raw conversation transcript is persisted as repository memory.
19. Existing EVK parallel-worktree behavior remains intact.
20. Relevant unit/integration/parallel/failure tests are added.
21. Existing project quality gates pass.
22. Implementation documentation explains lifecycle, recovery, upgrade, and troubleshooting.

---

# 43. Fundamental architecture

Final conceptual model:

```text
                      REPOSITORY
                  Source + Canonical Wiki
                           ▲
                           │
                     Wiki Reconciler
                           ▲
                           │
                Repository Shared Folder
                 Semantic Change Outbox
                   ▲       ▲       ▲
                   │       │       │
                Card A   Card B   Card C
                   │       │       │
                worktree worktree worktree
```

For each workspace:

```text
Conversation
     ↓
Workspace Memory
     ↓
Change Manifest
     ↓
source integration
     ↓
OpenWiki reconciliation
     ↓
Canonical Repository Memory
```

This separation is intentional.

Parallel coding remains parallel.

Only canonical knowledge publication is serialized.

That is the core architectural invariant of this feature.