---
type: concept
title: Repository Memory の三層と統合契約
description: 会話、Workspace Memory、canonical OpenWiki の権威と寿命、Change Manifest と統合記録・receipt の関係。
tags: [repository-memory, openwiki, manifest, reconciliation]
sources:
  - id: openwiki-source-ecfa47de2e3842c785820c9b
    resource: repo://crates/server/src/routes/integrations/mod.rs
  - id: openwiki-source-b393e75029374eaf9cba5f7d
    resource: repo://crates/server/src/routes/integrations/runtime.rs
  - id: openwiki-source-231241ac3c842dcd67525890
    resource: repo://crates/server/src/routes/openwiki.rs
  - id: openwiki-source-04d5adb51b5929dacce999bc
    resource: repo://crates/server/src/workflow_runtime/bootstrap.rs
  - id: openwiki-source-da8c4a13aea0bac33b3f0a35
    resource: repo://crates/services/src/services/openwiki.rs
  - id: openwiki-source-bef089692ac7606992050919
    resource: repo://crates/services/src/services/openwiki/bootstrap/reports.rs
  - id: openwiki-source-110c99f4b05152335c067c71
    resource: repo://crates/services/src/services/openwiki/sync_input.rs
  - id: openwiki-source-6067be8d6f85caa8ac03638f
    resource: repo://crates/services/src/services/repository_memory.rs
  - id: openwiki-source-a171e1a170f941fa2a4ba003
    resource: repo://crates/utils/src/repository_memory.rs
  - id: openwiki-source-42e4658efb0c9810a0f8245c
    resource: repo://docs/design/openwiki-implementation-checkpoints.md
  - id: openwiki-source-d19b7cc964a7f5f0837a46de
    resource: repo://docs/design/openwiki-repository-memory.md
