---
type: concept
title: Session・AgentRun・RunAttempt
description: 会話・依頼・実行試行の識別、継続と retry、provider binding、Goal と停止の契約。
tags: [session, agent-run, run-attempt, goal, lifecycle]
sources:
  - id: openwiki-source-d20a82e2192a07c687b838cb
    resource: repo://crates/db/src/models/agent_runtime.rs
  - id: openwiki-source-3e5425718d87b437a12edd60
    resource: repo://crates/db/src/models/session.rs
  - id: openwiki-source-13210275fc6cb3d346bc25ae
    resource: repo://crates/executors/src/approvals.rs
  - id: openwiki-source-22b82518475034ade38104e6
    resource: repo://crates/executors/src/executors/codex.rs
  - id: openwiki-source-c88a7c6fabdac15166554032
    resource: repo://crates/executors/src/executors/codex/client.rs
  - id: openwiki-source-35b6fb5835c638575801a2b6
    resource: repo://crates/executors/src/executors/codex/goal_lifecycle.rs
  - id: openwiki-source-f4e0f9b2ce513d3f8b8b070e
    resource: repo://crates/executors/src/executors/provider_adapter.rs
  - id: openwiki-source-f07f62eba81c4ffbaf0db9d5
    resource: repo://crates/executors/src/profile.rs
  - id: openwiki-source-dc7ce5cfcb903cb959653602
    resource: repo://crates/executors/src/profile/runtime_identity.rs
  - id: openwiki-source-351583963abfe37bbf137f7e
    resource: repo://crates/executors/src/runtime/contracts.rs
  - id: openwiki-source-6e93b2f0738ee5fe4379942f
    resource: repo://crates/executors/src/runtime/reducer.rs
  - id: openwiki-source-788fa2f6f1a3d449a1a9efe4
    resource: repo://crates/local-deployment/src/agent_run_port.rs
  - id: openwiki-source-343f68138e6e2d3312d2774c
    resource: repo://crates/local-deployment/src/container.rs
  - id: openwiki-source-8fd7f4fe2d73e778cc63c256
    resource: repo://crates/local-deployment/src/process_host.rs
  - id: openwiki-source-8de115a31415ef9c911d6ac0
    resource: repo://crates/server/src/routes/scratch.rs
  - id: openwiki-source-406c792263884bbf462d666c
    resource: repo://crates/server/src/routes/sessions/agent_run.rs
  - id: openwiki-source-a74a882889154c2549e88f41
    resource: repo://crates/server/src/routes/sessions/executor_config.rs
  - id: openwiki-source-3a27c9d52911b8d12051a764
    resource: repo://crates/server/src/routes/sessions/mod.rs
  - id: openwiki-source-9c5d335ecd1178e88c47c0a8
    resource: repo://crates/services/src/services/queued_message.rs
  - id: openwiki-source-8be20616e789c6a7122c8e93
    resource: repo://docs/workspaces/chat-interface.mdx
  - id: openwiki-source-23db569cee004f42315c9f61
    resource: repo://docs/workspaces/sessions.mdx
  - id: openwiki-source-f133883544522a7fa5fc57b2
    resource: repo://packages/web-core/src/features/workspace-chat/model/canonicalAgentControls.ts
  - id: openwiki-source-14cc0373faba797a6e4d1fdd
    resource: repo://packages/web-core/src/features/workspace-chat/model/hooks/useSessionMessageEditor.ts
  - id: openwiki-source-713e0401e69764a879935acf
    resource: repo://packages/web-core/src/features/workspace-chat/model/hooks/useWorkspacePendingApproval.ts
  - id: openwiki-source-6b4d677fd9667cbdb6924409
    resource: repo://packages/web-core/src/features/workspace-chat/model/pendingSessionDraft.ts
  - id: openwiki-source-f0616990372eebb94bc69e6f
    resource: repo://packages/web-core/src/features/workspace-chat/ui/SessionChatBoxContainer.tsx
  - id: openwiki-source-9847c0126769012f07993d09
    resource: repo://packages/web-core/src/shared/hooks/useLocalStorageScratch.ts
  - id: openwiki-source-43d7a2f6bd06c556e7d48e3c
    resource: repo://packages/web-core/src/shared/hooks/useScratch.ts
