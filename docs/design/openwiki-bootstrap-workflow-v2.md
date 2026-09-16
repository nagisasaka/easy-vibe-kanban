# OpenWiki Bootstrap Workflow
## Simplified Design Specification

Target: easy-vibe-kanban (EVK)  
Scope: OpenWiki初期生成フローのみ  
Path: `docs/design/openwiki-bootstrap-workflow.md`

# 1. 目的

Repository Settings の `Initialize Wiki` を、EVK既存Workflow/DAG runtimeを利用した以下の処理へ変更する。

```text
Generate
   ↓
Independent Review
   ↓
pass ──────────────┐
                   │
needs_refinement   │
   ↓               │
Refine             │
   └───────────────┘
          ↓
      Publish
```

目的は、初期Wikiを生成したCodexとは別のfresh Codex sessionでcoverageを独立評価し、重要な欠落があればさらに別のfresh Codex sessionでOpenWikiを補完することである。

OpenWikiはfork/vendor/patchしない。

# 2. Scope外

今回実装しないもの:

- 通常開発後のincremental OpenWiki Sync変更
- Change Manifest変更
- Wiki v1/v2の途中commit
- 中間checkpoint
- node単位の完全resume
- generic Workflow retryの修正
- Reviewer findingsの長期永続管理
- Refine後の再レビューloop
- coverage完全性の機械的保証
- 新しいartifact store / scheduler / audit subsystem

Bootstrapが途中で失敗した場合は、途中再開せず新しいBootstrap runとして最初から再実行する。

# 3. 基本原則

```text
Worktree = repository state
Session  = reasoning state
Workflow = orchestration
OpenWiki = repository knowledge backend
```

一回のBootstrapでは対象repository用の専用Workspace/worktreeを一つだけ使用する。

Generate / Review / Refineは同じworktreeを共有するが、すべて別のEVK Session / AgentRunとする。

conversation historyは共有しない。

# 4. Repository-scoped Workflow

現行WorkflowAttemptはIssue-scopedなので、OpenWikiのために架空Issueを作成してはならない。

既存Workflow runnerを最小限一般化し、Repository Settingsからrepository-scopedなsystem Workflow executionを起動できるようにする。

既存のSystem Workflow template / UUID / graph snapshot方式を再利用する。

通常のIssue Workflowは変更しない。

# 5. 既存OpenWiki lifecycleの再利用

現在のOpenWiki処理が持つ以下を可能な限りそのまま利用する。

- repository lock
- target branch / source SHA確定
- dedicated Workspace/worktree作成
- OpenWiki setup
- Native Audit / completion proof
- Wiki-only commit
- target branchへの既存publication
- receipt / RepositoryMemoryState更新

新しいpublication実装を作らない。

# 6. Maintenance ownership

現在のOpenWiki maintenanceは単一Session/AgentRunをownerとしている。

Bootstrap時だけ、ownerをBootstrap Workflow execution全体へ拡張する。

```text
Repository maintenance lease
          │
    Bootstrap Workflow
      ├─ Generate
      ├─ Review
      └─ Refine
```

Workflow実行中はleaseを保持し続ける。

Generate終了時にpublicationしてはならない。

子AgentRunがOpenWiki writerとして動けるのは、そのBootstrap Workflowの子である場合だけとする。

最終publication完了後またはWorkflow失敗時にleaseを解放する。

通常Syncは従来のsingle-AgentRun ownershipを維持する。

# 7. Generate

fresh Codex sessionを開始する。

既存OpenWiki host-driven integrationを使う。

```text
openwiki_begin(mode=init)
→ repository research
→ submit_plan
→ page generation
→ openwiki_finish
```

AgentRunの正常終了だけでは成功扱いにしない。

既存Native Audit / OpenWiki completion proofで `openwiki_finish` 成功を確認してGenerate成功とする。

Generate後にGit commitしない。

生成された`openwiki/`はそのまま同じworktree上に残す。

# 8. Independent Review

Generateとは別のfresh Codex sessionを同じworktreeで起動する。

ReviewerにGeneratorの以下を渡してはならない。

- conversation
- terminal summary
- upstream workflow context
- planning history

`include_workflow_context=false` とする。

Reviewerが見るもの:

- repository source/tests/config
- Generate済みOpenWiki
- review taxonomy
- structured output schema

ReviewerはReadOnly sandboxで実行し、OpenWiki writer用MCP/Skillを与えない。

終了後、Reviewerによるrepository mutationがないことをserver側で確認する。

# 9. Review内容

Reviewerの目的は文章品質評価ではなくsemantic coverage評価である。

最低限確認する:

- architecture / boundaries
- domain model / entity relationships
- state / lifecycle
- end-to-end workflows
- execution / configuration
- persistence / synchronization / views
- concurrency
- failure / cancellation semantics
- cross-component dependencies
- important invariants
- dangerous modification points
- repository-specific development conventions

# 10. Review出力

Review結果は小さなJSONとしてNodeExecution outputに保持する。

例:

```ts
interface OpenWikiCoverageReview {
  version: 1;
  verdict: "pass" | "needs_refinement";

  findings: Array<{
    severity: "material" | "minor";
    title: string;
    description: string;
    evidencePaths: string[];
    recommendedAction:
      | "add_page"
      | "expand_page"
      | "verify_claim";
  }>;

  summary: string;
}
```

