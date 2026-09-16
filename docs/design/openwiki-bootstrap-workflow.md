# OpenWiki Bootstrap Review Workflow
## EVK 詳細設計仕様

Status: Implementation Ready  
Target: easy-vibe-kanban (EVK)  
Depends on: existing OpenWiki integration  
Canonical path: `docs/design/openwiki-bootstrap-workflow.md`

---

# 1. 目的

現在EVKでは、Repository SettingsでOpenWikiを有効化し、対象branchを指定して `Initialize Wiki` を実行すると、専用workspace/worktreeを作成し、CodexがOpenWiki MCPを利用して初期Wikiを生成する。

この仕組みを、EVK既存のサーバーサイドDAG / Workflow機能上で実行する方式へ変更する。

目的は、初期Wiki生成を一つのCodex sessionだけで完結させず、

1. Generator
2. Independent Reviewer
3. Refiner

を独立したCodex sessionとして実行し、初期plannerのcoverage不足を補うことである。

---

# 2. 基本原則

## 2.1 Workflow Attempt全体でworktreeを共有する

OpenWiki初期化1回につき、Wiki専用のWorkflow Attemptを1つ作成する。

Workflow Attempt全体では、現在のEVK Workflow機能が持つ「1つの共有workspace/worktree」を利用する。

新しいworktree管理機構を作らない。

```text
OpenWiki Bootstrap Workflow Attempt
        │
        └── shared Wiki worktree
              ├── Generate session
              ├── Review session
              └── Refine session
```

---

## 2.2 各Agent Stepはfresh Codex session

Generator / Reviewer / Refiner間でCodex conversation historyを共有しない。

必要な独立性はworktreeではなくagent cognitionの独立性である。

```text
shared repository state
        ↑
Codex A   Codex B   Codex C
Generate  Review    Refine
fresh     fresh     fresh
```

ReviewerはGeneratorのconversation、planning過程、reasoning historyを参照してはならない。

Reviewerが参照するのは、

- repository source/tests/config
- 完成したWiki v1

だけである。

---

# 3. /goal を使用しない

OpenWiki Bootstrap Workflow内部のCodex Agent StepではCodex `/goal` を使用しない。

理由:

- 各stepはbounded jobである
- 長時間継続性はEVK Workflow engineが担う
- retry / branch / condition / node stateはDAG側で管理する
- Goal state machineとの二重管理を避ける
- Reviewerをfresh contextで起動する必要がある

各Agent Stepには通常の明確なtask promptを与える。

---

# 4. Initialize Wiki UI

既存Repository Settings UIを原則維持する。

```text
OpenWiki: Enabled
Target branch: <branch>
[ Initialize Wiki ]
```

ユーザー操作は変更しない。

内部実装のみ、

```text
旧:
Initialize Wiki
 → dedicated workspace
 → single Codex/OpenWiki run

新:
Initialize Wiki
 → dedicated Workflow Attempt
 → system workflow: openwiki-bootstrap-v1
```

へ変更する。

既存target branch指定、workspace命名、OpenWiki enable state等は再利用する。

---

# 5. System Workflow Template

EVK内部にsystem-managed workflow templateを定義する。

例:

```text
openwiki-bootstrap-v1
```

通常のユーザーDAGと同じWorkflow engine上で実行する。

OpenWiki専用の別オーケストレーターや独自state machineを実装してはならない。

System workflowであることを示す内部metadataを持たせてよい。

---

# 6. Workflow概要

```text
[Preflight]
    │
    ▼
[Generate Wiki]
 Codex A
 fresh session
 OpenWiki init
    │
    ▼
[Commit Wiki v1]
    │
    ▼
[Independent Coverage Review]
 Codex B
 fresh session
 same worktree
    │
    ▼
[Review Decision]
   ┌──────────────┐
   │              │
 PASS      NEEDS_REFINEMENT
   │              │
   │              ▼
   │        [Refine Wiki]
   │         Codex C
   │         fresh session
   │         force update
   │              │
   │              ▼
   │        [Commit Wiki v2]
   │              │
   └───────┬──────┘
           ▼
      [Final Validate]
           │
           ▼
        [Complete]
```

初期実装ではRefineは最大1回とする。

無限review/refine loopを実装しない。

---

# 7. Preflight

既存OpenWiki初期化前処理を再利用する。

最低限:

- OpenWiki enabled
- target branch解決
- OpenWiki CLI利用可能
- Codex integration利用可能
- Wiki専用shared worktree作成
- repository state確認

既存実装に相当処理がある場合、それをWorkflow nodeから再利用する。

---

# 8. Generate Wiki — Codex A

新規Codex sessionを開始する。

Codex AはOpenWiki host-driven integrationを使い、

```text
openwiki_begin(mode="init")
→ repository research
→ openwiki_submit_plan
→ page generation
→ openwiki_finish
```

を完了する。

OpenWiki内部を再実装しない。