generated: { by: "codex", at: "2026-09-22T01:26:32.834Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-22T01:26:32.834Z
---

# Session・AgentRun・RunAttempt

**Session** は [Workspace](workspace.md) 内の会話単位である。同じ Workspace の Session はファイルを共有する一方、会話履歴は別である。**AgentRun** は一つの実行依頼の正本、**RunAttempt** はその依頼を provider で実行する一回の試行を表す。Session 自体を OS プロセスの寿命と同一視しない。[Session の意図](../../docs/workspaces/sessions.mdx)、[保存モデル](../../crates/db/src/models/agent_runtime.rs#L47-L108)

## 識別と固定するもの

| 単位 | 所有する関係・契約 |
| --- | --- |
| Session | Workspace、executor、会話名・作業ディレクトリ |
| AgentRun | Session、Workspace、依頼入力、provider/profile、相関 ID、冪等キー |
| Turn | run に属する依頼の intent と入力メッセージ |
| RunAttempt | run/turn を参照し、試行番号、transport、capability snapshot、設定、Skill、native session 参照を固定 |
| Provider session | provider 内部の会話識別子。EVK Session と同じ UUID ではない |

[Session モデル](../../crates/db/src/models/session.rs#L22-L37)、[依頼 envelope](../../crates/executors/src/runtime/contracts.rs#L222-L237)、[試行 envelope](../../crates/executors/src/runtime/contracts.rs#L267-L292)

Attempt 検証は run/turn/Session/correlation の一致だけでなく、provider、profile、Workspace、設定と capability snapshot の整合性を要求する。Review に selected Skills を付けることや、FollowUp 以外への reset 指定も拒否する。[契約検証](../../crates/executors/src/runtime/contracts.rs#L294-L351)。これらを永続化してから起動する機構と監査の詳細は [Agent Runtime](../architecture/agent-runtime.md) に置く。

## 実行設定の復元と DEFAULT identity

profile 比較では variant 省略と正確な `DEFAULT` だけを同一視する。保存済み ID の表記は変更せず、`PLAN`・小文字の `default`・異なる provider は混同しない。既存履歴を migration で書き換えるのではなく、binding と request の比較境界で共通規則を使う。[identity](../../crates/executors/src/profile/runtime_identity.rs#L1-L13)

Session の設定復元は最新 RunAttempt の immutable launch request を読み、現在の可変 preset や script 実行から推測しない。起動前の native adoption では保存済み binding を使う。最新の保存値が壊れていたらエラーであり、古い run や別 Session の設定へ黙って戻さない。モデル・reasoning・mode・Goal budget 等の保存値と、provider が実行時に解決した既定値は区別する。[復元](../../crates/server/src/routes/sessions/executor_config.rs#L23-L79)、[順序・破損テスト](../../crates/server/src/routes/sessions/executor_config.rs#L147-L214)

## Follow-up と retry

通常 follow-up 経路は Session に非終端の AgentRun があれば拒否し、queue の使用を案内する。別 Session の会話は分離できても、同じ Workspace のファイルは共有されるので、Session を増やすだけで作業ファイルは隔離されない。[起動 guard](../../crates/server/src/routes/sessions/mod.rs#L314-L348)、[共有の契約](../../docs/workspaces/sessions.mdx)

- **Follow-up** は同じ Session に新しい AgentRun を作る。保存された provider session があれば FollowUp intent、なければ Initial になる。明示的な native resume 指定は Resume mode、通常の続きは Launch mode でも provider session 参照を渡す。
- **Retry** は終端になった既存 AgentRun の同じ turn を維持し、新しい attempt ID と増加した試行番号で再起動する。mode は Resume または Restart。Resume は対応 capability と観測済み provider session を要求し、Restart はその参照を持ち越さない。

[follow-up 選択](../../crates/server/src/routes/sessions/mod.rs#L392-L439)、[retry の契約](../../crates/local-deployment/src/agent_run_port.rs#L3016-L3081)

Retry は前回の設定・capability snapshot・Skill を引き継ぐ。設定を変えて別の仕事を始める操作と混同しない。制御リクエストも command ID、冪等キー、相関 ID を持って永続 command service を経由する。[retry 固定値](../../crates/local-deployment/src/agent_run_port.rs#L3049-L3079)、[制御 API](../../crates/server/src/routes/agent_runs.rs#L348-L373)

## Provider session の継続と取り込み

Session の executor は最初の指定で設定できるが、以後の別 executor 指定は拒否する。native binding を持った Session では provider、runtime profile、native session ID を別の値へ切り替えられない。別構成を使うときは新しい Session が境界になる。[executor 検証](../../crates/server/src/routes/sessions/agent_run.rs#L154-L171)、[binding 検証](../../crates/server/src/routes/sessions/agent_run.rs#L331-L374)

既存 native 会話を明示的に取り込む場合は正確な作業ディレクトリ scope が必要で、取り込み後は scope と profile fingerprint も維持する。同じ native 会話を別 EVK Session が所有していれば拒否する。履歴を表示できることと、任意のディレクトリ・profile で再開してよいことは別である。[scope 必須](../../crates/server/src/routes/sessions/mod.rs#L354-L381)、[継続検証](../../crates/server/src/routes/sessions/agent_run.rs#L375-L425)

Codex の thread 開始・再開では、app-server が設定から解決した model と reasoning effort を client が採用し、その後の turn 構築に用いる。EVK の依頼で effort override が未指定であることと、実行時の effort が未解決であることは同じではない。設定の継承と OpenWiki writer の起動前確認は [Provider 統合](../integrations/agent-providers.md)を参照する。[解決結果の採用](../../crates/executors/src/executors/codex/client.rs#L183-L190)、[開始・再開・turn](../../crates/executors/src/executors/codex/client.rs#L257-L322)

Codex の resume は同じ native thread ID を要求し、fork しない。再開設定の更新だけでは新しい developer instructions が会話履歴に現れない場合があるため、次の chat・Goal・review・compaction より先に `thread/inject_items` で現在の host context を注入する。空の context では注入せず、注入失敗なら継続を停止する。Goal objective の増量や擬似 user turn で代用しない。[再開処理](../../crates/executors/src/executors/codex/client.rs#L270-L306)、[順序と失敗のテスト](../../crates/executors/src/executors/codex/client.rs#L2237-L2327)。これにより、[Repository Memory の現在 run identity](repository-memory.md#change-manifest-は変更理由を運ぶイベント)と、[fresh 会話だけへ組み込む保存文脈](card-context-and-llm-wiki.md)を区別できる。

## 状態、停止、送信待ち

正本の状態は Pending / Starting / Running / AwaitingInput / AwaitingApproval / Cancelling と、終端の Succeeded / Failed / Cancelled / Crashed / AuditFailed。入力・承認待ちは終了扱いではない。ProjectionStatus はこの実行状態とは別に、正本の表示状態を安全に再構成できているかを表す。[状態契約](../../crates/executors/src/runtime/contracts.rs#L636-L691)

Cancel は終端 run には無操作で、活動中なら Cancelling を保存して実プロセス側を止める。InterruptTurn は provider の現在 turn への中断要求であり、終端 run には送れない。Steer は活動中の run に空でない追加指示を渡す。projection が degraded の間は Cancel 以外の制御を拒否する。[停止境界](../../crates/local-deployment/src/agent_run_port.rs#L2866-L2896)、[steer](../../crates/local-deployment/src/agent_run_port.rs#L2893-L2916)

通常の interactive Workspace の送信待ち queue は **Session ごとに一件、メモリ内**。再度 queue すると置換するため、永続の複数ジョブ列ではない。成功通知を受けると [Repository Memory](repository-memory.md) の source 完了処理を先に行い、それが成功してから follow-up を消費する。source 完了失敗なら queue を保持して起動せず、元 run の失敗・取消などでは queue を取り出して破棄する。[queue 保存](../../crates/services/src/services/queued_message.rs#L31-L69)、[終端処理](../../crates/local-deployment/src/container.rs#L397-L419)。サーバー再起動での queue 永続性を、[Workflow の outbox](../architecture/workflow-runtime.md) から類推しない。

実行専用 Workspace の終端通知では、上記の通常 source 完了・queue 消費を行わない。所有する統合・Wiki 保守が結果を確定し、一般の follow-up や retry を許可するかも [操作用途と実行所有者](workspace.md#操作用途と実行所有者)で検証する。[通常終端処理からの除外](../../crates/local-deployment/src/container.rs#L376-L396)

Cancel の応答は実プロセスの停止確認と監査の永続化を伴う。子プロセスの猶予・停止失敗の扱いは [Agent Runtime の取消](../architecture/agent-runtime.md#取消と実プロセスの終了)を参照する。

## 承認・質問と現在の接続状態

承認は provider の操作や計画を進める判断、質問は provider が求める回答である。UI は未解決イベントから `agentRunId` と `approvalId` または `inputId` を選び、`controlId` で個々の待機を識別する。承認・拒否と質問回答は別の制御であり、質問は質問名と回答の組を JSON にして送る。[待機の選択](../../packages/web-core/src/features/workspace-chat/model/hooks/useWorkspacePendingApproval.ts)、[制御への変換](../../packages/web-core/src/features/workspace-chat/model/canonicalAgentControls.ts)

PlanWithGoal の承認 UI は、編集した objective の保存に成功してから承認を送る。ただし、この画面の存在や AwaitingApproval/AwaitingInput 状態の定義だけでは、人の返答を待つ経路が有効とは限らない。[objective 更新と承認の順序](../../packages/web-core/src/features/workspace-chat/ui/SessionChatBoxContainer.tsx#L585-L598)

**この checkout の Codex 起動経路は `NoopExecutorApprovalService` を渡している。** このサービスは tool/plan 承認を即座に Approved とし、質問回答の待機は ServiceUnavailable にする。Codex client は質問待機のエラーを TimedOut として記録する。計画承認後は Plan なら実装 turn を始め、PlanWithGoal なら保存済み objective で Goal を始めるが、この配線を人の承認による停止点と説明してはいけない。[host の注入](../../crates/local-deployment/src/process_host.rs#L333-L360)、[Noop の実装](../../crates/executors/src/approvals.rs#L69-L103)、[質問と計画処理](../../crates/executors/src/executors/codex/client.rs#L1004-L1138)

さらに Codex の汎用 DirectControl は Approve/Input を「元の server request ID を使う必要がある」と Unsupported で拒否する。[制御の制約](../../crates/executors/src/executors/codex/client.rs#L428-L434)。廃止表示のない [chat-interface の計画レビュー手順](../../docs/workspaces/chat-interface.mdx) は承認・変更依頼という操作意図を記録するが、現在の host 配線との隔たりがある。ここはソースから確認した制限であり、対話実験の成功を意味しない。[Workflow の HumanGate](workflow-attempt.md) は別の停止・再開契約を持つ。

## 未送信ドラフトと Scratch

**Scratch** は編集状態であって、AgentRun の入力正本や実行待ち queue ではない。composer は本文・executor 設定を500 ms debounce で保存するが、入力時に tab-local な `sessionStorage` に pending backup も置く。サーバー保存の ACK は一致する版だけを消すため、debounce 前の hard reload や遅い保存応答でも新しい入力を失いにくい。初回 snapshot が届く前にユーザーが入力した内容も、遅い復元で上書きしない。[editor](../../packages/web-core/src/features/workspace-chat/model/hooks/useSessionMessageEditor.ts#L64-L179)、[pending backup](../../packages/web-core/src/features/workspace-chat/model/pendingSessionDraft.ts)

| 対象 | 保存キー・保存先 |
| --- | --- |
| 新規 Session の composer | Workspace ID |
| 既存 Session の composer | Session ID |
| 承認・質問の返答 | 待機固有の control ID。通常 follow-up と分離 |
| local runtime | SQLite Scratch と JSON Patch WebSocket |
| remote runtime | ブラウザー origin の localStorage。Cloud 同期ではない |
| 未 ACK の pending backup | runtime・host・scratch ID 別の sessionStorage。別タブ・別Sessionの原本ではない |

保存 mutation は対象 identity ごとに直列化する。画面切替時は編集時に捕捉した旧対象へ flush し、次の Session の callback へ送らない。新規 Session 作成の遅い応答や送信 ACK が別 host / Workspace / Session の現在入力を奪わないよう、scope と編集 revision も照合する。[mutation queue](../../packages/web-core/src/shared/lib/scratchMutationQueue.ts)、[送信 ACK](../../packages/web-core/src/features/workspace-chat/ui/SessionChatBoxContainer.tsx#L717-L762)

送信成功後は送信時の本文・設定 snapshot を条件として Scratch を削除する。新しい draft や既に削除済みの場合は安全な no-op。backend の follow-up 起動や queue 消費から無条件に削除しない。remote localStorage の条件付き削除も同じ考え方である。[DELETE 契約](../../crates/server/src/routes/scratch.rs#L102-L129)、[queue 終端](../../crates/local-deployment/src/container.rs#L407-L425)、[browser ACK](../../packages/web-core/src/shared/hooks/useLocalStorageScratch.ts#L148-L161)

queue 済み ID の local Scratch 作成・更新は引き続き拒否する。queue 取消時の本文・設定復元と、失敗・取消 run に伴う queue 破棄は異なる操作である。一件だけのメモリ内 queue を、この backup によって再起動後も実行できる耐久ジョブ列と説明してはいけない。[queue guard](../../crates/server/src/routes/scratch.rs#L45-L99)

保存エラーは editor の console に残り、未 ACK backup を保持する。ただしブラウザー storage が無効・容量不足なら backup 自体が書けない。この場合も入力欄を消さないが、再読み込み後の復元は保証できない。localStorage の保存失敗と sessionStorage の backup 失敗を、サーバーへの保存成功と取り違えない。[backup の限界](../../packages/web-core/src/features/workspace-chat/model/pendingSessionDraft.ts#L24-L54)。[ボード表示設定](project-and-issue.md#ボード表示と共有状態)は別の Scratch データである。

## Goal の寿命

ExecutionMode の Code / Plan / Goal / PlanWithGoal は、実行方法の指定であり PermissionPolicy とは別である。Goal は objective と状態、任意 token budget、使用 token・時間を持つ。Codex の objective は 4,000 文字上限を事前検証する。[mode の定義](../../crates/executors/src/profile.rs#L21-L37)、[Goal データ](../../crates/executors/src/runtime/contracts.rs#L199-L219)、[長さ検証](../../crates/executors/src/executors/codex.rs#L1-L23)

Codex Goal の管理開始は対応 RPC の成功応答で確定する。再生された通知や別 thread の完了から、この run が Goal を開始・達成したと判定しない。管理中は active、paused、blocked、usageLimited、budgetLimited でも host を維持し、同じ thread の complete/cleared で維持を解除する。[Goal lifecycle](../../crates/executors/src/executors/codex/goal_lifecycle.rs#L36-L104)、[順序・別 thread テスト](../../crates/executors/src/executors/codex/goal_lifecycle.rs#L111-L142)

Goal の subagent 上限は spawned agent の制約で、0 は無効化、未指定は Codex の既定値を使う。これは [Workflow](workflow-attempt.md) のノード並列数や、[Arena](arena.md) の候補数とは別の範囲である。[設定契約](../../crates/executors/src/profile.rs#L169-L175)、[Codex 設定テスト](../../crates/executors/src/executors/codex.rs#L1615-L1654)

## Native subagent と旧実行モデル

Provider が内部で起動する子 agent の活動は AgentActivity として記録される。子の完了を親の terminal output、status、Goal に適用しない。EVK が所有する [Workflow ノード](workflow-attempt.md) の別 AgentRun と区別する。[reducer と回帰テスト](../../crates/executors/src/runtime/reducer.rs#L278-L298)。表示と互換性の文書は [delegated-agent-display](../../docs/future/agent-runtime/delegated-agent-display.md) を参照し、将来の Audit 再投影構想を現在の自動修復と読まない。

旧 ExecutionProcess は script 実行などに残るが、coding-agent action は明示的に拒否される。現行 coding agent の活動判定・停止・履歴の正本は AgentRun を追う。[旧入口の拒否](../../crates/local-deployment/src/container.rs#L1534-L1540)、[活動判定のテスト](../../crates/server/src/routes/sessions/agent_run.rs#L873-L910)
