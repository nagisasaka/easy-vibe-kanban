---
type: concept
title: Project・Issue と実施の関係
description: ボード上の仕事と実行環境を結ぶ Project・Issue の意味、ローカル保存、旧 Task との境界。
tags: [project, issue, kanban, task]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-16T18:13:42.498Z
sources:
  - id: openwiki-source-11356f66af906ebb06f3ef2e
    resource: repo://crates/api-types/src/issue.rs
  - id: openwiki-source-e4c80fe5a2af8a4abd8a8684
    resource: repo://crates/db/migrations/20260427000000_local_kanban.sql
  - id: openwiki-source-ccd357b1781e6e90e56fd858
    resource: repo://crates/db/src/models/repo.rs
  - id: openwiki-source-1ffb01d1ba3953937e0771bf
    resource: repo://crates/server/src/routes/local_remote.rs
  - id: openwiki-source-a517c0f8e44d2cf0c440fbfc
    resource: repo://crates/server/src/routes/workflows.rs
  - id: openwiki-source-38de6ae4b89edddb12325c18
    resource: repo://crates/server/src/routes/workspaces/links.rs
  - id: openwiki-source-9870279ad197d3d37bb6575a
    resource: repo://docs/cloud/filtering.mdx
  - id: openwiki-source-36bc16e51274306c238a63a2
    resource: repo://packages/web-core/src/features/kanban/model/hooks/useKanbanFilters.ts
  - id: openwiki-source-e0c2de9e01617a8949b78360
    resource: repo://packages/web-core/src/features/kanban/ui/KanbanContainer.tsx
  - id: openwiki-source-00110cdcfec890895b99690a
    resource: repo://packages/web-core/src/shared/dialogs/settings/settings/RemoteProjectsSettingsSection.tsx
  - id: openwiki-source-b885bc17dfe85c4ca45223e9
    resource: repo://packages/web-core/src/shared/hooks/useUiPreferencesScratch.ts
  - id: openwiki-source-5ef837b2bc54d286b3dedd8f
    resource: repo://packages/web-core/src/shared/stores/useUiPreferencesStore.ts
generated: { by: "codex", at: "2026-09-16T18:13:42.498Z" }
---

# Project・Issue と実施の関係

