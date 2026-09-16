---
type: architecture
title: Agent Runtime の監査・投影・回復
description: AgentRun を独立プロセスで実行し、Native Audit から永続状態と画面表示へ変換する境界。再接続、DB 競合、投影劣化時の契約を説明する。
tags: [agent-runtime, audit, persistence, recovery]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-15T18:24:07.449Z
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
  - id: openwiki-source-7151ac14cb4570c5c04781dd
    resource: repo://crates/services/src/services/agent_runtime.rs
  - id: openwiki-source-7d788a7c8b1c5bf5122bab53
    resource: repo://crates/utils/src/assets.rs
  - id: openwiki-source-9b65aee0d6accda7142f90a7
    resource: repo://crates/utils/src/native_audit.rs
  - id: openwiki-source-965b490de6f8364cb7695944
    resource: repo://crates/workspace-manager/src/workspace_manager.rs
  - id: openwiki-source-11be03870aae85a259cab5c1
    resource: repo://docs/future/agent-runtime/architecture.md
  - id: openwiki-source-bb8ec0d0f300d3d2dd8edfc6
    resource: repo://docs/future/agent-runtime/data-planes-and-recovery.md
  - id: openwiki-source-37db6fa2c961da1810d195e7
    resource: repo://docs/future/agent-runtime/README.md
generated: { by: "codex", at: "2026-09-15T18:24:07.449Z" }
---

# Agent Runtime の監査・投影・回復

Agent Runtime は、[Session 内の AgentRun](../concepts/session-and-agent-run.md) を実プロセスとして動かし、その観測結果を製品が判断できる状態へ変換する。provider 固有の通信は adapter が担当し、Workflow は [AgentRunPort を通して実行を依頼](workflow-runtime.md)する。

## 起動と所有者

