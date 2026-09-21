---
type: concept
title: Workspace と Repository
description: 作業環境の所有権と用途、実行所有者、複数 Repo、共有資源、削除と回復の境界。
tags: [workspace, repository, worktree, ownership, storage]
sources:
  - id: openwiki-source-94559828ba40b7dfdd09bad0
    resource: repo://crates/db/migrations/20260921000000_workspace_usage.sql
  - id: openwiki-source-ccd357b1781e6e90e56fd858
    resource: repo://crates/db/src/models/repo.rs
  - id: openwiki-source-d76d1a64a0c116ad812aa678
    resource: repo://crates/db/src/models/workspace_repo.rs
  - id: openwiki-source-dfa8135bd6d4dbe37911c061
    resource: repo://crates/db/src/models/workspace_usage.rs
  - id: openwiki-source-5f083da2f629bcd57e386ad4
    resource: repo://crates/db/src/models/workspace.rs
  - id: openwiki-source-343f68138e6e2d3312d2774c
    resource: repo://crates/local-deployment/src/container.rs
  - id: openwiki-source-7464a2d1db8e85c09420cf28
    resource: repo://crates/server/src/middleware/model_loaders.rs
  - id: openwiki-source-6a0749527a9648c33ba957da
    resource: repo://crates/server/src/routes/workspaces/core.rs
  - id: openwiki-source-bdb4dae5722fe76ec5e44001
    resource: repo://crates/server/src/routes/workspaces/create.rs
  - id: openwiki-source-c128bdfa7a156621b004df06
    resource: repo://crates/services/src/services/workspace_usage.rs
  - id: openwiki-source-19419edea1d312cc9f07ef2d
    resource: repo://crates/utils/src/path.rs
  - id: openwiki-source-7d5590729f3e0f42d994b0e8
    resource: repo://crates/workspace-manager/src/shared_resources.rs
  - id: openwiki-source-965b490de6f8364cb7695944
    resource: repo://crates/workspace-manager/src/workspace_manager.rs
  - id: openwiki-source-08b3f4dcb1a91c97b6d127f8
    resource: repo://crates/worktree-manager/src/worktree_manager.rs
