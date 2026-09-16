---
type: concept
title: Card context・Pipeline・LLM Wiki
description: カードに保存される初期指示と、作業ブランチ内の .llm-wiki の生成・参照・更新契約。
tags: [card-context, pipeline, llm-wiki, knowledge]
sources:
  - id: openwiki-source-6b2f204195135cd6da6a956f
    resource: repo://assets/pipelines/wikillm.toml
  - id: openwiki-source-4da31d5fd4f685e63432c1aa
    resource: repo://crates/executors/src/knowledge_skills.rs
  - id: openwiki-source-406c792263884bbf462d666c
    resource: repo://crates/server/src/routes/sessions/agent_run.rs
  - id: openwiki-source-5590f47e4cee001a35a42117
    resource: repo://crates/server/src/routes/workspaces/wiki.rs
  - id: openwiki-source-1c3e0ab9fadf12dd97d8fd4f
    resource: repo://crates/services/src/services/pipelines.rs
  - id: openwiki-source-ead147478bd848437cb020e7
    resource: repo://crates/services/src/services/wiki.rs
  - id: openwiki-source-3bef0d0bfd4066b26b945f6c
    resource: repo://crates/services/src/services/wiki/openwiki.rs
  - id: openwiki-source-1f99436ccc32fb0e2aed140f
    resource: repo://docs/superpowers/specs/2026-09-09-llm-wiki-design.md
  - id: openwiki-source-483f7cd15b3732e8f861ec5c
    resource: repo://packages/web-core/src/features/pipeline/model/cardContext.test.ts
  - id: openwiki-source-ddf0f7c06298de76526a4f7c
    resource: repo://packages/web-core/src/features/pipeline/model/cardContext.ts
  - id: openwiki-source-74b3285af184d8dc6de75ea4
    resource: repo://packages/web-core/src/features/pipeline/model/cardPipeline.ts
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
generated: { by: "codex", at: "2026-09-16T18:13:42.498Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-16T18:13:42.498Z
---

# Card context・Pipeline・LLM Wiki

Card context は、[Issue](project-and-issue.md) の依頼本文と一緒に保存する、エージェント向けの初期指示である。Pipeline はその中に埋め込む宣言的な手順であり、ロードしてもサーバー上のジョブは始まらない。[WorkflowAttempt](workflow-attempt.md) の実行グラフとは実行責任が異なる。[README](../../README.md)、[Pipeline ローダー](../../crates/services/src/services/pipelines.rs#L1-L3)

## 保存と再編集の契約

新規カードの既定値は LLM Wiki と Shared directories。保存済みの Card context または従来の Pipeline ブロックがあれば、その指示を保持する。本文編集でプリセットの最新定義を再生成しない。空の context ブロックも明示的な無効化として残る。共有ディレクトリの案内は [Workspace の共有資源](workspace.md) を説明する指示であり、案内を無効にする操作とディレクトリの削除は別である。[実装](../../packages/web-core/src/features/pipeline/model/cardContext.ts#L25-L98)、[保存・無効化テスト](../../packages/web-core/src/features/pipeline/model/cardContext.test.ts#L89-L132)

本文内の `evk:card-context`、`vk:pipeline` の開始・終了コメントが境界になる。Pipeline 編集は最後の完全な独立行ブロックを対象とし、周囲の本文と手書きの補足を残す。安定した pipeline/stage ID により、TOML の表示名や指示が変わったときも、生成行と手書き行を区別できる。引用中の marker や不完全なブロックを正常な context と同一視しない。[再構成](../../packages/web-core/src/features/pipeline/model/cardPipeline.ts#L19-L109)、[境界テスト](../../packages/web-core/src/features/pipeline/model/cardContext.test.ts#L135-L144)

Card context はカードから作る初回 Workspace リクエストへ渡される。既存 Session への遡及適用や、全チャット送信への自動追加という契約ではない。[文書化された挙動](../../README.md)、[初回リクエストのテスト](../../packages/web-core/src/features/pipeline/model/cardContext.test.ts#L65-L74)

## Pipeline と実行主体

TOML のファイル名が pipeline ID となり、stage は宣言順に並ぶ。ローダーは ID、stage ID の一意性、空の名前・指示、予約 marker の混入を検査する。同梱定義は未存在時だけ配置され、利用者のファイルを上書きしない。不正な定義は警告して読み飛ばす。[検証](../../crates/services/src/services/pipelines.rs#L70-L135)、[配置とロード](../../crates/services/src/services/pipelines.rs#L138-L188)

同梱 `wikillm` の二段階は次の判断をエージェントに求める。

- **Recall**: 作業に役立つ既存知識を探し、重要な内容は現在のソースで確かめる。Wiki がない、空、関連結果なし、という場合も正常に続行できる。
- **Enrich**: 検証した作業から再利用可能な知識を得た場合に、近い既存ページと index を更新する。宛先が曖昧、または記録すべき知識がなければ書かない。

これはエージェントへ渡す製品の指示契約であり、各段階の実施をサーバーが保証するものではない。[同梱定義](../../assets/pipelines/wikillm.toml#L1-L14)、[実装仕様の三層分離](../../docs/superpowers/specs/2026-09-09-llm-wiki-design.md#architecture)

[Codex アダプター](../integrations/agent-providers.md) は完全な LLM Wiki ブロックを検出すると Recall/Enrich の同梱 Skill を選択肢へ追加する。同名の選択済み Skill を重複追加せず、配置失敗時は警告して既存の選択を返す。この利用可能性と、Skill の実行完了を混同しない。[検出・追加](../../crates/executors/src/knowledge_skills.rs#L11-L65)

## .llm-wiki の境界と初期化

`.llm-wiki/` は各作業リポジトリ内にあり、コードと同じブランチのレビュー・統合に従うという設計である。[リポジトリ共通 OpenWiki](repository-memory.md) は専用 checkout と別の検証・公開機構を持つため、保存先も更新経路も取り違えない。[設計意図](../../docs/superpowers/specs/2026-09-09-llm-wiki-design.md#architecture)

通常の Session AgentRun 作成経路は、LLM Wiki ブロックを持つ依頼について実行前の準備を行う。全ての接続リポジトリの既存 Wiki を先に検査し、その後に未存在分を作る。リポジトリのない DirectFolder はそのルートを使用し、リポジトリのない Worktree は拒否する。不正な既存 Wiki を自動修復しない。[準備処理](../../crates/server/src/routes/sessions/agent_run.rs#L47-L114)、[複数リポジトリの失敗テスト](../../crates/server/src/routes/sessions/agent_run.rs#L726-L764)

必要な構成は `config.toml`、`index.md`、`pages/`。新規設定は version 1、既定言語 `en`。設定 API は既に有効な Wiki の言語だけを更新するので、初期化・修復の代替ではない。言語を変えても既存ページの自動翻訳を許可する設計ではない。[ロード](../../crates/services/src/services/wiki.rs#L208-L271)、[初期化と設定変更](../../crates/services/src/services/wiki.rs#L320-L357)、[翻訳の契約](../../assets/skills/knowledge-enrich/SKILL.md)

## 閲覧・変更時の注意点

Workspace Wiki API は選択された接続リポジトリの現在のファイルを読む。リポジトリなし DirectFolder だけは Workspace ID を仮の repo ID として使用する。ページの `sources` は既知のローカル Issue の `simple_id` と解決できれば Issue リンクになる。[解決境界](../../crates/server/src/routes/workspaces/wiki.rs#L83-L155)

legacy `.llm-wiki/` reader は symlink とルート外への解決を拒否し、Markdown は一枚 512 KiB、`pages/` 直下の最大 2,000 件を読む。階層を再帰走査する reader ではない。[制限値](../../crates/services/src/services/wiki.rs#L13-L17)、[安全な読み込み](../../crates/services/src/services/wiki.rs#L160-L205)、[legacy のページ走査](../../crates/services/src/services/wiki.rs#L240-L265)

一方、同じ Viewer API でも `openwiki=true` は別の読み取り専用 adapter を選ぶ。こちらは `openwiki/` の階層 Markdown と OKF frontmatter を読み、`.claims` 等の隠し管理ファイルと `INSTRUCTIONS.md` はページにしない。legacy 設定を作ったり OpenWiki を実行したりせず、上限は一枚 512 KiB、索引を含む 2,000 ページ、合計 16 MiB で、超過は部分表示にせずエラーにする。[reader の選択](../../crates/server/src/routes/workspaces/wiki.rs#L158-L184)、[OpenWiki adapter](../../crates/services/src/services/wiki/openwiki.rs#L1-L71)

形式を切り替えても表示元は現在の Workspace のファイルであり、base branch の公開済み Wiki へ自動で切り替わるわけではない。本文をメイン領域、ページ一覧を右側に配置する共通 Viewer の操作は [Workspace の検査](../operations/workspace-inspection.md)を参照する。生成・更新権限は表示機能から独立しており、OpenWiki の正本更新は [maintenance](../operations/openwiki-maintenance.md)が所有する。

変更時は marker の保持、明示的な無効化、複数リポジトリの先行検証、設定変更が修復にならないことを確認する。製品の経緯と完全なメタデータ仕様は [LLM Wiki 実装仕様](../../docs/superpowers/specs/2026-09-09-llm-wiki-design.md) が一次資料である。