既存のOpenWiki initial generation prompt / integrationを可能な限り再利用する。

成功条件:

- `openwiki_finish` 成功
- generated Wikiがworktree内に存在
- OpenWiki validationが成功

---

# 9. Wiki v1 Commit

Generator完了後のrepository stateを固定するためWiki v1をcommitする。

Reviewerはこの確定状態をレビュー対象とする。

commit message例:

```text
docs(openwiki): initialize repository wiki
```

既存のOpenWiki commit処理がある場合は再利用する。

commit SHAをWorkflow contextへ保持する。

---

# 10. Independent Coverage Review — Codex B

Codex Bは必ずfresh sessionとする。

同じWiki worktreeを使用する。

Reviewerの目的は「Wikiを書き直す」ことではなく、

> repositoryを将来変更するcoding agentに必要なsemantic knowledgeがWikiから欠落していないかを独立評価する

ことである。

ReviewerはGenerator sessionのconversation/historyを受け取ってはならない。

---

# 11. Reviewer input

Reviewerには最低限以下を与える。

- repository source/tests/config
- current OpenWiki
- review taxonomy
- structured output requirement

Generatorのplan、conversation transcript、reasoning historyは与えない。

---

# 12. Review taxonomy

最低限以下を確認する。

- system architecture / boundaries
- domain model / entity relationships
- lifecycle / state transitions
- end-to-end development or runtime workflows
- execution/configuration model
- persistence / synchronization / views
- cross-component dependencies
- concurrency semantics
- cancellation / failure semantics
- important invariants
- dangerous modification points
- repository-specific development conventions

レビューは「ファイルが説明されているか」ではなく「semantic domainがcoverageされているか」を評価する。

---

# 13. Reviewer output

structured resultを使用する。

概念schema:

```ts
interface OpenWikiCoverageReview {
  version: 1;

  verdict: "pass" | "needs_refinement";

  findings: Array<{
    category: string;
    severity: "material" | "minor";
    title: string;
    description: string;
    evidencePaths: string[];
    recommendedAction:
      | "add_page"
      | "expand_existing_page"
      | "verify_claim";
    suggestedPage?: string;
  }>;

  summary: string;
}
```

`verdict=needs_refinement` は原則としてmaterial findingが存在するときのみ使用する。

単なる文体改善や軽微な説明不足でRefineを起動しない。

---

# 14. Review artifact

Review結果はworktree内のOpenWiki Markdownへ直接書かない。

優先順位:

1. 既存Workflowのstructured output/artifact mechanism
2. それが不足する場合は既存repository shared folder

新しい汎用artifact storeを作らない。

shared folderを使う場合もGit管理対象にはしない。

---

# 15. Reviewerはread-only

Codex Bはrepository / OpenWikiを変更してはならない。

可能なら既存sandbox/read-only mechanismを利用する。

技術的に完全なread-only化が難しい場合でも、step終了時にworktree diffを検査し、Reviewerによる変更があればfailure扱いとする。

---

# 16. Review Decision

Workflow condition nodeで、

```text
review.verdict == "pass"
```

ならRefineをskipしてFinal Validateへ進む。

```text
review.verdict == "needs_refinement"
```

ならRefineへ進む。

この条件分岐は既存DAG engineのcondition機能を利用する。

専用if/elseオーケストレーションを実装しない。

---

# 17. Refine Wiki — Codex C

Codex Cもfresh sessionとする。

入力:

- current repository
- Wiki v1
- Reviewerの最終structured findings

Codex Bのconversation/historyは与えない。

Review findingsはsource of truthではなく仮説として扱う。

必ずsource/tests/configで再検証する。

---

# 18. OpenWiki force update

RefineではOpenWiki 0.5.1のhost-driven MCPを利用する。

sourceが初期生成時から変化していないため、

```text
openwiki_begin(
  mode = "update",
  force = true
)
```

を使用する。

再`init`してはならない。

既存Wiki / Claimsを維持しつつ不足coverageを補完する。

Review findings自体をOpenWiki MCPへ直接渡す専用APIはないため、findingsはCodex Cのtask contextへ渡す。

Codex Cがfindingsを検証し、必要なpageをOpenWiki planへ反映する。

page planでは必要に応じて:

- purpose
- seedPaths
- instructions

へreview intentを落とす。

---

# 19. Refine prompt principle

Codex Cへは概ね次の契約を与える。

```text
Independent review found the following possible coverage gaps.

Treat them as hypotheses, not truth.
Verify them against repository source/tests/configuration.

Use OpenWiki update with force=true.

Add or expand documentation only for findings that are materially
important and supported by repository evidence.

Do not unnecessarily rewrite correct existing pages.
```

---

# 20. Wiki v2 Commit

Refineが実行された場合、その結果を別commitとして保存する。

例:

```text
docs(openwiki): refine initial wiki coverage
```

v1 commitをamend/squashすることは必須にしない。

