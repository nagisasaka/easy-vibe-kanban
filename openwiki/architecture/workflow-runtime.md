---
type: architecture
title: Workflow の実行と耐久オーケストレーション
description: 凍結グラフから AgentRun を起動する経路、outbox・inbox・lease の役割、分岐検証と取消・再起動時の所有境界。
tags: [workflow, orchestration, recovery, lifecycle]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-16T18:13:42.498Z
sources:
  - id: openwiki-source-a13fe4db1eee073d0a7e2c4d
    resource: repo://crates/server/src/main.rs
  - id: openwiki-source-a517c0f8e44d2cf0c440fbfc
    resource: repo://crates/server/src/routes/workflows.rs
  - id: openwiki-source-ed8c84278dba8a1f45af40e9
    resource: repo://crates/server/src/startup.rs
  - id: openwiki-source-04d5adb51b5929dacce999bc
    resource: repo://crates/server/src/workflow_runtime/bootstrap.rs
  - id: openwiki-source-864d86b798cce4d7e451a363
    resource: repo://crates/server/src/workflow_runtime/condition_router.rs
  - id: openwiki-source-6f2595571056199f0f83415c
    resource: repo://crates/server/src/workflow_runtime/runner.rs
  - id: openwiki-source-bef089692ac7606992050919
    resource: repo://crates/services/src/services/openwiki/bootstrap/reports.rs
  - id: openwiki-source-b19d02455156008b43c210c8
    resource: repo://crates/services/src/services/orchestration.rs
  - id: openwiki-source-45838aa0b8eb7ea92723e949
    resource: repo://docs/future/ai-workflow/spec-product.md
  - id: openwiki-source-ffc55e6e31e6ce477ca57329
    resource: repo://packages/web-core/src/shared/hooks/useWorkflowRun.ts
generated: { by: "codex", at: "2026-09-16T18:13:42.498Z" }
---

# Workflow の実行と耐久オーケストレーション

## 純粋な計画と副作用の分離

`workflow` crate はグラフの検証と実行可能 node の計画を行う。server の runner は Workspace、Session、DB の node execution、AgentRun を接続する。[Workflow Attempt](../concepts/workflow-attempt.md)が説明する辺・反復の意味を、DB やプロセスの識別へ落とす境界である。