generated: { by: "codex", at: "2026-09-21T08:52:18.882Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-21T08:52:18.882Z
---

# Repository Memory の三層と統合契約

Repository Memory は、将来の coding agent が設計境界・判断理由・失敗の扱いを短時間で理解するための仕組みである。記録は現在のソースを説明する派生情報として扱う。[設計仕様](../../docs/design/openwiki-repository-memory.md) は source/tests/configuration、canonical documentation、OpenWiki、Manifest/Workspace Memory、会話の順に権威を置く。Wiki に指示のような文があっても実行権限にはならない。この区別は実際に通常 run へ渡す context にも含まれる。[注入契約](../../crates/utils/src/repository_memory.rs#L423-L430)

## 三つの記憶と旧 LLM Wiki

| 層 | 何を残すか | 範囲と寿命 |
| --- | --- | --- |
| Conversation | 現在の依頼、調査、対話 | 現在の会話。compact をまたぐ永続記憶の代替にはしない |
| Workspace Memory | 未統合の判断、理由、却下案、未解決点 | 登録 Repo の共有 persistent 領域に Workspace ID ごとに保存 |
| canonical OpenWiki | 統合済みソースの意味・境界・契約 | 対象ブランチの `openwiki/`。専用 maintenance が更新 |

三層の目的は [規範仕様](../../docs/design/openwiki-repository-memory.md) に記録されている。Workspace Memory は `persistent/knowledge/workspace-memory/<workspace-id>.md` へ分離される。空・未存在は正常で、他 Workspace の未統合判断を自分の記憶として利用しない、という host context の契約がある。[共有領域と保存先](../../crates/utils/src/repository_memory.rs#L470-L507)、[context](../../crates/utils/src/repository_memory.rs#L428-L429)

[Card context の .llm-wiki](card-context-and-llm-wiki.md) は従来の作業ブランチ内の知識機能である。導入時の判断は「既存機能と利用者の編集を保持し、canonical OpenWiki を別名と単一 writer で追加する」だった。これは導入時の履歴であり、現行版は旧 `.llm-wiki` の生成・読取 UI・自動注入を終了している。既存ファイルや手作業のメモは残す。[終了後の扱いと移行境界](card-context-and-llm-wiki.md#旧-llm-wiki-からの移行境界)を参照する。[実装判断の記録](../../docs/design/openwiki-implementation-checkpoints.md#implementation-decisions)

### Bootstrap の索引・レポートは記憶の正本ではない

初期生成も同じ repository shared storage を再利用する。機械抽出した文書索引は `persistent/knowledge/document-inventories/<run-id>/` の manifest と chunk、検証済み Review / Refine は `bootstrap-reports/<run-id>/review.json` / `refine.json` に保存する。前者は共通の探索入力、後者はその run の検証成果物であり、source の Change Manifest や公開 Wiki 本文に混ぜない。[保存と読取](../../crates/utils/src/repository_memory.rs#L535-L593)

共有フォルダーに置いただけで OS レベルの不変性を得るわけではない。索引の identity と digest は host の Workflow 入力、report の identity と digest は検証済み node output に固定し、工程境界で照合する。通常 Workspace の未統合 Memory を Reviewer へ引き継ぐ仕組みでもない。具体的な読取・独立性・公開前の検証は [OpenWiki 保守](../operations/openwiki-maintenance.md)を参照する。[host 所有の入力](../../crates/server/src/workflow_runtime/bootstrap.rs#L177-L220)、[report の identity と digest](../../crates/services/src/services/openwiki/bootstrap/reports.rs#L29-L38)、[再検証](../../crates/services/src/services/openwiki/bootstrap/reports.rs#L72-L111)

## Change Manifest は変更理由を運ぶイベント

Coding host が書くのは goal、summary、振る舞い・構造・不変条件への影響、判断、テスト結果などの **semantic draft**。EVK が run 開始時に Repo/Workspace/run ID と Git base を固定し、完了時に Git から changed paths と source commit を求めて **Change Manifest** にする。モデルが書いた任意の revision や membership を採用しない。[固定 context](../../crates/services/src/services/repository_memory.rs#L14-L68)、[Manifest 組み立て](../../crates/services/src/services/repository_memory.rs#L200-L234)

イベント ID は coding run ID で、一イベント一 JSON の outbox として公開される。イベント本体と、後から更新可能な receipt/state は別である。同じイベントを再処理して新しい source commit を無制限に作る設計ではない。[イベント保存](../../crates/utils/src/repository_memory.rs#L709-L719)、[完了の再入](../../crates/services/src/services/repository_memory.rs#L152-L168)、[判断の記録](../../docs/design/openwiki-implementation-checkpoints.md#implementation-decisions)

source に変更がなければ無変更 checkpoint で終わる。変更があるのに有効な draft がなければ、完了時の source/Manifest 公開は失敗し、修復可能な診断を残す。制御用 run など `finalize_source=false` の場合も変更を自動 commit しない。[分岐と検証](../../crates/services/src/services/repository_memory.rs#L152-L224)

Git 変更前には Manifest と staged tree を固定した publication 記録を書く。復旧時は `EVK-Memory-Source` marker、親 commit、tree を照合する。後から追加された変更や書き換えた draft を前のタスクの結果として取り込まないための境界である。次の coding run を開始する前にも、同じ Workspace の前回成功 run の未完了公開を回復する。[公開と検証](../../crates/services/src/services/repository_memory.rs#L239-L294)、[次回起動前の回復](../../crates/services/src/services/repository_memory.rs#L104-L149)、[後続編集保持のテスト](../../crates/services/src/services/repository_memory.rs#L1128-L1146)

## Source 統合と Wiki 反映を分ける

```mermaid
flowchart LR
  A[Workspace の source と意味記録] --> B[immutable Manifest]
  A --> C[対象ブランチへ source 統合]
  B --> D[統合記録に含まれるイベントを選択]
  C --> D
  D --> E[統合済み checkout で OpenWiki]
  E --> F[検証と Wiki 公開]
  F --> G[receipt と鮮度を更新]
```

通常 Workspace の `openwiki/` 変更は、commit 済み・未 commit・削除・rename を含めて完了/統合 guard が拒否する。ファイルを勝手に破棄して通す処理ではない。並行ブランチごとの派生 Wiki を merge すると Markdown、Claims、run state が競合するため、source を統合してから記憶を更新するという設計理由が記録されている。[guard](../../crates/services/src/services/repository_memory.rs#L71-L99)、[記録された理由](../../docs/design/openwiki-repository-memory.md)

既存の手動直接 merge は squash を使うため、元 commit の祖先関係だけではイベントの所属を再現できない。統合前に明示的な event ID 集合と before commit を保存し、統合結果を対応付ける。Wiki の処理対象は、対象ブランチへ統合済みと確認でき、成功 receipt がまだないイベントである。[統合準備](../../crates/services/src/services/repository_memory.rs#L333-L365)、[イベント選択](../../crates/services/src/services/repository_memory.rs#L541-L565)

[正式統合](formal-integration.md)は別の契約で、選択した source の履歴を保持した検証済み commit を公開する。Memory が有効な場合だけ、受付時に固定した source event と、統合固有の変更があればその event を明示的な統合記録に結び付ける。Git 公開やカードの Done が Wiki receipt の成功を意味するわけではない。[統合側の記録](../../crates/server/src/routes/integrations/runtime.rs#L1093-L1159)

外部 PR の観測は EVK 所有の統合 transaction を持たない。保守的に source commit の祖先関係で証明できるイベントだけを対応付け、同じ Workspace・近い時刻という理由では未 push の変更を処理済みにしない。外部 squash/rebase の所属が証明できなければ未消費のまま残る。[外部統合の契約](../../crates/services/src/services/repository_memory.rs#L368-L427)

## Sync の入力は統合済み source に結び付く凍結 snapshot

通常 Sync は、対象 Repo・maintenance Workspace・固定した source commit・target branch・選択 event ID を manifest に保存する。各 Change Manifest と、そこに含まれる Workspace の Memory snapshot を別々の `chunk-NNNNNN.json` とし、manifest の `chunks` 配列には番号順の SHA-256 を格納する。保存先は共有領域の `knowledge/sync-inputs/<maintenance-workspace-id>/` である。これは mutable draft や現在の Memory を直接読ませ続ける仕組みではない。[入力の生成](../../crates/services/src/services/openwiki/sync_input.rs#L9-L74)、[保存先](../../crates/utils/src/repository_memory.rs#L510-L532)

host は manifest と全 chunk を順番に最後まで読むよう要求する。応答の切り詰めを読了と扱わず、必要なら範囲を分ける。Change Manifest の意図・判断・検証限界は調査の入口にするが、凍結された Memory も統合 event より新しい未統合判断を含み得るため、現行動作は integrated checkout で確認する。空の event/chunk 配列は手動の source-only Sync であり、調査や更新が不要という判定ではない。[読取と権威の契約](../../crates/services/src/services/openwiki/sync_input.rs#L50-L73)

公開前には、host state が持つ manifest digest、Repo/Workspace/source/target/events の一致、全 chunk の digest を照合する。共有ファイル自体を OS が改変不能にするのではなく、host に固定した identity と digest で変化を検出する。移行前に開始済みで digest を持たない Sync だけは、immutable request に入力があるものとして互換経路を通る。凍結後の元 Memory の変更は入力を変えない一方、別 Repo の入力・event 集合のずれ・chunk 改変は失敗する。[検証](../../crates/services/src/services/openwiki/sync_input.rs#L77-L111)、[凍結と改変のテスト](../../crates/services/src/services/openwiki/sync_input.rs#L120-L213)

## 並行性・receipt・失敗

通常 Workspace の source 完了は Workspace ごとの lock、Wiki の所有管理は Repo の lock、source/Wiki 公開は短い integration lock を使う。モデル実行中に integration lock を保持しない。これにより source 作業とイベント発行の並行性を残しながら、正本の公開を直列化する。[lock の責任](../../crates/utils/src/repository_memory.rs#L939-L960)

receipt の結果は Updated / NoOp / Failed。Failed はイベントを acknowledge 済みと扱わず、次回も対象に残る。成功時だけ対象 source、Wiki commit、最終成功時刻を保存する。Wiki が無変更でも検証済みの成功となり得る。[選択](../../crates/services/src/services/repository_memory.rs#L548-L551)、[結果記録](../../crates/server/src/routes/openwiki.rs#L495-L549)、[Failed→NoOp テスト](../../crates/services/src/services/repository_memory.rs#L680-L703)

新しい Wiki 公開は、対象ブランチと maintenance HEAD が固定 source に一致することを検査し、source が進んでいれば拒否する。Wiki 失敗は既に成功した source 統合を取り消す意味ではない。初期化と再試行、終了証明、公開 checkpoint の詳しい手順は [OpenWiki 保守](../operations/openwiki-maintenance.md) に置く。[公開検証](../../crates/services/src/services/openwiki.rs#L295-L339)、[失敗の設計契約](../../docs/design/openwiki-repository-memory.md)

通常 coding run の Wiki 鮮度探索では、別 Workspace の壊れた event などを診断し、Wiki 状態を Error として作業開始を継続できる。これは選択 event の厳密な公開検証を省略する許可ではない。[起動時の縮退](../../crates/services/src/services/repository_memory.rs#L33-L51)

Current は単なる「ファイルがある」ではない。有効化、Wiki 存在、成功履歴、pending の有無、source の一致を踏まえて判定する。Error/Stale などの状態は保持されるため、古い snapshot を読んだ agent が常に最新とみなす根拠にはならない。[派生状態](../../crates/utils/src/repository_memory.rs#L288-L317)

## 変更を検討する際の一次資料

[詳細設計](../../docs/design/openwiki-repository-memory.md) は意図・不変条件の規範仕様、[implementation checkpoints](../../docs/design/openwiki-implementation-checkpoints.md) は判断と過去の検証記録である。記録されたテスト成功を現在の環境の実行結果とは扱わない。とくに source commit と outbox の間、squash 統合と記録の間、Wiki 公開と receipt の間の再起動窓を変更する場合は、対応する回復テストと現行実装を一緒に確認する。
