---
type: architecture
title: Agent Runtime の監査・投影・回復
description: AgentRun を独立プロセスで実行し、Native Audit から永続状態と画面表示へ変換する境界。再接続、DB 競合、投影劣化時の契約を説明する。
tags: [agent-runtime, audit, persistence, recovery]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-22T01:26:32.834Z
sources:
  - id: openwiki-source-d20a82e2192a07c687b838cb
    resource: repo://crates/db/src/models/agent_runtime.rs
  - id: openwiki-source-f4e0f9b2ce513d3f8b8b070e
    resource: repo://crates/executors/src/executors/provider_adapter.rs
  - id: openwiki-source-2312e3967b9fd7f73e349d9a
    resource: repo://crates/executors/src/runtime/native_audit.rs
  - id: openwiki-source-788fa2f6f1a3d449a1a9efe4
    resource: repo://crates/local-deployment/src/agent_run_port.rs
  - id: openwiki-source-8fd7f4fe2d73e778cc63c256
    resource: repo://crates/local-deployment/src/process_host.rs
  - id: openwiki-source-5d78398344c565f028fc5984
    resource: repo://crates/local-deployment/src/process_host/journal.rs
  - id: openwiki-source-7151ac14cb4570c5c04781dd
    resource: repo://crates/services/src/services/agent_runtime.rs
  - id: openwiki-source-7d788a7c8b1c5bf5122bab53
    resource: repo://crates/utils/src/assets.rs
  - id: openwiki-source-9b65aee0d6accda7142f90a7
    resource: repo://crates/utils/src/native_audit.rs
  - id: openwiki-source-991414672d835fda0eead290
    resource: repo://crates/utils/src/process.rs
  - id: openwiki-source-965b490de6f8364cb7695944
    resource: repo://crates/workspace-manager/src/workspace_manager.rs
  - id: openwiki-source-11be03870aae85a259cab5c1
    resource: repo://docs/future/agent-runtime/architecture.md
  - id: openwiki-source-bb8ec0d0f300d3d2dd8edfc6
    resource: repo://docs/future/agent-runtime/data-planes-and-recovery.md
  - id: openwiki-source-37db6fa2c961da1810d195e7
    resource: repo://docs/future/agent-runtime/README.md
generated: { by: "codex", at: "2026-09-22T01:26:32.834Z" }
---

# Agent Runtime の監査・投影・回復

Agent Runtime は、[Session 内の AgentRun](../concepts/session-and-agent-run.md) を実プロセスとして動かし、その観測結果を製品が判断できる状態へ変換する。provider 固有の通信は adapter が担当し、Workflow は [AgentRunPort を通して実行を依頼](workflow-runtime.md)する。

## 起動と所有者