通常 run の開始では、実行用グラフを検証し、主 Workspace を作成または再利用し、Agent node の Session を確保する。その後 `workflow_runs.graph_snapshot`、orchestration run、node execution の対応を保存してから駆動する。[開始経路](../../crates/server/src/workflow_runtime/runner.rs#L983-L1046)

run は開始時のグラフを保持する。復旧時も snapshot を優先し、現在のテンプレートへ無条件に切り替えない。nullable fallback は最小テスト fixture の互換用途として記録されている。[保存と再読込](../../crates/server/src/workflow_runtime/runner.rs#L2054-L2105)

## durable orchestration が保証するもの

| 記録 | 役割 |
| --- | --- |
| plan snapshot / node identity | dispatch 前に、どの run・node execution かを固定 |
| outbox | AgentRunPort へ渡す Create・Retry・Cancel 等の耐久コマンド |
| inbox / consumption | canonical event と、どの下流が終端事実を消費したかの識別 |
| lease | dispatcher の一時的な処理所有権 |

`OrchestrationService` は frozen plan を先に永続化し、node identity を準備する。[service](../../crates/services/src/services/orchestration.rs#L67-L105)。inbox の join 処理は製品 planner から独立しており、再送・再起動による二重の下流実行を防ぐ消費 identity を確立する。Workflow の辺の意味を `OrchestrationJoinPolicy::All` というフィールドだけから判断してはいけない。製品の Ready 判定は別に存在する。[境界を明示するコメント](../../crates/services/src/services/orchestration.rs#L417-L449)、[plan への変換](../../crates/server/src/workflow_runtime/runner.rs#L1126-L1167)

watcher や lease が失われただけで実行を失敗・置換しない。startup は in-flight outbox、処理中 inbox、lease を照合し、子の状態を回復してから inbox を replay する。[再起動処理](../../crates/services/src/services/orchestration.rs#L1044-L1075)。実プロセスへの再接続は [Agent Runtime](agent-runtime.md)が所有する。

## 実行の前進と待機

driver は node iteration と snapshot を読み、Ready node を順に起動する。Agent が Started を返すと Session・AgentRun・orchestration node の ID を node execution に保持して待機する。driver の Pause はサーバー thread のブロック待ちではなく、次のイベント等による照合まで駆動を返す意味である。[driver](../../crates/server/src/workflow_runtime/runner.rs#L2248-L2322)、[起動結果の保存](../../crates/server/src/workflow_runtime/runner.rs#L2392-L2416)

通常 Workflow watcher は canonical AgentRun のリンクを列挙して購読し、Repository scope の run を対象から外す。Repository Memory の独自完了検証を通常 watcher が先取りしないための境界になる。[watcher の対象](../../crates/server/src/workflow_runtime/runner.rs#L3207-L3263)

### 起動形態と前進の契機

| 起動・要求 | 通常 Workflow を進める主体 |
| --- | --- |
| Tauri が使う `startup::initialize_deployment` | 起動時回復の後に completion watcher を開始する |
| standalone `main.rs` | 起動時回復と ScheduledTask loop はあるが、通常 completion watcher の起動呼出しはない |
| Run 取得 GET / Run events の SSE 接続 | handler 自身が reconciliation を実行する |
| 活動中の Run を表示する UI | `useWorkflowRun` は既定4秒ごとに GET する。設定で停止・変更可能 |

[startup の結線](../../crates/server/src/startup.rs#L214-L225)・[standalone の回復と定時 loop](../../crates/server/src/main.rs#L87-L133)・[GET](../../crates/server/src/routes/workflows.rs#L1954-L1971)・[SSE 接続時](../../crates/server/src/routes/workflows.rs#L1992-L2011)・[UI polling](../../packages/web-core/src/shared/hooks/useWorkflowRun.ts#L26-L56)

したがって Run の取得は純粋な表示読み取りではない。画面経由では前進しても、standalone を画面なしで動かす場合に同じ継続駆動を保証する根拠にはならない。ScheduledTask loop は定時の起動を担い、完了後の全ノード連鎖を監視する watcher の代わりではない。[定時実行の skip 条件](../workflows/task-to-integration.md#定時に-workflow-を起動する)と併せて、起動に成功した run が進行し続けるかを別に確認する。

[製品設計の確認稿](../../docs/future/ai-workflow/spec-product.md)は「ノード完了後に自動で後続を起動する」という意図を記録する。上表は現行呼出し経路の適用差であり、無人運転での停止を再現した試験結果ではない。起動形態を問わない保証へ変更する意図や時期は、この証拠からは確定できない。

## Condition と人の判断

通常 Condition router は候補となる接続先、上流情報、作業木の情報を用いて判断する。prompt は read-only inspection を要求するが、それだけを隔離保証にしない。完了時は作業木の変化も照合する。[router prompt](../../crates/server/src/workflow_runtime/condition_router.rs#L125-L158)、[完了検証](../../crates/server/src/workflow_runtime/runner.rs#L3389-L3454)

出力は一つの `workflow_router_decision` block の schema v1 JSON として解析する。不正な形式、複数 block、接続先の不整合、十分でない確信、作業木変更等では AwaitingHuman に置き、推測した分岐で先へ進めない。`medium` confidence も自動進行しないことは focused test に明示されている。[parser](../../crates/server/src/workflow_runtime/condition_router.rs#L347-L393)、[confidence test](../../crates/server/src/workflow_runtime/condition_router.rs#L770-L783)

Human Gate と Arena の選択も製品の待機境界である。Arena は別の隔離 Workspace 群を使うため、通常 Agent Step の共有作業木と混同しない。[Arena](../concepts/arena.md)で選択・反映の意味を確認する。

## 取消とシステム所有 run

取消は Workflow を Cancelling にして node と子実行へ伝え、照合を続ける。終端 run への取消は現在の結果を返す。outbox は enqueue 時だけでなく delivery 時にも親状態を確認し、取消後に古い Create/Retry が残っていても新しい AgentRun を起動しない。Cancel の配送は残す。[取消経路](../../crates/server/src/workflow_runtime/runner.rs#L1681-L1729)、[配送ガード](../../crates/services/src/services/orchestration.rs#L136-L207)

Repository scope の Workflow は maintenance owner だけが駆動できる。一般開始 API は server-managed decision source を拒否する。bootstrap の node には `requires_product_validation` を設定し、provider の成功と製品上の成功を分ける。[所有ガード](../../crates/server/src/workflow_runtime/runner.rs#L983-L996)、[駆動ガード](../../crates/server/src/workflow_runtime/runner.rs#L2262-L2265)、[製品検証 flag](../../crates/server/src/workflow_runtime/runner.rs#L1142-L1164)。具体的な検証・再試行は [OpenWiki maintenance](../operations/openwiki-maintenance.md)が正本である。

### Bootstrap の成果物参照は通常の上流本文と分ける

OpenWiki Bootstrap は、通常 Workflow の会話引継ぎへ Review の全文を流さない。host が Reviewer の JSON と evidence path を検証し、repository shared storage へ完全なレポートを保存する。node output には repository・source SHA・Workflow run・Session・AgentRun・phase の identity、digest、verdict と件数だけを残す。[Review 完了時](../../crates/server/src/workflow_runtime/bootstrap.rs#L504-L533)、[保存と参照](../../crates/services/src/services/openwiki/bootstrap/reports.rs#L15-L38)

system Condition はこの検証済み verdict から `pass` / `refine` を決定的に選び、通常の LLM Condition を呼ばない。Refiner には host が決めたファイル参照を渡す。参照の identity と内容 digest は再読込時に検証され、共有ファイルが改変された場合は再生成で隠さず拒否する。したがって、通常の上流 handoff の文字数制限を緩めずに Review の情報を保持できる。[分岐](../../crates/server/src/workflow_runtime/runner.rs#L2435-L2464)、[Refine dispatch](../../crates/server/src/workflow_runtime/bootstrap.rs#L320-L332)、[参照の再検証](../../crates/services/src/services/openwiki/bootstrap/reports.rs#L72-L111)

この仕組みは任意ファイルを下流 prompt に挿入する汎用 artifact 機構ではない。詳細なレビュー契約と publication 前の再検証は [OpenWiki maintenance](../operations/openwiki-maintenance.md)へ集約する。

## 検証の入口

[outbox と lease の再起動テスト](../../crates/db/src/models/orchestration.rs#L2189-L2241)は配送中コマンドの回収を確認する。グラフ凍結、重複 dispatch、親取消、router 不正出力、bootstrap の製品検証を、それぞれの境界で確認する。過去の [Workflow V1 設計](../../docs/superpowers/specs/2026-05-08-ai-workflow-v1-design.md)にある仕組みは設計経緯であり、現行 runtime の保証を代用しない。