server側でJSON schema validationしてからReview成功とする。

`needs_refinement`はmaterial findingが存在するときだけ。

既存handoff制限を超えないよう、finding数とdescription長には合理的な上限を設ける。

専用artifact storeは作らない。

# 11. PASS / REFINE判定

既存Condition nodeのLLM routerは使用しない。

validated Reviewer JSONの`verdict`から決定的に:

```text
pass             → Publish
needs_refinement → Refine
```

を選択する。

汎用expression languageは作らない。

既存Workflow runtimeへ、このsystem workflowで必要な最小のdeterministic branch selectionを追加する。

既存LLM Conditionの挙動は変更しない。

# 12. Refine

`needs_refinement`の場合だけfresh Codex sessionを起動する。

入力:

- current repository
- current OpenWiki
- validated Reviewer findings

Reviewer conversationは渡さない。

findingsは仮説として扱い、source/tests/configurationで再検証する。

OpenWiki 0.5.1:

```text
openwiki_begin(
  mode = update,
  force = true
)
```

を使用する。

再initしない。

妥当なfindingだけを新規ページ追加または既存ページ補強としてOpenWiki planへ反映する。

Refine後もAgentRun終了だけでは成功扱いにせず、OpenWiki completion proofを確認する。

# 13. OpenWiki setup

Generate / RefineはOpenWiki writerとして必要なsetupを使用する。

Reviewではwriter用MCP/Skillを使用しない。

現在のAGENTS/CLAUDE prepare/restoreが単一host run前提の場合、Generate / Review / Refineのphase境界で安全に利用できる最小限の修正を行う。

Reviewerに一時的なwriter instructionを残さないこと。

# 14. Publication

Workflow中はworktree HEADを開始時source SHAから動かさない。

Generate / Review / Refine完了後だけ、既存publicationを一度実行する。

既存処理による:

```text
setup restore
→ source SHA safety validation
→ Wiki-only commit
→ target branchへのsquash反映
→ receipt
→ RepositoryMemoryState更新
```

を再利用する。

# 15. Failure

Generate / Review / Refine / final validationのいずれかが失敗した場合:

- publicationしない
- running child AgentRunを停止
- OpenWiki setupを復元
- maintenance leaseを解放
- Workflowをfailedとする

途中状態からのresumeは今回実装しない。

再実行時は新しい専用Workspace/worktreeと新しいBootstrap Workflow executionを作り、最初から実行する。

これにより既存generic retryや残存OpenWiki runとの複雑な整合を今回のscopeから除外する。

# 16. Initialize / Sync

Repository Settingsの既存UI/APIを維持する。

対象branchでOpenWiki未初期化ならBootstrap Workflowを起動する。

既にOpenWikiが存在する通常Syncは、現在実装済みのincremental maintenance pathをそのまま利用する。

# 17. UI

既存の:

```text
OpenWiki Enabled
Target Branch
[ Initialize Wiki ]
```

を維持する。

必要なら内部Workflow状態を:

```text
Generating
Reviewing
Refining
Publishing
```

として表示する。

DAG editorの表示を必須にしない。

# 18. Tests

最低限:

### Repository scope
- IssueなしでBootstrapを起動できる
- 通常Issue Workflowはregressionしない

### Shared worktree / fresh sessions
- Generate / Review / Refineが同じworktree
- Session / AgentRun IDは別

### Ownership
- Workflow実行中は別OpenWiki maintenanceを開始できない
- Generate終了時にpublicationされない

### Review isolation
- upstream conversation/contextがReviewerへ入らない
- ReadOnly
- writer MCP/Skillなし
- mutation検出

### PASS
```text
Generate → Review(pass) → Publish
```

### REFINE
```text
Generate → Review(needs_refinement)
→ Refine(update+force)
→ Publish
```

### Failure
- 各phase failureでpublishしない
- cleanup / lease release
- 新規Bootstrapとして再実行可能

### Publication
-途中commitなし
- final phaseだけ既存Wiki commit / target反映

### Regression
- normal OpenWiki Sync
- Issue Workflow
- LLM Condition
- existing publication

# 19. Acceptance Criteria

1. Repository SettingsからIssue無しでBootstrap Workflowを起動できる。
2. 既存Workflow runnerを利用する。
3. Workflow全体がOpenWiki maintenance ownerとなる。
4. Generate / Review / Refineは同一worktree・別fresh session。
5. ReviewerはGenerator contextを受け取らない。
6. ReviewerはReadOnlyでOpenWiki writer能力を持たない。
7. Review JSONをserver側でvalidateする。
8. PASS/REFINEを決定的に分岐する。
9. material gapがある場合のみRefineする。
10. RefineはOpenWiki `update + force=true`。
11.途中commit/checkpointを作らない。
12.途中失敗時はpublishせず、再実行は最初から行う。
13. final publicationを既存経路で一度だけ実行する。
14. 通常OpenWiki Syncを変更しない。
15. PASS / REFINE / failureの主要テストが通る。
16. 既存Workflow/OpenWiki quality gatesにregressionがない。