`LocalAgentRunPort::reserve` は request と attempt の整合性、Workspace を検証し、実行 ID を DB に保存する。Setup の完了待ちなどでは、予約と起動を分けられる。`launch_reserved` は既に Pending でなくなった実行を重複起動せず、現在の状態を返す。[起動境界](../../crates/local-deployment/src/agent_run_port.rs#L300-L328)

`agent-process-host` は一つの provider process と Native Audit を所有する独立ホストである。アプリ側は認証付き loopback endpoint に再接続し、保存済み host-event cursor の後から観測を取り込む。このため、サーバーとの接続断をそのまま provider の終了と扱ってはいけない。[ホストの契約](../../crates/local-deployment/src/process_host.rs#L1-L5)

## 四つのデータ経路

| 経路 | 保存先と役割 | 消失・再接続時の扱い |
| --- | --- | --- |
| Native Audit | 生の入出力フレームと manifest | adapter の再解釈や完了証拠の原資料 |
| Live | メモリ上の通知と WebSocket の `live` | 表示中の文字列差分。永続履歴を保証しない |
| Canonical | SQLite の `agent_events` と `agent_run_state` | 完了メッセージ、状態遷移、制御など、製品判断用の永続記録 |
| Snapshot | `agent_run_usage_snapshots` など | 累積使用量等の最新値。通知回数分を加算しない |

この分離の記録された目的は、表示用 token delta によって SQLite を二つ目の完全監査ログにしないことである。[データ経路の設計文書](../../docs/future/agent-runtime/data-planes-and-recovery.md)は現行の batch・snapshot 実装と対応する。`future/` というディレクトリ名だけで未実装文書と判断しない。

stdout は Native Audit へ記録してから分類・投影する。Audit の書き込み失敗、解釈が必須のフレームの失敗は通知して読み取りを打ち切る。既知の audit-only 通知と任意の未知通知には canonical event を作らない。[監査優先の処理](../../crates/local-deployment/src/process_host.rs#L741-L778)、[任意通知の扱い](../../crates/executors/src/executors/provider_adapter.rs#L2041-L2050)

Codex のメッセージ差分と完成メッセージは別の経路を通る。画面は完成した永続メッセージで一時表示を収束させる。再接続を「全 delta の再送」として実装しない。[Web 側の同期](web-and-sync.md)も参照。

## Host の再送記録と有界配信

Host は SQLite と別に `runtime/host-events/<attempt>-<host>.v1.jsonl` を持つ。これは再接続用の意味的 batch journal であって、raw delta の第二監査ではない。文字列 Live delta は記録から外し、usage snapshot は残す。128件・256 KiB・50 ms を契機に grouped flush し、Started / Terminal は直ちに同期保存する。未保存の cursor を consumer へ公開しない。[journal の保存](../../crates/local-deployment/src/process_host/journal.rs#L156-L238)、[周期 flush](../../crates/local-deployment/src/process_host.rs#L566-L584)

再送ページは128件／4 MiBを目安に制限し、直近 Live は256件／4 MiBの ring に置く。producer 側も件数・bytes による backpressure を持つ。単一の過大な意味的 event は黙って切り詰めず失敗させ、原文は Native Audit に残す。一方、journal の batch offset 索引は実行時間とともに増えるため、全メモリが定数という保証ではない。[上限](../../crates/local-deployment/src/process_host/journal.rs#L17-L23)、[producer の byte budget](../../crates/local-deployment/src/process_host.rs#L791-L815)

protocol v2 の Subscribe は cursor から追いつき、通知と heartbeat を契機に journal を読み直す。制御用接続を長い購読と分け、遅い購読先への書込も有限時間に制限する。旧 Host の Attach は互換経路として残り、未知の protocol を推測して replay しない。[Subscribe](../../crates/local-deployment/src/process_host.rs#L691-L720)

## 接続断と確認済み終了を分ける

複数回の通信失敗だけでは死亡としない。Host の開始 identity を照合して終了を確認した後、journal の checksum・連番・run identity と、閉じた Native Audit に対する参照を検証してから再生する。provider / process group が生存中または不明なら終端成功を採用せず、観測・正規 Cancel の対象として残す。代替 provider を勝手に起動しない。[回復入口](../../crates/local-deployment/src/agent_run_port.rs#L1497-L1555)、[子と終端の確認](../../crates/local-deployment/src/agent_run_port.rs#L1580-L1653)

provider の「turn 完了」と Host の終了は別である。成功を最終化する証拠は、出力 drain と Audit close 後の Host Terminal。未完了 journal tail を除いた再生は完全回復とは呼ばず、終端証拠のない死亡を成功へ昇格させない。この順序は [OpenWiki publication](../operations/openwiki-maintenance.md) が閉じた監査を読むためにも必要である。[Audit と terminal](../../crates/local-deployment/src/process_host.rs#L1090-L1166)

## 制御入力の送信と監査の順序

初回 launch は canonical input と launch payload を Audit に保存してから provider を起動する。一方、活動中の制御は `control_peer.send`、または `stdin.write_all` / `flush` が先で、返された非空 bytes を後から `append_native_input` へ保存する。後段の保存に失敗すると要求へエラーを返し、`AuditFailure` による終了・cleanup へ進む。**AuditFailed や該当フレームの不在は「未送信」「副作用なし」の証明にならない。** [初回起動](../../crates/local-deployment/src/process_host.rs#L333-L360)・[制御と失敗](../../crates/local-deployment/src/process_host.rs#L941-L1011)

耐久 command service は command を DB に enqueue してから配送するが、その記録と provider へ送った生 bytes の監査は別である。再試行時は command の状態に加えて実行状態・作業木も確認する。[command の事前保存](../../crates/services/src/services/agent_runtime.rs#L106-L145)

[Runtime README](../../docs/future/agent-runtime/README.md) は「計画承認済み・実施開始待ち」、[目標設計](../../docs/future/agent-runtime/architecture.md) は Draft と明記する。そこに記録された「native input を保存してから送信する」方針は、この制御経路の現在の順序とは一致しない。設計自身も既発生の作業木変更を rollback する保証はしない。これは静的な配線確認であり、監査ディスク障害の再現試験結果ではない。[記録された順序・失敗方針](../../docs/future/agent-runtime/architecture.md#写入顺序与失败策略)

## Native Audit の保全・エクスポート・寿命

Audit の本体は SQLite の外、`<asset_dir>/runtime/native-audit/v1/sessions/<Session UUID先頭2文字>/<Session UUID>/agent-runs/<AgentRun UUID>/attempts/<RunAttempt UUID>/` にある `manifest.json` と `frames.jsonl` である。debug の asset_dir は repository の `dev_assets`、release は OS のアプリデータ領域。SQLite だけの保全では原文を保存できない。[パス構築](../../crates/utils/src/native_audit.rs#L7-L31)・[asset root](../../crates/utils/src/assets.rs#L6-L29)

フレームは payload の無損失 base64 と checksum を保持する。プログラム上の `AuditBundle::export` は bundle を検証してから manifest / frames と、存在すれば fixture の期待イベントをそのままコピーする。脱敏された表示ログではなく、送受信した本文を含む原資料なので、共有する bundle の内容もその単位で確認する。[payload](../../crates/executors/src/runtime/native_audit.rs#L175-L213)・[読取検証](../../crates/executors/src/runtime/native_audit.rs#L849-L910)・[export](../../crates/executors/src/runtime/native_audit.rs#L922-L943)

読取は Complete / Recovered の manifest、連番、frame 数、checksum を検査し、open・不完全な bundle を拒否する。replay は audit schema / runtime / adapter / protocol / mapper の version を照合し、不一致をエラーにする。既存の bundle replay は、下記の将来作業である「製品 DB の完全再投影」と同じ操作ではない。[version 契約](../../crates/executors/src/runtime/native_audit.rs#L945-L964)・[比較](../../crates/executors/src/runtime/native_audit.rs#L1103-L1142)・[不一致テスト](../../crates/executors/src/runtime/native_audit.rs#L1317-L1345)

[Runtime 計画の「周期 cleanup を追加しない」という保存方針](../../docs/future/agent-runtime/README.md#已确认的产品边界)と、利用者の明示削除は分ける。[Workspace 削除](../concepts/workspace.md#archive削除期限-cleanup)では Session ID 群を取得し、DB 削除後の background cleanup が各 Session の Native Audit と process log を削除する。External の作業ディレクトリ保護も、この別保存先の永久保持を意味しない。必要な監査は削除前に保全する。cleanup 失敗はログに残るため、削除 API の受付だけで消去完了とも判定しない。[削除 context](../../crates/workspace-manager/src/workspace_manager.rs#L174-L205)・[cleanup](../../crates/workspace-manager/src/workspace_manager.rs#L436-L515)・[削除と再実行のテスト](../../crates/workspace-manager/src/workspace_manager.rs#L1463-L1492)

## DB の原子性と冪等性

一つの host response の永続イベント、provider session 観測、投影、usage snapshot、cursor 更新を同じ transaction に収める。canonical sequence は密な連番を割り当て、生フレームや host の sequence とは区別する。cursor だけ先へ進めると再接続時に未反映データを失う。[batch の入口](../../crates/db/src/models/agent_runtime.rs#L1479-L1554)、[cursor と commit](../../crates/db/src/models/agent_runtime.rs#L1772-L1793)

usage は RunAttempt ごとに native sequence が新しい場合だけ置き換える。同じ sequence に異なる内容が来ると冪等性の衝突として拒否し、古い通知は無視する。[usage 更新](../../crates/db/src/models/agent_runtime.rs#L1723-L1755)

## 障害を区別する

| 障害 | 処理 |
| --- | --- |
| 一時的な SQLite 競合 | 有限回 retry 後に Unavailable。恒久的な投影劣化へ変換しない |
| 決定的な契約・reducer エラー | batch を観測単位に分け、失敗観測を `agent_projection_failures` に記録 |
| DB 基盤の障害 | その失敗を隔離済みとして cursor を進めない。次の attach で再取得する |

隔離時は理由、`projection_degraded`、当該観測の cursor 前進を原子的に保存し、後続観測の取り込みを継続できる。[分離処理](../../crates/local-deployment/src/agent_run_port.rs#L1969-L2051)、[失敗台帳](../../crates/db/src/models/agent_runtime.rs#L234-L310)

投影が Current でない場合、backend は Cancel 以外の制御を拒否する。Cancel は終端実行には成功する無操作であり、それ以外では Cancelling へ遷移して停止を依頼する。表示が不完全でも停止経路を残す契約である。[制御ガード](../../crates/local-deployment/src/agent_run_port.rs#L2866-L2889)。この前段にある [実行専用 Workspace の所有者検証](../concepts/workspace.md#操作用途と実行所有者)は別の操作制約である。

## 取消と実プロセスの終了

host は native interrupt の前に子プロセスを捕捉し、送信できた場合は最大2秒、provider 自身による tool cleanup を待ってから transport を閉じる。native interrupt が利用不能でも取消を中断せず、process group と捕捉した子の終了を並行して試みる。両方の cleanup が成功して初めて Cancelled とし、待機失敗は Crashed になる。取消応答も監査の終了と terminal 記録の後で返す。[取消経路](../../crates/local-deployment/src/process_host.rs#L941-L1042)、[完了応答](../../crates/local-deployment/src/process_host.rs#L1090-L1166)、[失敗状態](../../crates/local-deployment/src/process_host.rs#L1227-L1241)

Linux では別 session に移る tool も扱うため `/proc` から子孫を列挙し、親と開始時刻を再照合した pidfd を保持する。PID 再利用で無関係なプロセスを停止しないための仕組みであり、捕捉後の daemon 化まで閉じ込める sandbox ではない。Linux 以外ではこの子孫捕捉は空の処理となり、既存 process group cleanup に依存する。[捕捉の範囲](../../crates/utils/src/process.rs#L8-L83)、[終了確認](../../crates/utils/src/process.rs#L85-L136)。独立 session の子だけを停止する [focused test](../../crates/utils/src/process.rs#L220-L259)と、応答しない native interrupt でも取消が有限時間で進む [test](../../crates/local-deployment/src/process_host.rs#L1517-L1553)がある。

## 変更時の検証と未解決点

`host_batch_commits_events_projection_and_cursor_atomically` は密な sequence、重複受信、内容衝突時の event・cursor・provider session の rollback を検証する。[テスト](../../crates/db/src/models/agent_runtime.rs#L2392-L2512)を、mapper や永続化単位の変更時に確認する。

完全な canonical projection を Native Audit から再構築する管理操作は、[回復文書](../../docs/future/agent-runtime/data-planes-and-recovery.md#compatibility-and-future-work)では将来作業とされる。失敗台帳が存在することを自動修復の保証と解釈しない。古い mapper と履歴の互換性を含む設計意図は [Runtime 目標設計](../../docs/future/agent-runtime/architecture.md)に記録されており、個別機能の実装有無は現行 adapter と照合する。