generated: { by: "codex", at: "2026-09-21T08:16:36.701Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-21T08:16:36.701Z
---

# Workspace と Repository

Workspace は、エージェントの [Session](session-and-agent-run.md)、作業ディレクトリ、ブランチ、接続 Repo を束ねる実施環境である。[Issue](project-and-issue.md) の仕事とは関連で結び、一つの Workspace に複数 Repo・Session を持てる。`container_ref` はローカル実装ではファイルシステム上のパスを表す。名称だけから Docker や OS の隔離境界を想定しない。[Workspace モデル](../../crates/db/src/models/workspace.rs#L62-L82)、[ローカルの作成](../../crates/local-deployment/src/container.rs#L1283-L1322)

## Repo と WorkspaceRepo

**Repo** は登録された source パスと、setup/cleanup/archive/dev script、copy files、target branch・working directory の既定値を持つ。**WorkspaceRepo** は登録 Repo を特定 Workspace に接続し、その実施で使う target branch を保存する。Repo の既定値と、既存 Workspace の選択を区別する。[Repo](../../crates/db/src/models/repo.rs#L36-L54)、[membership](../../crates/db/src/models/workspace_repo.rs#L11-L34)

通常の追加は Repo 存在、target branch 存在、重複 attachment を検証する。managed Workspace のルート直下に Repo 名ごとの Git worktree を作り、共通の作業 branch 名を各 Repo で使用する。各 Repo の Git 履歴・target branch は独立しているため、一つの Workspace が複数 Repo をまとめていても Git の一括 transaction にはならない。[追加検証](../../crates/workspace-manager/src/workspace_manager.rs#L133-L162)、[worktree 作成](../../crates/workspace-manager/src/workspace_manager.rs#L528-L571)、[複数 Repo の利用契約](../../docs/workspaces/multi-repo-sessions.mdx)

## 二つの環境と所有権

| 種別 | 作業場所 | ファイル所有 |
| --- | --- | --- |
| Worktree | EVK 管理のルート内に作る Repo ごとの worktree | Managed |
| DirectFolder | 利用者が選んだ既存ディレクトリ | External |

削除可能性は kind の名称からではなく `container_ownership` から判定する。External の container path を EVK が削除することは許可しない。[所有権](../../crates/db/src/models/workspace.rs#L35-L53)、[判定とテスト](../../crates/db/src/models/workspace.rs#L137-L148)、[削除テスト](../../crates/db/src/models/workspace.rs#L876-L886)

DirectFolder で選択先が開ける Git Repo なら Repo を登録し、現在の branch を使い、選択ディレクトリの親を container_ref にする。通常の Repo 相対解決で選択先に到達するためである。Git Repo でなければ選択先自体をルートとし、Repo membership を持たない。DirectFolder は新しい Git worktree を作る操作ではない。[DirectFolder 作成](../../crates/server/src/routes/workspaces/create.rs#L107-L168)、[既存パス検証](../../crates/local-deployment/src/container.rs#L1287-L1306)

Worktree の保管先を変更すると、指定先の `.vibe-kanban-workspaces` 子ディレクトリを使用する。既存ユーザーフォルダーを orphan cleanup の対象にしないため、アプリ所有の子ディレクトリに限定するという理由がソースに記録されている。[保管先](../../crates/worktree-manager/src/worktree_manager.rs#L511-L523)

## 操作用途と実行所有者

`usage` は Workspace で人が行える操作の契約で、物理的な kind/ownership、archive、実行中の一時予約とは独立する。既定の `interactive` は通常の開発用、`execution_only` は [正式統合](formal-integration.md)や [OpenWiki 保守](../operations/openwiki-maintenance.md)のために所有者が作った実行環境である。完了して予約が解放されても通常の開発 Workspace には変わらない。[用途モデル](../../crates/db/src/models/workspace_usage.rs#L1-L53)、[永続列](../../crates/db/migrations/20260921000000_workspace_usage.sql)

`execution_owner` は kind・run ID・Repo ID により製品の実行記録を指す。準備中は run ID が未確定でもよいが、dispatch 前に所有者へ結び付け、別 run に付け替えて過去の環境を再利用しない。Sync など製品側に残らない結果は、同じ所有者へ succeeded/failed/cancelled、Wiki commit、no-op、診断を保存する。AgentRun の成功だけでは公開結果を推定しない。[一度だけの結付けと結果保存](../../crates/db/src/models/workspace_usage.rs#L81-L131)

実行専用環境でも Session と監査ログは残り、ファイルが現存すれば閲覧できる。汎用の chat・Session 作成・retry・Git 操作・terminal・editor 起動・script・ファイル変更・ライフサイクル変更は通常の開発入口では許可しない。Workspace middleware は read と seen を残し、停止は所有者専用入口へ渡す。editor tunnel の承認に使う GET も禁止するため、HTTP の read method なら常に受動操作とは限らない。[共通境界](../../crates/server/src/middleware/model_loaders.rs#L64-L101)。表示と停止の導線は [実行履歴の確認](../operations/workspace-inspection.md#実行専用-workspace-の確認)を参照する。

所有者ラベル自体は起動権限ではない。各 dispatch で Repo membership と未完了の所有を確認し、Integration は保存済み Workspace/Session/correlation と予約を、Bootstrap は Workflow と現在の delegated child を、Sync は maintenance state と起動前に固定した AgentRun ID を照合する。不明 kind や欠けた owner は閲覧対象のままにし、任意の実行権限を与えない。[agent の検証](../../crates/services/src/services/workspace_usage.rs#L134-L237)

script の例外は IntegrationValidation だけで、validating 状態・未取消・同じ予約・Session/Repo・保存済み未完了検証項目の command/cwd を要求する。`next_action` による別の起動も許可しない。検証計画の意味と結果 commit の固定は [正式統合の host 検証](formal-integration.md)で説明する。[script の検証](../../crates/services/src/services/workspace_usage.rs#L260-L320)

既存データの移行は名称から推測しない。Integration/Repository bootstrap の実行記録、または一致する maintenance の保存記録を根拠に分類し、証拠なしは interactive のままにする。過去 Sync の AgentRun が一つに定まらなければ run ID は不明のまま、複数の所有証拠が衝突すれば `conflicting_evidence` として閲覧専用にする。archive・Session・監査・Git 履歴を書き換える移行ではない。[証拠に基づく backfill](../../crates/services/src/services/workspace_usage.rs#L23-L129)

## 作成・準備の失敗

複数 Repo の worktree は順に作り、途中で失敗した場合はそれまで作った worktree の cleanup を試みて PartialCreation を返す。DB、ファイル作成、添付ファイル取り込み、AgentRun 起動を一つの完全な transaction とみなさない。[部分作成](../../crates/workspace-manager/src/workspace_manager.rs#L545-L606)

Agent 起動前の setup script は Repo 設定に従う。DirectFolder では自動 setup 対象を空にする。全ての setup が parallel 指定なら先行 script と agent を並行起動でき、それ以外は [起動ゲート](../workflows/task-to-integration.md) を経由する。この順序を変えると、依存物が未準備の agent 起動につながる。[setup 分岐](../../crates/server/src/routes/workspaces/create.rs#L448-L486)

## 共有ディレクトリの意味

managed Git Workspace の各 Repo にある `.evk-shared/persistent/` と `.evk-shared/cache/` は、**同じ登録 Repo を使う他 Workspace と内容を共有するリンク**である。保存先は既定 storage base の `shared/<sanitized-name>-<full-repo-UUID>/`。worktree の保管先 override とは独立している。[パス生成](../../crates/utils/src/path.rs#L140-L157)、[README の運用契約](../../README.md)

- persistent は利用者が維持するローカルファイル。[Repository Memory](repository-memory.md) の記録もその下に置く。
- cache は再生成できる成果物や cache。ツールごとの cache 設定を EVK が自動変更するものではない。
- Workspace 削除ではリンク先の共有内容を削除しない。自動清掃やバックアップが保証される領域でもない。
- DirectFolder ではこれらのリンクがない場合がある。存在を前提にする前に作業環境を確認する。

[共有の利用契約](../../README.md)、[削除後もデータを保持するテスト](../../crates/workspace-manager/src/shared_resources.rs#L171-L224)

provisioner は tracked な `.evk-shared`、既存の通常ファイル、異なる宛先のリンクを拒否し、全リンクを検査してから作成する。Git のローカル `info/exclude` に追加し、利用者のパスを上書きしない。Windows のリンク作成には Developer Mode または symlink 権限が必要で、作成失敗は明示エラーになる。[検証・配置](../../crates/workspace-manager/src/shared_resources.rs#L41-L126)、[Windows](../../crates/workspace-manager/src/shared_resources.rs#L134-L136)

provision lock は配置処理を守る。利用者の共有ファイルへの同時書き込みまで直列化するものではない。共有内容を使うツール側が競合を扱う必要がある。[配置 lock](../../crates/workspace-manager/src/shared_resources.rs#L13-L20)、[共有利用時の指示](../../packages/web-core/src/features/pipeline/model/cardContext.ts#L15-L23)

## Archive、削除、期限 cleanup

Archive は Workspace の状態更新であり、その後に archive 処理を呼ぶ。後段失敗はログに残し、状態更新の成功応答を返す。Archive と完全削除の完了を同じ表示状態で判断しない。[更新順](../../crates/server/src/routes/workspaces/core.rs#L43-L87)

明示削除の DB transaction は書き込み lock を先に取得し、非終端 AgentRun または spawned/running/unreachable の process registry が残る場合に拒否する。生きた orchestration の関連も保護する。terminal の正本表示だけで実プロセス消滅を証明したことにはならない。[削除 guard](../../crates/workspace-manager/src/workspace_manager.rs#L233-L349)

DB 削除後、ファイル・runtime ファイル・指定時の branch cleanup を background に渡し、API は 202 Accepted を返す。リモート削除や background cleanup の失敗はログに残り得るので、202 は全保存先の物理削除完了を意味しない。External Workspace は削除 context に container path を入れない。[削除 API](../../crates/server/src/routes/workspaces/core.rs#L141-L182)、[所有境界](../../crates/workspace-manager/src/workspace_manager.rs#L174-L205)、[background cleanup](../../crates/workspace-manager/src/workspace_manager.rs#L436-L483)

明示削除は Session の process logs と **Native Audit のディレクトリも削除対象**にする。この処理は container path の有無とは独立しているため、External の作業ファイル保護は監査履歴の保持を意味しない。調査・保全が必要なら削除前に [Native Audit の保存先とエクスポート契約](../architecture/agent-runtime.md#native-audit-の保全エクスポート寿命) を確認する。[削除対象と順序](../../crates/workspace-manager/src/workspace_manager.rs#L436-L515)

期限 cleanup は別経路で、30 分間隔に候補を探す。SQL は managed、未削除、旧 ExecutionProcess の活動と時刻を見て、archive は 1 時間・それ以外は 72 時間を閾値にする。`DISABLE_WORKTREE_CLEANUP` が環境に存在すれば期限 cleanup を止める。[候補 SQL](../../crates/db/src/models/workspace.rs#L337-L393)、[周期と停止設定](../../crates/local-deployment/src/container.rs#L626-L669)

実際の期限 cleanup は Integration の予約と execution owner を再検証する。実行専用環境は非終端 AgentRun、reserved/spawned/running/unreachable の registry、実行中 script があれば保持する。さらに所有者の終端を要求し、Integration の recovery_required や active Wiki maintenance、不明 owner は削除しない。検証エラーでも保持する。[cleanup 本体](../../crates/local-deployment/src/container.rs#L565-L623)、[所有者ごとの保持条件](../../crates/services/src/services/workspace_usage.rs#L323-L375)

**変更時の確認点:** この追加 guard は interactive の非予約 Workspace には AgentRun/registry 検査を加えない。通常環境の期限 cleanup と明示削除には依然として差があり、長時間の AgentRun に対する安全性を明示削除から類推しない。実行専用環境のファイルが cleanup 済みなら、閲覧要求で worktree や script を再作成せず明示エラーにする。保存済みログは別に閲覧できる。[受動的な閲覧ルート](../../crates/services/src/services/workspace_usage.rs#L240-L257)

利用手順は [Multi-Repo & Sessions](../../docs/workspaces/multi-repo-sessions.mdx)、レビュー・ファイル操作の境界は [Workspace の確認と実行](../operations/workspace-inspection.md)、Git 統合までの順序は [依頼から統合まで](../workflows/task-to-integration.md) を参照。
