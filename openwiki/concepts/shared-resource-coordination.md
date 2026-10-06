---
type: concept
title: 共有リソースの所有権と AI 調停
description: 複製できない実機・DB・環境を並列 Card から扱うための排他制御、状態変更、調停と復旧の境界。
tags: [resources, coordination, concurrency, recovery, codex]
sources:
  - resource: repo://crates/db/src/models/resource_coordination.rs
  - resource: repo://crates/db/src/models/resource_coordination/tests.rs
  - resource: repo://crates/server/src/routes/resource_coordination/runtime.rs
  - resource: repo://crates/services/src/services/resource_coordination.rs
  - resource: repo://crates/executors/src/executors/codex.rs
---

# 共有リソースの所有権と AI 調停

LVK の並列化には三つの異なる整合性がある。worktree はソース編集を分離し、
[正式 Integration](formal-integration.md) は利用者が選んだ変更を Git 上で統合する。
共有リソース調停は、その間に行う実機・DB・デプロイ環境などへの操作を調整する。
意味の記録があっても、同じ実機への同時操作や互換性のない DB 変更は防げない。
このため、AI の意味判断と、プログラムによる所有権の管理を分ける。

## 調停 AI は所有権の根拠にならない

資源・操作・所有者・世代番号・状態リビジョン・イベントは SQLite に保存する。
空いていて前提が一致する操作は LVK が直接割り当てる。競合・前提変更・異常時だけ
既存の Codex 認証で調停を起動する。調停は専用の execution-only Workspace と
既存の AgentRun / Orchestration を使い、別の実行基盤や API キーを要求しない。

AI は公開された利用目的と資源の状態から順番や再検討の必要性を提案する。
私的な会話とコマンド本文は調停用 snapshot に含めない。host は snapshot が
変わっていないことと、判断が待機要求の範囲内であることを確認する。AI は稼働中の
所有権を奪えず、ロックの解除、コマンド変更、前提の免除、Card の自動統合は行えない。
古い判断は履歴に残して未適用とする。調停失敗によって通常の割り当ては止めない。

## Card より短い寿命で、必要資源を一括取得する

初版は排他的な資源（容量 1）を扱う。一つの操作は、必要な全資源と、その時点で
観測した状態リビジョンを宣言する。取得はトランザクションで一括実行し、待機中は
一つも占有しない。長時間の Goal 全体や、ADB の一命令ごとをロック単位にしない。
操作・後始末・独立した状態確認までが一つの critical section である。

host は保存済みのコマンドと所有権を照合し、既存の ExecutionProcess で実行する。
同じ request UUID の再送は再実行しない。正常終了だけでなく、検証コマンドの成功と
プロセスグループの後始末を確認して解放する。各 grant の fence と、状態の revision
は別物である。互換な利用を繰り返しても状態 revision は変わらない。

操作完了、Card 完了、Git 統合はそれぞれ別の出来事である。操作が終わっても Card
で追加開発できる。待機要求はモデルのターンや通信接続より長く保存されるが、
一時停止した Goal を勝手に再開しない。未解決の操作がある Workspace は cleanup と
正式 Integration の採用元への受付を拒否する。

## 解放されてもスキーマ変更は消えない

成功時に宣言した変更後の状態（例: schema-v2）を保存し、revision を増やしてから
所有権を解放する。古い revision で待っていた要求は実行せず、再検討待ちにする。
Card の AI は新しい schema/API/APK の実体と調停結果を確認して対応を判断する。
revision の数字だけを最新に置き換えることは互換性確認ではない。

## タイムアウト・切断・中止は解放の証拠ではない

待機中の中止は即時に終わる。実行中の中止は停止要求であり、外部処理の完了では
ない。コマンド失敗、検証失敗、host 再起動時の不明状態では所有権を保持して
`recovery_required` にする。TTL による自動解放や、不明な操作の自動再実行はしない。

復旧では旧プロセスと外部処理が止まった根拠、全資源の確認済み状態、現在 revision
を利用者が記録する。host は管理対象プロセスがまだ動ける場合には拒否する。
復旧により revision を増やして全資源を解放するが、その操作を成功扱いにはしない。
DB の履歴が実行時の正本であり、Wiki は live owner ledger として使わない。

## 適用範囲を広く解釈しない

- 同じ物理資源は一つの canonical key で登録する。異なるキーが同じ実体であることを
  自動発見する機能はない。
- 同じ DB を使う LVK が一つの調停主体である。安定版と開発版が別 DB を持つ場合は
  ロックを共有しない。実資源を扱う主体は一つにし、独立した開発版では模擬資源を使う。
- raw shell と外部認証情報を持つ agent の迂回を OS レベルで禁止する仕組みではない。
  調停 AI のツール制限と、汎用作業 agent の権限は同一ではない。
- fence は外部アダプターと対象資源が検証して初めて古い要求の実行を防げる。
  環境変数を渡すだけでは任意の ADB・SQL・AWS 呼び出しを fence できない。
- 実機接続、外部資源固有の復旧アダプター、共有読み取り・容量 N、複数 LVK 間の調停は
  初版の対象外。正式 Integration の検証コマンドも自動的に資源要求へ変換しない。

操作方法と API の入口は [実装仕様](../../docs/design/shared-resource-coordination.md) を参照する。
実装・テストが正本であり、このページは設計意図と危険な変更点の案内である。
