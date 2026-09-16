---
type: guide
title: クイックスタートと調査案内
description: easy-vibe-kanban の目的、主要概念の説明先、変更時の調査経路、既存文書の適用範囲を案内する。
tags: [overview, onboarding, concepts, navigation]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-16T18:13:42.498Z
sources:
  - id: openwiki-source-0d0b05d2fac028aecd3be162
    resource: repo://.github/workflows/publish-easy-npx.yml
  - id: openwiki-source-4e5231907394b801763cfa97
    resource: repo://crates/api-types/src/issue_tag.rs
  - id: openwiki-source-d6072e60b63fe041de50f89e
    resource: repo://crates/api-types/src/tag.rs
  - id: openwiki-source-0ba09128382dd913fe9a4c4a
    resource: repo://crates/db/src/models/tag.rs
  - id: openwiki-source-219b8c8774f9360a647d00af
    resource: repo://crates/remote/src/github_app/pr_review.rs
  - id: openwiki-source-502510e01f3135fb1c5219e4
    resource: repo://crates/remote/src/routes/review.rs
  - id: openwiki-source-98acd1706f9355ed92cb62a8
    resource: repo://docs/docs.json
  - id: openwiki-source-beb5b5e9c33782df52c2de7e
    resource: repo://docs/getting-started.mdx
  - id: openwiki-source-ffe48d9fc170154e85861afd
    resource: repo://docs/settings/creating-task-tags.mdx
  - id: openwiki-source-26312f1dedcae510b462cb43
    resource: repo://npx-cli/src/download.ts
  - id: openwiki-source-f5c8f8cc45fdf36d98b214ce
    resource: repo://packages/local-web/src/app/entry/Bootstrap.tsx
  - id: openwiki-source-2bc26d0169842f4566548104
    resource: repo://packages/local-web/src/app/providers/ConfigProvider.tsx
  - id: openwiki-source-d4726fdd33da0f7c2f1ae1ee
    resource: repo://packages/web-core/src/shared/lib/remoteApi.ts
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
generated: { by: "codex", at: "2026-09-16T18:13:42.498Z" }
---

# クイックスタートと調査案内

easy-vibe-kanban（EVK）は、Issue で仕事を整理し、Workspace 内で coding agent を動かし、成果物をレビューして Git に統合する製品である。README は「計画と Agent 出力のレビューを速くする」ことを目的として記録し、複数の Session を graph で協働させる Workflow を主要機能に位置づける。[製品の目的](../README.md) 実装の境界は [システム構成](architecture/system.md)、一連の操作は [Issue から統合まで](workflows/task-to-integration.md) から読む。

## 最初の作業