既存Git workflowとの整合上、安全にsquashできる既存機構がある場合のみ利用してよい。

---

# 21. Final Validate

PASS経路、Refine経路ともFinal Validateを通す。

最低限:

- OpenWiki generated state exists
- OpenWiki finalization成功
- worktreeに未意図変更がない
- expected commits存在
- Reviewer artifact整合
- workflow state整合

既存OpenWiki validation / integration testsを再利用する。

---

# 22. Publish / Existing Integration

既に実装済みの

- target branchへの反映
- commit/push policy
- OpenWiki status
- repository metadata
- failure/retry handling

は原則変更しない。

今回の実装はbootstrap orchestrationの変更であり、既存publication機構を作り直すものではない。

必要なadapterだけ追加する。

---

# 23. 通常のWiki maintenanceは変更しない

今回の変更対象は初期Wiki bootstrapのみ。

既に実装済みの、

```text
EVK development
→ Change Manifest
→ source integration
→ normal OpenWiki update
```

というincremental maintenance flowは変更しない。

将来的にmaintenance自体をWorkflow化する余地はあるが、本実装のscope外とする。

---

# 24. Failure semantics

## Generate失敗
Workflow failure。Reviewへ進まない。

## Commit v1失敗
Workflow failure。

## Review失敗
Wiki v1は保持する。
WorkflowをError/Retryableとして扱う。

## Reviewerがfile変更
Review node failure。

## Refine失敗
Wiki v1を保持する。
既存の生成済みWikiを破壊しない。
Retry可能にする。

## Final Validate失敗
Complete扱いにしない。

既存Workflow retry semanticsを優先して利用する。

---

# 25. Observability / UI

既存Initialize Wiki UIは維持しつつ、可能ならWorkflow node状態を表示する。

例:

```text
Initializing Wiki

✓ Generate
✓ Commit initial wiki
● Independent review
○ Refine if needed
○ Final validation
```

ユーザーへDAG editorを強制表示する必要はない。

詳細画面からWorkflow Attemptを参照可能にしてよい。

---

# 26. Auditability

各Agent Stepについて最低限:

- session ID
- start/end
- result
- model
- relevant commit SHA
- structured review artifact

を既存Workflow audit mechanismで追跡可能にする。

新しいaudit subsystemは作らない。

---

# 27. Tests

最低限追加する。

## Workflow construction
- Initialize Wikiがsingle-agent attemptではなくOpenWiki Bootstrap Workflowを起動する
- shared worktreeが全stepで同一

## Fresh session
- Generate / Review / Refineが異なるagent session IDを持つ

## PASS path
- Generate成功
- Review pass
- Refine skip
- Complete

## REFINE path
- Review `needs_refinement`
- Refine実行
- `update + force`経路を使用
- Complete

## Reviewer isolation
- ReviewerにGenerator conversationを渡さない
- Reviewerによるfile変更を検出

## Failure/retry
- Review failure
- Refine failure
- retryしてもworktree/stateが破壊されない

## Existing behavior
- target branch / commit / publish処理が維持される
-通常OpenWiki maintenance flowがregressionしない

real model callをCI必須にしない。

---

# 28. Acceptance Criteria

以下をすべて満たしたら完了。

1. `Initialize Wiki` が既存EVK Workflow engine上のsystem workflowとして実行される。
2. Workflow Attempt全体で1つのWiki worktreeを共有する。
3. Generate / Review / Refineはそれぞれfresh Codex sessionである。
4. `/goal`を使用しない。
5. Generatorは既存OpenWiki host-driven initを利用する。
6. 初期Wiki生成後に独立Coverage Reviewを実行する。
7. ReviewerはGenerator conversation/historyを参照しない。
8. Reviewerはrepository/OpenWikiを変更しない。
9. Reviewer結果はstructured artifactとして後続stepへ渡る。
10. material gapがなければRefineをskipする。
11. material gapがある場合のみfresh Refinerを起動する。
12. RefinerはOpenWiki `update` + `force=true`を利用し、再initしない。
13. Review findingsは仮説としてsource/testsで再検証される。
14. 既存OpenWikiを不必要に全面再生成しない。
15. Workflowの条件分岐・retry・state管理には既存DAG基盤を使う。
16. 既存target branch / publication / status管理を維持する。
17. 通常のincremental OpenWiki maintenance flowを壊さない。
18. 主要なPASS / REFINE / failure pathにテストがある。
19. 既存lint/typecheck/test/buildが通る。
20. 実装・運用方法を文書化する。

---

# 29. Core invariant

本機能の最重要設計原則:

```text
Worktree = repository state isolation
Session  = reasoning isolation
Workflow = orchestration
OpenWiki = repository knowledge state
```

Generator / Reviewer / Refinerは同じrepository state上で協調するが、互いのconversation historyを共有しない。

これにより、ReviewerがGeneratorの最初のplanningにアンカーされることを防ぎながら、追加worktreeを作る複雑性も避ける。