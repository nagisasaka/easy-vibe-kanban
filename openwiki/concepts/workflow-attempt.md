---
type: concept
title: Workflow Attempt とグラフの契約
description: Issue 専用の実施グラフ、安定 Session、run snapshot、辺の発火・分岐・再実行の意味。
tags: [workflow, graph, attempt, condition, lifecycle]
sources:
  - id: openwiki-source-b40ff99339e2929bded4b58c
    resource: repo://crates/db/src/models/workflow.rs
  - id: openwiki-source-a517c0f8e44d2cf0c440fbfc
    resource: repo://crates/server/src/routes/workflows.rs
  - id: openwiki-source-864d86b798cce4d7e451a363
    resource: repo://crates/server/src/workflow_runtime/condition_router.rs
  - id: openwiki-source-62bbc78a91edc9656dab8447
    resource: repo://crates/server/src/workflow_runtime/envelope.rs
  - id: openwiki-source-6f2595571056199f0f83415c
    resource: repo://crates/server/src/workflow_runtime/runner.rs
  - id: openwiki-source-5bd46b87a60accbf6cd6da13
    resource: repo://crates/workflow/src/planner.rs
  - id: openwiki-source-b388436d40b78500f0d81684
    resource: repo://crates/workflow/src/transform.rs
  - id: openwiki-source-bfc7d8936f7f60c78ed7b911
    resource: repo://crates/workflow/src/validation.rs
  - id: openwiki-source-45838aa0b8eb7ea92723e949
    resource: repo://docs/future/ai-workflow/spec-product.md
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
generated: { by: "codex", at: "2026-09-21T08:16:36.701Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-21T08:16:36.701Z
---

# Workflow Attempt とグラフの契約

**WorkflowAttempt** は [Issue](project-and-issue.md) を解決する一つの実施であり、専用グラフ、共有 [Workspace](workspace.md)、ノードの [Session](session-and-agent-run.md) をまとめる。Workflow は定義、WorkflowRun はその定義を起動した記録、NodeExecution は特定ノード・iteration の実行記録である。AgentRun の一回の retry と WorkflowAttempt 全体を同じ「attempt」と扱わない。[保存モデル](../../crates/db/src/models/workflow.rs#L66-L134)

## テンプレート・実施・run の寿命

Project 用定義と System 用定義があり、System テンプレートは通常 API から更新・削除できない。Issue 専用の backing Workflow はテンプレート一覧から隠して作成する。[作成](../../crates/server/src/routes/workflows.rs#L639-L658)、[System 保護](../../crates/server/src/routes/workflows.rs#L1109-L1123)、[削除拒否](../../crates/server/src/routes/workflows.rs#L1168-L1179)

資源を伴う Attempt 作成は、まず draft、次に main Workspace を作成・関連付け、Agent ノードに不足している Session ID を補ってグラフへ保存し、Ready にする。既存 Session ID を毎回作り直す処理ではない。複数 Repo を明示指定する場合は、それぞれ target branch が必要である。[資源準備](../../crates/server/src/routes/workflows.rs#L674-L739)、[Session 固定](../../crates/server/src/routes/workflows.rs#L743-L818)

通常の Session 準備は interactive Workspace を要求する。Repository bootstrap だけは専用の内部入口を通り、execution_only、bootstrap 所有者の Repo/run ID、未完了結果、Repo membership が保存済みであることを確認してから子 Session を作る。クライアントの指定で所有検証を回避する入口ではない。[二つの準備経路](../../crates/server/src/routes/workflows.rs#L743-L786)。用途と所有の一次説明は [Workspace](workspace.md#操作用途と実行所有者)に置く。

起動時はグラフ snapshot と実行状態を保存し、以後の runtime はその snapshot を使う。Attempt は最新 run ID と Workspace ID を保持し、draft/ready と running/awaiting/terminal の状態を区別する。編集されたテンプレートから進行中 run を再解釈する前提にはしない。[起動と snapshot](../../crates/server/src/workflow_runtime/runner.rs#L983-L1046)、[snapshot 読み込み](../../crates/server/src/workflow_runtime/runner.rs#L2054-L2105)、[状態と関連](../../crates/db/src/models/workflow.rs#L32-L47)

## ノードの役割

| 種類 | 意味 |
| --- | --- |
| Start / End | 起点と結果の構造。Start は入力、End は上流出力を結果にする |
| Agent | main Workspace の安定 Session で指示を実行する |
| Condition | router の判断から次の対象ノードを選ぶ |
| HumanGate | 明示的な人の判断まで待つ |
| Transform | テンプレート、正規表現抽出、文字数切り詰めによるテキスト変換 |
| Arena | 候補を作り、勝者の選択・適用を待つ |

[実行ノード型](../../crates/workflow/src/graph.rs#L105-L125)、[構造・待機 handler](../../crates/workflow/src/handlers.rs#L136-L158)、[Transform](../../crates/workflow/src/transform.rs#L28-L74)。StickyNote / StageGroup は canvas object であり、実行ノードに追加する種類ではない。[canvas 型](../../crates/workflow/src/graph.rs#L89-L94)

[Arena](arena.md) の候補は別 Workspace に隔離される例外で、勝者の変更を main Workspace に適用してから後続へ進む。通常の Agent ノード間は同じ作業ファイルを共有する。このため、並行 Agent の編集競合はグラフ表示だけでは解消されない。[共有モデルの記録](../../docs/future/ai-workflow/spec-product.md#1-核心结论)、[勝者適用](../../crates/server/src/workflow_runtime/arena.rs#L712-L793)

## 辺・分岐・合流

現行グラフ検証は version 1/2、一意の node ID、Start がちょうど一個、End が一個以上、存在する辺の両端、Start からの到達可能性を要求し、**循環を拒否する**。実行時には Condition 設定も追加検証する。[validation](../../crates/workflow/src/validation.rs#L43-L125)

辺は上流の成功を後続の起動に結び付ける。Condition からの辺だけは選択された target ID を出力 JSON と照合する。**複数入力があるノードは、いずれかの入力元が成功すれば ready になる**。全入力の成功を待つ barrier ではない。たとえば A と B の両方から Review に接続しても、A の成功時に Review が起動可能となる。[planner](../../crates/workflow/src/planner.rs#L150-L214)、[一方の成功で ready になるテスト](../../crates/workflow/src/planner.rs#L362-L405)

発火数は成功した上流実行から数え、NodeExecution は iteration を持つ。「一ノードだから run 中に常に一回だけ」という仮定も避ける。永続 orchestration の JoinPolicy と、この製品グラフの ready 判定は別の責任である。[発火数](../../crates/workflow/src/planner.rs#L118-L133)、[NodeExecution](../../crates/db/src/models/workflow.rs#L112-L134)、[runtime の責任分担](../architecture/workflow-runtime.md)

## コンテキストと結果

設計の中心は共有 worktree だが、現行実装にはテキスト入力もある。`{{input}}` / `{{run_input}}` は run 入力、`{{upstream}}` は上流出力を展開する。Agent は既定で Workflow context の envelope を付け、`include_workflow_context=false` ならノードの指示だけを返す。[prompt 構築](../../crates/server/src/workflow_runtime/runner.rs#L2808-L2857)

Transform は上流出力を使い、上流出力がなければ run 入力を使う。正規表現に一致しない場合はエラーになるので、「空文字で正常継続」と仮定しない。[context](../../crates/workflow/src/handlers.rs#L45-L55)、[変換失敗](../../crates/workflow/src/transform.rs#L51-L66)

共通 envelope が付ける Workflow Input は 8,000 文字、各 direct upstream handoff は 12,000 文字で切り詰め、切詰めを本文に明示する。これは envelope の上限であり、明示的な `{{upstream}}` の展開や provider の回答長全般に対する上限と同一視しない。[envelope](../../crates/server/src/workflow_runtime/envelope.rs#L1-L83)、[template 展開](../../crates/server/src/workflow_runtime/runner.rs#L2851-L2856)

[OpenWiki Bootstrap](../operations/openwiki-maintenance.md) は Reviewer / Refiner の完全なレポートを host 検証後に共有ファイルへ保存し、node 間では小さな参照を使う専用経路を持つ。通常 Workflow の handoff 上限は維持し、Reviewer に指摘数を絞る目的として流用しない。[参照の契約](../architecture/workflow-runtime.md#bootstrap-の成果物参照は通常の上流本文と分ける)

## 判断待ちと再試行

Condition router は構造化出力を解析・検証し、有効なら選択枝を進める。不正な出力や判断上の問題は AwaitingHuman に変換し、解析できた質問または検証理由を示す。人による選択も target の有効性を再検証する。詳細な応答形式と mutation 検出は [Workflow Runtime](../architecture/workflow-runtime.md) に置く。[router 結果](../../crates/server/src/workflow_runtime/condition_router.rs#L253-L332)

HumanGate は AwaitingHuman 状態で承認すると succeeded にして続きを駆動する。拒否はノード失敗、残る pending の skip、run 失敗になる。`Rejection` という edge kind が存在するだけで「拒否時に任意の次ノードへ分岐する」と説明しない。[承認](../../crates/server/src/workflow_runtime/runner.rs#L1487-L1517)、[拒否](../../crates/server/src/workflow_runtime/runner.rs#L1658-L1678)

ノード retry は Issue run の Failed な Agent / Condition / Transform に限る。ノードを reset し、下流の skipped ノードを戻して再駆動する。[AgentRun retry](session-and-agent-run.md) のように同一 provider 試行を Resume する API とは別である。Repository bootstrap の個別ノード retry は拒否され、所有する保守フローを通す。[retry 条件](../../crates/server/src/workflow_runtime/runner.rs#L1824-L1872)

Run/Attempt の Canceled と NodeExecution の Cancelled はシリアライズ名も異なる。待機状態やキャンセル中を、成功・失敗の終端と一括しない。[enum](../../crates/db/src/models/workflow.rs#L16-L64)。起動失敗、取消の伝搬、再起動回復は [runtime](../architecture/workflow-runtime.md) が説明する。

## 文書との照合と未確定点

[2026-05-16 の製品設計](../../docs/future/ai-workflow/spec-product.md) は合意された製品意図として有用だが、「上流出力を prompt に入れない」という記述は現在の template 展開・context 構築より狭い。また同文書の ExecutionProcess による agent 実行モデルは、現在の [AgentRun](session-and-agent-run.md) に読み替える必要がある。

[README の例](../../README.md) には戻り矢印があるが、現在の validation は循環を受け付けない。記録から将来の loop 支援時期や、現行 fan-in を全入力待ちへ変更する意図は確定できない。グラフやテンプレートの変更では、README の図だけでなく planner・validation のテストを判断基準にする。

Repository scope の System Workflow は Issue を持たず、[OpenWiki bootstrap](../operations/openwiki-maintenance.md) の所有・終了検証に使われる。一般の Issue Workflow の操作権限や自動回復をそのまま適用しない。