**Project** は関連する仕事を集める単位で、Issue、ボードの Status、分類用 Tag を持つ。**Issue** は「何を達成するか」を表す仕事である。AI が作業する場所は [Workspace](workspace.md)、会話と実行は [Session・AgentRun](session-and-agent-run.md) が受け持つ。この分離により、一件の仕事に複数の実施や比較候補を関連付けられる。[製品上の定義](../../docs/cloud/projects.mdx#what-is-a-project)、[Issue 契約](../../crates/api-types/src/issue.rs#L20-L40)、[関連表](../../crates/db/migrations/20260427000000_local_kanban.sql#L208-L223)

Cloud で Project を所有するのは [Organization](organization-and-membership.md)である。所属と Admin の違い、個人用組織、招待の受理はそのページにまとめる。Issue の担当者・Follower は組織 role ではない。

## 仕事の識別と分類

Issue の主キーは UUID。表示用 `simple_id` と番号、Project、Status、本文、任意の優先度・期限・完了日時、親 Issue、拡張メタデータを持つ。ローカル作成は Project ごとの最大番号から `LOCAL-N` を採番する。DB の番号・simple ID の一意性も Project 内なので、複数 Project を横断する処理で表示文字列だけをグローバル ID と扱わない。[型](../../crates/api-types/src/issue.rs#L20-L40)、[採番](../../crates/server/src/routes/local_remote.rs#L737-L748)、[一意制約](../../crates/db/migrations/20260427000000_local_kanban.sql#L45-L69)

Status は Project の列を参照する ID であり、エージェントのプロセス状態ではない。ローカルの初期列は Todo / In Progress / In Review / Done / Cancelled で、Cancelled は初期状態で非表示。更新 API は `status_id` と `completed_at` を別々の入力として保持するため、列を Done に変えるだけで完了日時が自動計算されるとは読めない。[初期列](../../crates/server/src/routes/local_remote.rs#L49-L55)、[更新](../../crates/server/src/routes/local_remote.rs#L797-L852)

Issue の分類と関係はそれぞれ別の意味を持つ。

| 情報 | 意味と保存上の関係 |
| --- | --- |
| Parent / sub-issue | 仕事の分解。親削除時は子の親参照が null になる |
| Assignee | 担当者。Issue と user の複数関連を保存する |
| Follower | 追跡する利用者。担当者とは別の関連 |
| Tag | Project 内の分類ラベル。Issue と多対多で結ぶ |
| Relationship | blocking / related / has_duplicate を保存する |

この関係を [Workflow の制御エッジ](workflow-attempt.md) と同じ依存グラフとして扱う根拠はない。表の保存契約は [スキーマ](../../crates/db/migrations/20260427000000_local_kanban.sql#L125-L179) と [親参照](../../crates/db/migrations/20260427000000_local_kanban.sql#L58-L68) を参照。ボード操作、コメント、絞り込みの利用手順は [Issues](../../docs/cloud/issues.mdx) が詳しい。

部分更新では「送らなかった」と「null に消した」を区別する。たとえば description や priority は二重 Option であり、未指定なら既存値を保持し、明示 null ならクリアする。フォームや API クライアントの変更でこの違いを失うと、無関係な編集で情報を消し得る。[更新リクエスト](../../crates/api-types/src/issue.rs#L79-L146)、[適用](../../crates/server/src/routes/local_remote.rs#L828-L847)

## ボード表示と共有状態

Team / Personal は同じ Project の Issue 集合を見る filter preset であり、所有権・公開範囲を切り替えるものではない。既定の Team は担当者を絞らず手動順、Personal は現在 user の担当 Issue・優先度順で sub-issue も表示する。Project ごとの active view と、Project / view ごとの検索・sort・表示 option を UI preferences に保存する。[preset](../../packages/web-core/src/shared/stores/useUiPreferencesStore.ts#L82-L209)・[保存キー](../../packages/web-core/src/shared/stores/useUiPreferencesStore.ts#L744-L783)

保存先は [Scratch の runtime 境界](../architecture/web-and-sync.md#ドラフト表示設定通知の保存境界)に従う。local-web では固定の UI preferences レコード、remote-web ではブラウザーの localStorage であり、Cloud user ごとのサーバー保存を仮定しない。[Scratch 化](../../packages/web-core/src/shared/hooks/useUiPreferencesScratch.ts#L27-L98)・[保存](../../packages/web-core/src/shared/hooks/useUiPreferencesScratch.ts#L235-L260)

| 操作 | 変更する対象 |
| --- | --- |
| 検索、優先度・担当者・Tag の filter、表示 sort | view の設定。元 Issue を変更しない |
| 同じ列での手動並べ替え | 影響する Issue の共有 `sort_order`。Manual 以外では拒否 |
| 列をまたぐカード移動 | 移動先 Issue 群の `status_id` / `sort_order`、移動元の順序 |
| Status の並べ替え・非表示設定 | Project Status の `sort_order` / `hidden`。下記の blocked 判定にも影響 |

[絞り込み](../../packages/web-core/src/features/kanban/model/hooks/useKanbanFilters.ts#L74-L169)・[カードの mutation](../../packages/web-core/src/features/kanban/ui/KanbanContainer.tsx#L712-L806)・[Status の保存](../../packages/web-core/src/shared/dialogs/settings/settings/RemoteProjectsSettingsSection.tsx#L821-L880)

`hideBlocked` は blocking 関係の元 Issue を blocker として調べる。blocker が **最後の可視 Status（sort_order 順）または任意の hidden Status** にあれば解決済みとして扱い、それ以外なら被ブロック Issue を隠す。参照先 blocker が手元の集合に存在しなければ、ブロック中と判定しない。名前が Done か、`completed_at` が埋まっているかという判定ではない。列の順序・非表示化は見た目だけでなく、この依存表示の意味を変える。[解決済み Status 集合](../../packages/web-core/src/features/kanban/ui/KanbanContainer.tsx#L311-L325)・[blocking の向きと判定](../../packages/web-core/src/features/kanban/model/hooks/useKanbanFilters.ts#L149-L169)

[Kanban の移動操作](../../docs/cloud/kanban-board.mdx#drag-and-drop)、[列のカスタマイズ](../../docs/cloud/customisation.mdx#managing-columns)、[Filtering](../../docs/cloud/filtering.mdx)は廃止表示のない操作文書である。ただし Filtering の「タイトルのみ検索」「Project 切替で filter リセット」は現在の実装に適用できない。検索は title / simple_id / issue_number の部分一致で、Project / view 別設定を保持する。filter はカテゴリー内 OR、カテゴリー間は順に適用する AND である。[検索・filter](../../packages/web-core/src/features/kanban/model/hooks/useKanbanFilters.ts#L82-L147)・[Project 別設定の復元](../../packages/web-core/src/features/kanban/ui/KanbanContainer.tsx#L244-L285)

## Issue の共同作業

Cloud の Attachment は Issue / Comment と Blob の関連を持ち、Session の添付 File と別の寿命である。[添付の確定・清掃・Workspace への取り込み](issue-collaboration.md#attachment-と-blob)を参照する。Follower と Assignee から誰に変更が通知されるか、保存成功でも通知が欠ける条件、既読の意味は [購読と通知](issue-collaboration.md#購読と通知)に集約する。

## Issue から実施へ

通常の Workspace 作成は `linked_issue` を受け取り、コンテナーと Session を作る前に関連付けを行う。ローカルでは `local_workspace_links.workspace_id` が主キーで、一つの Workspace の関連先を upsert する。一件の Issue は複数 Workspace を持てる。関連解除はリンクの削除であり、Workspace の作業ファイル削除とは別の操作である。[作成順](../../crates/server/src/routes/workspaces/create.rs#L419-L438)、[関連付け・解除](../../crates/server/src/routes/workspaces/links.rs#L24-L59)

「実施」には複数の形がある。

- 単独 Workspace とその Session で進める。
- [Arena](arena.md) により同じ Issue の候補を並べ、設計または実装を比較する。
- [WorkflowAttempt](workflow-attempt.md) により Issue 専用の編集可能なグラフと実施環境を持つ。

WorkflowAttempt は Project と Issue の所属を検証し、通常のテンプレート一覧から隠す専用 Workflow を作って draft として保存する。Issue のタイトル・本文から起動入力を作る処理は、グラフの定義とは分離されている。[作成契約](../../crates/server/src/routes/workflows.rs#L627-L669)、[入力構築とテスト](../../packages/web-core/src/features/workflow/model/issueWorkflow.test.ts#L8-L25)。実際の操作の順序は [依頼から統合まで](../workflows/task-to-integration.md) にまとめる。

## ローカルとクラウドの意味の違い

共通 API の Project は organization ID と表示メタデータを含む。一方、ローカル DB の `projects` は作業ディレクトリ既定値と remote project ID を持ち、`local_project_metadata` を join してボード向けの形にする。同じ名前の Rust 型だけから同じ保存モデルだと判断しない。[共通型](../../crates/api-types/src/project.rs#L8-L17)、[ローカル型](../../crates/db/src/models/project.rs#L7-L17)、[組み立て](../../crates/server/src/routes/local_remote.rs#L375-L401)

ローカル互換 API は固定の Local organization と Local User/Admin を返す。この擬似メンバーシップを、[Cloud の組織所属と管理権限](organization-and-membership.md) と同等の利用者管理だと説明しない。[ローカル応答](../../crates/server/src/routes/local_remote.rs#L1205-L1233)。REST と Electric の取得経路は [Web と同期](../architecture/web-and-sync.md) が説明する。

Workspace をクラウドへリンクする経路では、ローカルリンクを先に保存し、remote client があればリモート Workspace を作る。後段の通信失敗はエラーとして返り、ローカル保存との一括ロールバックはない。解除はリモート削除を先に行い、404 は既に解除済みとしてローカルを消す。この非対称性は再試行・表示不整合を調べる際に重要である。[リンク順序](../../crates/server/src/routes/workspaces/links.rs#L62-L86)、[解除順序](../../crates/server/src/routes/workspaces/links.rs#L148-L164)

## 旧 Task を読むとき

`tasks` と固定 enum の `TaskStatus` はソースに残る。ローカル Kanban 導入 migration は Task ID を保って `local_issues` へコピーし、旧 `workspaces.task_id` からリンク表を初期化した。現在のボード取得はリンク表と `local_issues` を join する。このため、旧 Task の状態や `task_id` だけを追って現行 Issue の挙動を説明すると境界を見失う。[移行](../../crates/db/migrations/20260427000000_local_kanban.sql#L75-L123)、[リンク移行](../../crates/db/migrations/20260427000000_local_kanban.sql#L225-L231)、[現行取得](../../crates/server/src/routes/local_remote.rs#L1142-L1173)

Repo はパス、対象ブランチ既定値、setup/cleanup/dev script などの実行資源を持つ。[Workspace](workspace.md) がそれを選択して作業環境を構成するので、Project を Git リポジトリ一個と同一視しない。[Repo の責任](../../crates/db/src/models/repo.rs#L36-L54)