`LocalAgentRunPort::reserve` は request と attempt の整合性、Workspace を検証し、実行 ID を DB に保存する。Setup の完了待ちなどでは、予約と起動を分けられる。`launch_reserved` は既に Pending でなくなった実行を重複起動せず、現在の状態を返す。[起動境界](../../crates/local-deployment/src/agent_run_port.rs#L290-L318)

`agent-process-host` は一つの provider process と Native Audit を所有する独立ホストである。アプリ側は認証付き loopback endpoint に再接続し、保存済み host-event cursor の後から観測を取り込む。このため、サーバーとの接続断をそのまま provider の終了と扱ってはいけない。[ホストの契約](../../crates/local-deployment/src/process_host.rs#L1-L5)

## 四つのデータ経路

| 経路 | 保存先と役割 | 消失・再接続時の扱い |
| --- | --- | --- |
| Native Audit | 生の入出力フレームと manifest | adapter の再解釈や完了証拠の原資料 |
| Live | メモリ上の通知と WebSocket の `live` | 表示中の文字列差分。永続履歴を保証しない |
| Canonical | SQLite の `agent_events` と `agent_run_state` | 完了メッセージ、状態遷移、制御など、製品判断用の永続記録 |
| Snapshot | `agent_run_usage_snapshots` など | 累積使用量等の最新値。通知回数分を加算しない |

この分離の記録された目的は、表示用 token delta によって SQLite を二つ目の完全監査ログにしないことである。[データ経路の設計文書](../../docs/future/agent-runtime/data-planes-and-recovery.md)は現行の batch・snapshot 実装と対応する。`future/` というディレクトリ名だけで未実装文書と判断しない。

stdout は Native Audit へ記録してから分類・投影する。Audit の書き込み失敗、解釈が必須のフレームの失敗は通知して読み取りを打ち切る。既知の audit-only 通知と任意の未知通知には canonical event を作らない。[監査優先の処理](../../crates/local-deployment/src/process_host.rs#L630-L665)、[任意通知の扱い](../../crates/executors/src/executors/provider_adapter.rs#L1684-L1693)

Codex のメッセージ差分と完成メッセージは別の経路を通る。画面は完成した永続メッセージで一時表示を収束させる。再接続を「全 delta の再送」として実装しない。[Web 側の同期](web-and-sync.md)も参照。

## 制御入力の送信と監査の順序

初回 launch は canonical input と launch payload を Audit に保存してから provider を起動する。一方、活動中の制御は `control_peer.send`、または `stdin.write_all` / `flush` が先で、返された非空 bytes を後から `append_native_input` へ保存する。後段の保存に失敗すると要求へエラーを返し、`AuditFailure` による終了・cleanup へ進む。**AuditFailed や該当フレームの不在は「未送信」「副作用なし」の証明にならない。** [初回起動](../../crates/local-deployment/src/process_host.rs#L302-L329)・[制御と失敗](../../crates/local-deployment/src/process_host.rs#L797-L887)

耐久 command service は command を DB に enqueue してから配送するが、その記録と provider へ送った生 bytes の監査は別である。再試行時は command の状態に加えて実行状態・作業木も確認する。[command の事前保存](../../crates/services/src/services/agent_runtime.rs#L106-L145)

[Runtime README](../../docs/future/agent-runtime/README.md) は「計画承認済み・実施開始待ち」、[目標設計](../../docs/future/agent-runtime/architecture.md) は Draft と明記する。そこに記録された「native input を保存してから送信する」方針は、この制御経路の現在の順序とは一致しない。設計自身も既発生の作業木変更を rollback する保証はしない。これは静的な配線確認であり、監査ディスク障害の再現試験結果ではない。[記録された順序・失敗方針](../../docs/future/agent-runtime/architecture.md#写入顺序与失败策略)

## Native Audit の保全・エクスポート・寿命

Audit の本体は SQLite の外、`<asset_dir>/runtime/native-audit/v1/sessions/<Session UUID先頭2文字>/<Session UUID>/agent-runs/<AgentRun UUID>/attempts/<RunAttempt UUID>/` にある `manifest.json` と `frames.jsonl` である。debug の asset_dir は repository の `dev_assets`、release は OS のアプリデータ領域。SQLite だけの保全では原文を保存できない。[パス構築](../../crates/utils/src/native_audit.rs#L7-L31)・[asset root](../../crates/utils/src/assets.rs#L6-L29)

フレームは payload の無損失 base64 と checksum を保持する。プログラム上の `AuditBundle::export` は bundle を検証してから manifest / frames と、存在すれば fixture の期待イベントをそのままコピーする。脱敏された表示ログではなく、送受信した本文を含む原資料なので、共有する bundle の内容もその単位で確認する。[payload](../../crates/executors/src/runtime/native_audit.rs#L75-L113)・[読取検証](../../crates/executors/src/runtime/native_audit.rs#L749-L810)・[export](../../crates/executors/src/runtime/native_audit.rs#L822-L843)

読取は Complete / Recovered の manifest、連番、frame 数、checksum を検査し、open・不完全な bundle を拒否する。replay は audit schema / runtime / adapter / protocol / mapper の version を照合し、不一致をエラーにする。既存の bundle replay は、下記の将来作業である「製品 DB の完全再投影」と同じ操作ではない。[version 契約](../../crates/executors/src/runtime/native_audit.rs#L845-L864)・[比較](../../crates/executors/src/runtime/native_audit.rs#L1003-L1042)・[不一致テスト](../../crates/executors/src/runtime/native_audit.rs#L1161-L1189)

[Runtime 計画の「周期 cleanup を追加しない」という保存方針](../../docs/future/agent-runtime/README.md#已确认的产品边界)と、利用者の明示削除は分ける。[Workspace 削除](../concepts/workspace.md#archive削除期限-cleanup)では Session ID 群を取得し、DB 削除後の background cleanup が各 Session の Native Audit と process log を削除する。External の作業ディレクトリ保護も、この別保存先の永久保持を意味しない。必要な監査は削除前に保全する。cleanup 失敗はログに残るため、削除 API の受付だけで消去完了とも判定しない。[削除 context](../../crates/workspace-manager/src/workspace_manager.rs#L174-L205)・[cleanup](../../crates/workspace-manager/src/workspace_manager.rs#L436-L515)・[削除と再実行のテスト](../../crates/workspace-manager/src/workspace_manager.rs#L1463-L1492)

## DB の原子性と冪等性

一つの host response の永続イベント、provider session 観測、投影、usage snapshot、cursor 更新を同じ transaction に収める。canonical sequence は密な連番を割り当て、生フレームや host の sequence とは区別する。cursor だけ先へ進めると再接続時に未反映データを失う。[batch の入口](../../crates/db/src/models/agent_runtime.rs#L1452-L1527)、[cursor と commit](../../crates/db/src/models/agent_runtime.rs#L1745-L1764)

usage は RunAttempt ごとに native sequence が新しい場合だけ置き換える。同じ sequence に異なる内容が来ると冪等性の衝突として拒否し、古い通知は無視する。[usage 更新](../../crates/db/src/models/agent_runtime.rs#L1696-L1728)

## 障害を区別する

| 障害 | 処理 |
| --- | --- |
| 一時的な SQLite 競合 | 有限回 retry 後に Unavailable。恒久的な投影劣化へ変換しない |
| 決定的な契約・reducer エラー | batch を観測単位に分け、失敗観測を `agent_projection_failures` に記録 |
| DB 基盤の障害 | その失敗を隔離済みとして cursor を進めない。次の attach で再取得する |

隔離時は理由、`projection_degraded`、当該観測の cursor 前進を原子的に保存し、後続観測の取り込みを継続できる。[分離処理](../../crates/local-deployment/src/agent_run_port.rs#L1540-L1622)、[失敗台帳](../../crates/db/src/models/agent_runtime.rs#L218-L294)

投影が Current でない場合、backend は Cancel 以外の制御を拒否する。Cancel は終端実行には成功する無操作であり、それ以外では Cancelling へ遷移して停止を依頼する。表示が不完全でも停止経路を残す契約である。[制御ガード](../../crates/local-deployment/src/agent_run_port.rs#L2370-L2395)

## 変更時の検証と未解決点

`host_batch_commits_events_projection_and_cursor_atomically` は密な sequence、重複受信、内容衝突時の event・cursor・provider session の rollback を検証する。[テスト](../../crates/db/src/models/agent_runtime.rs#L2278-L2395)を、mapper や永続化単位の変更時に確認する。

完全な canonical projection を Native Audit から再構築する管理操作は、[回復文書](../../docs/future/agent-runtime/data-planes-and-recovery.md#compatibility-and-future-work)では将来作業とされる。失敗台帳が存在することを自動修復の保証と解釈しない。古い mapper と履歴の互換性を含む設計意図は [Runtime 目標設計](../../docs/future/agent-runtime/architecture.md)に記録されており、個別機能の実装有無は現行 adapter と照合する。