1. 利用する coding agent の認証を済ませて EVK を起動する。配布版の入口として README は npx easy-vibe-kanban を案内する。現在の Easy 同梱版は Linux/Windows x64 を対象にするため、[NPX の配布経路と platform 条件](operations/development.md#npx-配布経路と同梱-binary) を確認する。ソースから開発する場合は [開発環境と検証範囲](operations/development.md) を参照する。[配布版の案内](../README.md)
2. [Project / Issue](concepts/project-and-issue.md) と、変更先の [Repo / Workspace](concepts/workspace.md) を確認する。仕事の分類と、実ファイルを置く場所は別の設定である。
3. 通常 Session、[WorkflowAttempt](concepts/workflow-attempt.md)、または [Arena](concepts/arena.md) を選び、[Setup → 実行 → レビュー → 統合](workflows/task-to-integration.md) を進める。
4. canonical repository memory を使う場合は [OpenWiki 初期生成](operations/openwiki-maintenance.md) を別途設定する。[Card context の LLM Wiki](concepts/card-context-and-llm-wiki.md) と役割を混同しない。成果物の確認は [Wiki Viewer の表示元・形式の選択](operations/workspace-inspection.md#wiki-の本文ツリーと表示元を確認する)から行い、閲覧できることと publication 成功を区別する。

画面の基本操作は [Get Started](../docs/getting-started.mdx) が入口になる。ただし同文書の「sign-in を省略すると看板・Issue が利用不可」という記載は、現行の local-web 全体には適用できない。local-web は local API を有効化し、remote API base が空なら local identity と /api/local の経路を使う。[旧来の説明](../docs/getting-started.mdx)・[local 設定](../packages/local-web/src/app/providers/ConfigProvider.tsx#L122-L125)・[identity](../packages/local-web/src/app/entry/Bootstrap.tsx#L82-L90)・[API 選択](../packages/web-core/src/shared/lib/remoteApi.ts#L19-L32)

## 主要概念の説明先

| 知りたいこと | 説明の本拠地 |
| --- | --- |
| 共有データを誰が所有し、誰を招待・管理できるか | [Organization・Member・Invitation](concepts/organization-and-membership.md)。個人用組織、Admin、招待 token の寿命 |
| 何を作るか、誰の仕事か、status や親子関係は何か | [Project・Issue](concepts/project-and-issue.md)。ボード表示と共有 status の差、Workspace との link と旧 Task の違いもここ |
| Issue を共同編集し、添付を確定し、変更を受け取るには | [Issue の共同作業](concepts/issue-collaboration.md)。Attachment/Blob、購読者、通知の生成と配達 |
| どこで変更するか、何を共有し、何を削除できるか | [Workspace・Repo](concepts/workspace.md)。Worktree / DirectFolder、所有権、共有ディレクトリ、archive と cleanup |
| 会話、実行、再試行をどう識別するか | [Session・AgentRun・RunAttempt](concepts/session-and-agent-run.md)。native session、現在の承認配線、未送信ドラフト、Goal、queue、子 agent の境界 |
| 複数段階の仕事をどう定義して動かすか | [WorkflowAttempt](concepts/workflow-attempt.md)。template と Run、分岐・合流、人の判断待ち |
| 候補をどう比較し、採用後の実装へ移るか | [Arena](concepts/arena.md)。Design / Implementation、候補、promotion と Workflow 内の選択 |
| prompt の再利用 context と branch 上の知識をどう扱うか | [Card context・Pipeline・LLM Wiki](concepts/card-context-and-llm-wiki.md) |
| 並列作業の知識を統合済み repository にどう反映するか | [Repository memory](concepts/repository-memory.md)。canonical Wiki、Change Manifest、Workspace Memory、source checkpoint と receipt |

## 変更内容から調査する

| 変更・障害 | 調査経路 |
| --- | --- |
| 起動、DB、desktop と server の差 | [システム境界と起動順](architecture/system.md) → [開発・検証](operations/development.md) |
| Agent が止まった、表示と監査が合わない、再接続したい | [Session の状態](concepts/session-and-agent-run.md) → [Agent Runtime の監査・投影・回復](architecture/agent-runtime.md) |
| provider や model 設定、MCP、Skill の追加 | [Provider integration](integrations/agent-providers.md)。effort override と解決済み値、外部 MCP を使う方向と EVK MCP を公開する方向を分ける |
| Workflow の分岐、取消、outbox、再起動回復 | [Graph の契約](concepts/workflow-attempt.md) → [Workflow Runtime](architecture/workflow-runtime.md) |
| 定期的に作業を実行したい | [ScheduledTask の起動・skip 条件](workflows/task-to-integration.md#定時に-workflow-を起動する) |
| frontend、host 選択、Issue 同期 | [Web と同期](architecture/web-and-sync.md) → [Remote と認証](integrations/remote-access.md) |
| ファイル、Session 添付、diff、preview、terminal、editor | [Workspace inspection](operations/workspace-inspection.md)。Cloud 添付は [Issue の共同作業](concepts/issue-collaboration.md) |
| ログインが切れる、refresh が競合する | [Cloud AuthSession の寿命](integrations/remote-access.md#cloud-authsession-と-token-の寿命)。ホスト署名用 session と区別する |
| PR 自動レビューが始まらない、結果通知が来ない | [GitHub App と Cloud Review](integrations/github-review.md)。組織設定・webhook・R2/worker の受付と完了を分ける |
| 実行の元データを保全したい | [Native Audit の保存・エクスポート・削除](architecture/agent-runtime.md#native-audit-の保全エクスポート寿命) |
| Merge / Rebase / PR 後の状態が合わない | [統合までの順序と部分失敗](workflows/task-to-integration.md) → [memory の統合契約](concepts/repository-memory.md) |
| Wiki を読む、形式を切り替える、表示対象が違う | [Wiki の本文・ツリーと表示元](operations/workspace-inspection.md#wiki-の本文ツリーと表示元を確認する) → [共有 reader とチャット保持](architecture/web-and-sync.md#wiki-のナビゲーションと本文を一つの状態で動かす) |
| Wiki が stale/error、Bootstrap が途中で止まる | [OpenWiki の phase・共通索引・レポート・完了証拠・回復](operations/openwiki-maintenance.md)。MCP 起動前確認と公開条件を分けて診断する |
| モバイル・self-hosting・公開運用 | [Remote の接続境界](integrations/remote-access.md) → [運用文書の使い分け](operations/development.md) |

## 小さな用語の境界

### Prompt 用 Tag と Issue の分類 Tag

Settings の Tag は tag_name/content を持つ、再利用する prompt text の保存モデルである。[Creating Tags](../docs/settings/creating-task-tags.mdx) は @mention から snippet を挿入する操作を説明する。Issue の分類 Tag は project_id/name/color を持ち、IssueTag が issue_id と tag_id を関連づける。名前が同じでも保存対象と利用目的が異なる。[prompt Tag](../crates/db/src/models/tag.rs#L7-L25)・[分類 Tag](../crates/api-types/src/tag.rs#L7-L23)・[IssueTag](../crates/api-types/src/issue_tag.rs#L5-L19)

### Cloud の review service

Remote の Cloud Review は、公開 upload API または組織の GitHub App から外部 worker へ依頼する機能である。設定・起動イベント・部分失敗・結果通知の一次説明は [Cloud Review と GitHub App](integrations/github-review.md) に置く。通常 Workspace の [Agent review](workflows/task-to-integration.md#3-会話を続け成果物を確認する)、Workflow、[OpenWiki の独立 Review](operations/openwiki-maintenance.md) は別の lifecycle である。[Remote の入口](../crates/remote/src/routes/mod.rs#L103-L140)

## 既存文書を使うとき

操作手順は Wiki へ全文コピーせず、[文書 navigation](../docs/docs.json) から原典を使う。Workspace 操作、Cloud の組織・チーム・filter、Settings、agent 別設定、IDE / Git host integration が主な文書群である。[navigation](../docs/docs.json#L21-L96)

実装の確認には source・tests・実際の設定を使い、文書は目的・契約・過去の判断の証拠として状態を区別する。特に次の適用差は各説明先で扱う。

- README の循環例と現行 graph validator、prompt を渡さないという説明と現行 template 展開：[Workflow の文書照合](concepts/workflow-attempt.md)。
- Bootstrap v2 の一部を後続 implementation record が明示的に更新：[OpenWiki 設計文書](operations/openwiki-maintenance.md#設計文書の読み方)。
- Arena v2 と AI Mobile の draft を、提供済み機能の証明にしない：[Arena](concepts/arena.md)・[Remote / Mobile](integrations/remote-access.md)。
- Cloud 文書の認証期限・招待手順・filter 説明は、それぞれ [認証](integrations/remote-access.md)、[組織](concepts/organization-and-membership.md)、[ボード](concepts/project-and-issue.md#ボード表示と共有状態) で現行実装への適用範囲を記す。
- README の Rust 検証範囲と実 manifest の差：[開発・検証](operations/development.md)。

## カバー範囲と限界

この構成は、README と文書 index に加え、local server の router/startup、独立 Remote router、MCP の公開 router、Workflow/Agent runtime、Git と memory の境界から照合している。文書の章構成や直近差分だけを説明範囲にはしていない。[local API の入口](../crates/server/src/routes/mod.rs#L44-L98)・[Remote の入口](../crates/remote/src/routes/mod.rs#L103-L140)・[MCP の入口](../crates/mcp/src/task_server/tools/mod.rs#L52-L73)

この Wiki は実装理解のための要約であり、すべての UI 項目や依存 package の一覧ではない。外部サービス、配布済み binary、IDE extension、model の意味的な出力品質は、この source 調査だけでは確定しない。製品 test の実行結果については [検証記録の限界](operations/development.md) を参照する。
