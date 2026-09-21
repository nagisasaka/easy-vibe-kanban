---
title: "Workspaceの利用契約 — 継続開発用と実行専用"
description: "Workspaceの表示、ユーザー操作、実行所有権を分離し、内部実行を閲覧可能な履歴として扱うための実装仕様。"
---

## 1. 目的と適用範囲

Workspaceに汎用的な利用契約を導入する。通常の開発環境と、製品機能が所有する実行環境を区別し、後者が通常の作業一覧を埋め尽くしたり、ユーザーの追加チャットによって実行契約を壊したりすることを防ぐ。

本仕様の中心は「Workflowを二種類に分ける」ことではなく、**Workspaceを誰が、何の契約で操作できるかを明確にすること**である。OpenWikiやIntegrationという機能名を表示・操作制限の共通条件にしない。

- **継続開発用（Interactive）**: ユーザーがチャット、Session追加、通常の開発操作を行うWorkspace。
- **実行専用（Execution-only、ユーザー向けには閲覧専用）**: 所有する実行処理が変更し、ユーザーは進捗・ログ・ファイル・差分・成果物を調査するWorkspace。
- 実行専用は通常一覧から既定で除外するが、明示的な実行・履歴の入口から閲覧できる。非表示はアクセス禁止や削除ではない。
- 実行停止や承認などは、Workspaceの自由編集ではなく、所有する実行の正規の制御操作として扱う。

今回の対象はlocal EVKと、そのWorkspaceを表示・操作する共有フロントエンド。remote Boardの新しい実行機能、独立remote backendへの同等機能追加は対象外とする。既存のhost-scopedな閲覧・通信とremote-webの動作は維持する。

本文はこれから実装する要求であり、実装・検証済みの報告ではない。調査、migration、実装、自動テスト、レビュー、Chrome DevTools MCPによる操作確認、関連不具合の修正、運用文書までを一つの実装ゴールとする。

## 2. 調査基準と既存機構

仕様作成時点: 2026-09-21。ブランチは `feat/workspace-attrib`、HEADは `7d1b0685d4ed22dea2e4c4b3ef65d9bec25d0ccd`。ブランチ作成前後でHEADは同一で、仕様追加前の作業ツリーはクリーンだった。リモートの最新性は今回確認していない。

以下は静的なコード確認結果である。本仕様のmigration、変更後の実行やMCP試験は未実施。実装開始時には適用される `AGENTS.md`、本書全文、現在の差分と実コードを再確認する。行番号は移動し得るため、シンボルと呼出関係を優先する。

| 領域               | コード確認済みの事実・再利用点                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Workspaceモデル    | [workspace.rs](../../crates/db/src/models/workspace.rs) の `Workspace` は `workspace_kind`、`container_ownership`、`archived` 等を持つが、本仕様の利用契約はない。`WorkspaceKind::{Worktree, DirectFolder}` は物理的な作業場所、`ContainerOwnership::{Managed, External}` はファイル所有境界であり、流用しない。                                                                                                                                                                         |
| 作成               | [create.rs](../../crates/server/src/routes/workspaces/create.rs) の `create_workspace_record` を通常作業と内部実行の両方が使用する。公開作成APIと、内部の作成呼出を区別して拡張する。                                                                                                                                                                                                                                                                                                    |
| 通常Workflow       | [workflow.rs](../../crates/db/src/models/workflow.rs)、[workflows.rs](../../crates/server/src/routes/workflows.rs) の `WorkflowAttempt`、`ensure_agent_node_sessions`、`run_workflow_attempt_runtime_with_arena` はWorkspaceやnodeのSessionを再利用する。`WorkflowSource::System` はテンプレートの由来であって、実行専用の意味ではない。                                                                                                                                                 |
| OpenWiki           | [openwiki.rs](../../crates/server/src/routes/openwiki.rs) の `prepare_run` が専用Workspaceを作成し、初回は [bootstrap.rs](../../crates/server/src/workflow_runtime/bootstrap.rs) の `start` へ、それ以外は通常Syncへ進む。Bootstrap全体と各childは別の所有・権限境界を持つ。                                                                                                                                                                                                             |
| Formal Integration | [integrations/runtime.rs](../../crates/server/src/routes/integrations/runtime.rs) の `prepare` が専用Workspace／Sessionを作る。[integration.rs](../../crates/db/src/models/integration.rs) の `IntegrationRun.workspace_id` は持続的な対応、`integration_reservations` は一時的な業務予約。採用元の開発Workspaceも予約される。                                                                                                                                                           |
| Agent実行          | [agent_run_port.rs](../../crates/local-deployment/src/agent_run_port.rs) にSession／Workspace照合、Integration dispatch guard、OpenWiki ownerとReviewerの照合がある。共通利用契約を加えても、個別実行の厳しい条件を置き換えない。                                                                                                                                                                                                                                                        |
| 表示               | [useWorkspaces.ts](../../packages/web-core/src/shared/hooks/useWorkspaces.ts)、[streams.rs](../../crates/server/src/routes/workspaces/streams.rs)、[workspace_summary.rs](../../crates/server/src/routes/workspaces/workspace_summary.rs) は主にarchive状態で一覧を分ける。[WorkspacesSidebarContainer.tsx](../../packages/web-core/src/pages/workspaces/WorkspacesSidebarContainer.tsx) と [WorkspacesSidebar.tsx](../../packages/ui/src/components/WorkspacesSidebar.tsx) を拡張する。 |
| 個別表示と既定値   | [WorkspaceProvider.tsx](../../packages/web-core/src/shared/providers/WorkspaceProvider.tsx) はID指定取得を一覧と別に行う。[CreateModeProvider.tsx](../../packages/web-core/src/features/create-mode/model/CreateModeProvider.tsx) は最近のWorkspaceから作成時の既定値を選ぶ。一覧だけのフィルターでは他consumerへの混入を防げない。                                                                                                                                                      |
| 閲覧時の副作用     | [files.rs](../../crates/server/src/routes/workspaces/files.rs) の `get_workspace_file_tree`、`resolve_workspace_repo_root` は `ensure_container_exists` を呼ぶ。[wiki.rs](../../crates/server/src/routes/workspaces/wiki.rs) も関連する。GETであることだけから、環境を変更しないと仮定しない。                                                                                                                                                                                           |
| Archiveとcleanup   | [core.rs](../../crates/server/src/routes/workspaces/core.rs)、[services/container.rs](../../crates/services/src/services/container.rs) の `archive_workspace` はフラグだけでなくdev server停止・archive script実行を伴う。[local-deployment/container.rs](../../crates/local-deployment/src/container.rs) の期限cleanupと、WorkspaceのDB削除・Audit削除は別処理。                                                                                                                        |

既存Wikiの [Workspace](../../openwiki/concepts/workspace.md)、[Workspaceの確認と実行](../../openwiki/operations/workspace-inspection.md)、[OpenWiki maintenance](../../openwiki/operations/openwiki-maintenance.md) は探索の入口として利用する。ただし旧Wiki経路や過去のcleanup実装の記述が残るため、現行ソースを優先する。本変更のために旧 `.llm-wiki` を復活させたり、Wikiを直接書き換えたりしない。

## 3. 利用契約、所有者、表示の分離

### 3.1 利用契約

共通モデルに `interactive` / `execution_only` 相当の永続的な区別を追加する。名称・格納形式は実装時に既存の型・SQLx・API設計へ合わせてよいが、意味は以下を維持する。

| 項目                             | Interactive                  | Execution-only                                  |
| -------------------------------- | ---------------------------- | ----------------------------------------------- |
| 継続的な開発の主体               | ユーザーと通常Session        | 所有する実行処理                                |
| 通常作業一覧                     | 表示                         | 既定で除外                                      |
| 実行・履歴からの閲覧             | 既存動作を維持               | 可能                                            |
| 自由なチャット・新規Session      | 既存の予約・状態条件内で可能 | 不可                                            |
| ファイル・ログ・差分・Wikiの閲覧 | 可能                         | 存在する資料を閲覧可能                          |
| Agent／scriptによる書込          | 現行の実行権限に従う         | 所有者が許可した実行だけ。phase固有の権限も維持 |
| 完了後の扱い                     | 継続開発できる               | 閲覧専用のまま履歴に残る                        |

`ReadOnly` は**人間からのEVK操作契約**を表す。CodexのsandboxをすべてReadOnlyにする指示ではない。OpenWikiのGenerator／Refiner、Integrationの実装・検証処理は必要な書込を続ける。一方、OpenWiki Reviewerの既存ReadOnly設定は維持する。

`WorkspaceKind`、`ContainerOwnership`、archive状態、実行中か否かから利用契約を推論しない。単なる `visible` フラグで操作権限を表現しない。表示設定は利用契約に従うUIの選択であり、権限昇格の入力にはしない。

### 3.2 所有する実行との対応

実行専用Workspaceには、作成元・所有する実行を追跡できる永続的な対応を持たせる。共通層では小さなowner参照と操作判定を扱い、実行の状態機械・キャンセル・完了判定は既存の所有者に残す。

- BootstrapはWorkflow run全体がowner。Generate／Review／RefineのSession／AgentRunを個別のWorkspace ownerと誤認しない。
- 通常OpenWiki Syncは既存のmaintenance実行と対応付ける。
- Formal Integrationは `IntegrationRun` と対応付ける。
- 将来の別機能は、同じ利用契約と既存実行への参照を指定して参加できる。Sidebarやチャット部品に機能名ごとのif分岐を追加しなければ制限できない設計にしない。

新しい汎用Jobテーブル、scheduler、実行履歴の複製ストアは不要。既存のrun参照・Workspace属性・必要最小限の関連情報を使う。異なる種類のownerを識別する型や小さな解決helperは許容するが、新しいプラグイン基盤や権限エンジンは作らない。

owner参照は履歴・帰属の情報であり、dispatchを許可する万能トークンではない。実行時には現在のowner状態、Session／AgentRun、phase、既存の予約・権限を確認する。

### 3.3 寿命と一時的な制約

- 実行完了、失敗、取消、lease解放、EVK再起動によって `execution_only` を解除しない。
- Integrationが採用元A/Bを予約しても、A/Bの利用契約は `interactive` のまま。予約解除後は従来どおり開発を続けられる。
- 未知のowner種別や、既知の実行専用Workspaceでownerが欠落・不整合の場合は、閲覧と診断を維持し、自由な開発や未証明の新規dispatchは許可しない。
- 今回は利用契約の相互変換UI/APIを作らない。閲覧用に開く、URLを直接指定する、表示フィルターを変更することは変換ではない。
- 失敗した実行を通常チャットから修復する導線は作らない。既存の所有者が許す新規実行／復旧操作を案内する。新しい開発Workspaceへ成果物を移す機能も対象外。

## 4. 作成経路と既存データ

### 4.1 新規作成

通常Workspace、Issueのsingle-agent attempt、通常Workflow attempt、通常のArena、direct-folder、既存の通常作成導線は `interactive` を維持する。組み込みWorkflowテンプレートを選んだだけで実行専用にしない。

現在の実行専用作成元はOpenWiki Bootstrap、通常Sync、Formal Integration。この三つを実際の作成境界で明示する。通常SyncをDAGへ変換したり、通常Workspaceの組み合わせ検証を別の正式実行へ作り替えたりしない。

新しい内部Workspaceは、最初から実行専用として保存する。一度Interactiveで公開して後から切り替える競合窓を作らない。owner参照が後段で確定する既存作成経路では、確定前からユーザーの書込を拒否し、ownerの永続的な対応が確認されるまでAgentをdispatchしない。DBとファイル作成を架空の単一transactionと扱わず、途中失敗を既存の準備・cleanup経路で記録する。

公開作成・更新APIに `internal=true` やowner IDを渡すだけで内部実行の権限を得られるようにしない。既存runを参照するだけの偽装要求も拒否する。

### 4.2 非破壊migrationとbackfill

既存WorkspaceのID、Session、AgentRun、リンク、Git branch、未commitファイル、ログ、Auditを維持する。migration前のデータをテストで用意し、追加属性とSQLxの全読取経路・生成型を整合させる。

既存の用途判定は、以下のようなhost管理の永続的な証拠から行う。

- `IntegrationRun.workspace_id` による専用Workspaceの対応。
- Bootstrapのrun scope・trigger・Workspace対応を照合したWorkflow記録。
- 通常Syncの保存済みmaintenance identityや、既存の実行／setup／publication記録から確認できる対応。

通常Syncの最新repository状態は、次のSyncで更新される。最新の `maintenance_workspace_id` だけでは過去の全実行を分類できない。実装時に既存証拠を調べ、復元できる範囲とできない範囲を報告する。今後の実行は最新状態の上書きで帰属を失わないようにする。

次の情報だけでは分類しない。

- `OpenWiki:`、`Integration:` などの名前やbranchの接頭辞。
- Issueリンクやlegacy `task_id` が存在しないこと。
- 最新AgentRunの終了状態、archive、worktreeの欠落。
- `WorkflowSource::System` またはrepository scopeという一点だけ。

証拠がなく用途を確定できない過去データは、勝手に隠したり新しい用途制限をかけたりせず、既存の制約を維持して未分類として報告する。追加の第3の利用契約を必須にはしない。実行専用と確認できる一方でowner同士が矛盾する場合は、閲覧専用を維持して不整合を示す。

既に確定した新しい属性をbackfillで上書きしない。複数回実行・再起動に安全な処理にし、分類件数と根拠不明件数を記録する。SQL migrationで扱えないローカル保存物の調査が必要なら、既存の起動時整合処理へ小さく接続する。各一覧表示で全Auditを再走査しない。

実行中の過去Workspaceを分類しても、その正当なownerの子実行を止めたり、予約を解除したりしない。所有関係を確定できない場合に推測で許可しない。

## 5. 操作契約とサーバー側の防御

UIとバックエンドは同じ利用契約を参照する。UIのdisabled表示は安全境界ではない。バックエンド側で対象Workspaceを解決して、必要な副作用が始まる前に拒否する。dispatch時にも現在状態を再確認し、予約後・キュー待機中の状態変化を取りこぼさない。

| 操作                                                                                      | 実行専用Workspaceでの契約                                                          |
| ----------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| 詳細、Session一覧、会話履歴、Native Audit／既存ログの閲覧                                 | 許可。新しいSessionやAgentRunは作らない                                            |
| ファイル、差分、Wiki本文・リンク・再読込                                                  | 許可。既存のpath／repository境界と安全な描画を維持                                 |
| 任意のチャット、steer、follow-up、新Session、通常review依頼                               | 拒否                                                                               |
| 任意のGoal新規作成・目的変更・resume、queue追加                                           | 拒否。所有者が既に起動した実行の正規な継続と区別                                   |
| 対話terminal、editor起動、dev server、任意script、添付書込                                | Workspaceへの開発操作としては拒否                                                  |
| 手動commit／merge／rebase／reset／branch変更／PR、Repo・Issueリンク変更                   | 拒否。ownerによる既存publicationは別経路で維持                                     |
| 通常Workflow／Arenaへの既存Workspaceとしての再バインド、他の正式Integrationの採用元にする | 拒否。正当なowner配下の既存内部利用は維持                                          |
| Workspace名・archive等の一般更新、通常Workspace削除API                                    | 今回の閲覧専用導線からは許可しない。既存のowner cleanupは別扱い                    |
| 既読記録、ブラウザー内の表示設定                                                          | 許可可能。実行状態や利用契約を変更しない                                           |
| 実行の停止、承認／入力応答、既存の復旧                                                    | 所有者が現在の状態で許す操作だけを許可。汎用チャットや任意のnode再実行で代用しない |

各機能が現在提供していない操作を新設する要求ではない。たとえばBootstrapのnode retryが禁止なら、そのまま禁止する。承認や入力応答も、既存の待機要求への回答と自由な追加作業を区別する。

最低限、次の経路を調査し、共通policyの呼出位置と回帰テストを実装記録へ残す。

1. Workspace／Session／AgentRunの公開作成、更新、follow-up、review、steer、retry、Goal、queue。
2. 実dispatch、setup gate、queued follow-up、Goalの継続、legacy ExecutionProcess／script起動、終了後の次処理。
3. terminal、editor、Repo／Issueリンク、手動Git操作、PR、archive／削除、Workspaceの再利用を受け付けるWorkflow／Arena／Integration。
4. 正当なownerによるSession作成、child dispatch、script検証、停止、復元、publication。

「内部呼出だからすべて許可」という無条件bypassは作らない。既存の永続的な実行対応を検証できる小さな内部境界を使う。一方、低レベルのSession作成を一律禁止して正当なWorkflow実行を壊す設計も避ける。

拒否時は、理由と実行詳細への参照を返す。拒否する要求がSession／AgentRun／script／queue項目を先に作成したり、worktreeを準備したりしないことをテストする。APIエラーやcapabilityの型は既存形式へ合わせ、生成TypeScriptを直接編集しない。

これは協調的なローカル実行におけるEVKの操作契約である。外部shell、同じOSユーザーの別プロセス、danger-full-accessのAgentまで完全に隔離する保証はしない。別ユーザー／container／ACLによる新しいセキュリティ基盤は導入しない。

## 6. 所有する実行の制御を維持する

実行専用Workspaceの自由編集を止めても、ユーザーが内部実行を止められなくなってはいけない。

- UIからのStopは、Bootstrap全体、通常Sync、Formal Integrationそれぞれの正規の停止・取消経路に接続する。childだけを停止してownerが後続phaseを開始し続ける状態にしない。
- 汎用AgentRun操作への直リンクでもownerの制御を迂回できないようにする。既存の監査付きCancel、command identity、idempotency、process終了確認を維持する。
- 取消受理と取消完了を区別する。publication済みのGit反映を取り消したと表示しない。既存の不可逆境界に従う。
- 履歴投影がdegradedでも、既存で許可されている緊急停止を利用契約だけで無効化しない。不整合時に未監査のkillや強制lease解放で解決しない。
- 所有者の停止・復旧経路が足りない場合は、本機能に必要な最小接続を加える。新しい汎用retry/resume基盤は作らない。
- 正当な実行に含まれる承認／入力待ちを表示し、対応する既存操作に到達できるようにする。権限やphaseを変更する自由なチャット欄は出さない。

## 7. 通常一覧と実行・履歴のUI

### 7.1 発見可能性

既存のWorkspace画面・Sidebarに、次の二つを区別できる入口を設ける。表記やタブ／フィルターの配置は既存デザインへ合わせてよい。

- **通常作業**: Interactiveだけを既定表示。既存の検索、Project絞込、pin、archive切替を維持。
- **実行・履歴**: Execution-onlyを明示的に表示。実行中／要対応と、終了した実行の履歴を区別し、詳細を開ける。

実行専用を通常一覧へ混ぜ直して可視化するのではなく、必要なときに閲覧できる別の表示範囲とする。実行中・承認待ち・失敗の件数や状態を入口から認識できるようにし、隠したために対応が必要な実行を見失わない。既存通知・既読機構を利用し、新しい通知サービスは作らない。

OpenWiki設定、Integrationパネル、WorkflowのCanvas／Dashboard、既存のWorkspace／Session直リンクからも、同じ閲覧専用表示へ到達できる。人間が操作して開始した内部実行も実行専用である。

絞込、検索、件数、並び順、再接続、archive表示で利用契約を一貫させる。古いレスポンスや別hostのキャッシュによってチャットを一瞬有効にしない。状態未取得時は操作を保留し、取得失敗をInteractiveと解釈しない。

API、stream、summary、UIのどこで絞るかは既存consumerを確認して決める。汎用の全Workspace取得を無断で通常作業専用に変えない。必要なら明示的なscopeを追加し、通常作業の表示と、実行一覧／ID指定取得の役割を分ける。既存の全件取得を踏襲する場合は限界を記録し、汎用ページング基盤の追加を今回の必須条件にしない。

### 7.2 閲覧専用の詳細

既存のWorkspace詳細、Canonical timeline、Workflow表示、ファイルtree、diff、Markdown／Mermaid、Wiki Viewerを再利用する。別の会話／ファイル表示エンジンを作らない。

- `実行専用・閲覧のみ` と、その意味を表示する。チャット欄、新Session、通常開発用メニュー、ショートカット等を出さないか理由付きで無効にする。
- ownerの種類・実行ID、Workspace、repository、branch、現在状態を確認できるようにする。未保存情報を推測で埋めない。
- 複数Session／child AgentRunの履歴を見分けられるよう既存表示を維持する。
- Wikiは現在の生成worktreeか公開先かを区別し、未公開生成物を公開済みと表示しない。新しいBase branch Viewerは本仕様の必須ではない。
- 完了時に画面が突然閉じたり、通常Workspaceへ選択が移ったりしない。閲覧中の詳細を保持し、一覧の分類だけを更新する。
- ブラウザーの再読込、直接URL、戻る操作、別Workspace／hostへ切替えても契約が維持される。通常Workspaceへ戻ったときに、その通常チャットの入力や選択Sessionを壊さない。

### 7.3 閲覧による実行副作用の禁止

実行専用Workspaceの閲覧時は、保存されたパスと既存ファイルを解決する。ファイル／Wiki／diff／summaryの取得や画面mountによって、以下を行わない。

- worktreeやcontainerの再作成、削除済みフラグの解除、branchの作成・変更。
- setup／cleanup／archive scriptやdev serverの実行。
- Session／AgentRunの作成、Goal／queueの再開、再publication。

現在の `ensure_container_exists` 呼出を無条件に流用しない。必要なら既存のパス解決へ非作成モードを加え、通常開発用の準備契約を壊さずに分離する。パス未準備、既存cleanupによるファイル欠落、読取不能はそのまま表示し、保存済みの実行履歴は可能な範囲で残す。別branchからの復元を「元の成果物」として見せない。

path traversal、symlink、repository membership、rawファイルの安全な配信、host scopeを維持する。閲覧のために任意パスへの新しいAPIや権限緩和を追加しない。既読記録など、明示された軽量な表示用metadataの更新は許容する。

## 8. 完了済み実行の履歴化とcleanup

履歴化は表示上の分類であり、`archived=true` への自動更新でも、worktreeやDBの削除でもない。実行専用の終了判定はownerの状態を使い、最後のAgentRunの成功だけでは決めない。

- Bootstrap: Generator終了後もReview／Refine／Publishが残る。Workflowと既存publicationの結果を区別する。
- Sync: Agent終了とcompletion proof／publication／receiptの結果を区別する。
- Formal Integration: 実装・検証、Git反映、Card後処理、Wiki Syncへの接続を区別する。後続Wiki Syncの完了まで自動で保証した表示にしない。
- 失敗、取消、後処理中、復旧待ち、結果不明を成功へ丸めない。所有する実行に終端状態がない場合、消えたプロセスや古い時刻だけで成功にしない。

過去の実行状態に最新repositoryの成功結果を転用しない。既存run記録やrun別publication証拠を使い、不足する通常Syncの終端結果は既存のrun情報等への小さな永続化で補ってよい。過去に記録されていない結果は不明と表示する。新しい汎用実行履歴DBへ全イベントを複製しない。

本機能でarchive scriptを呼んだり、保持期間を短縮したり、完了直後の自動削除を追加したりしない。既存のownerによる一時設定の復元、予約解放、cleanup、publication復旧は維持する。実行・承認待ち・終了確認待ち・publication復旧中の資源を、表示上の履歴化を理由に回収しない。

既存の期限／orphan cleanupと衝突する場合は、owner・AgentRun・process状態の既存検査を再利用して必要な保護を追加する。新しいGC基盤は作らない。既存ポリシーで物理ファイルが消えた後の無期限閲覧は保証せず、第7.3節の欠落表示と履歴閲覧を維持する。

通常Workspaceの明示archive／削除は維持する。実行専用の新しい管理削除UI、成果物の移植、保存期限設定は将来課題とする。本実装や移行中に過去Workspace・監査ログを整理目的で削除しない。

## 9. 既存consumerへの影響

利用契約が必要なのはSidebarだけではない。少なくとも以下を確認する。

- 最近のWorkspaceからの新規作成既定値、候補選択、通常Workflowの既存Workspace選択。
- Project／Cardの関連Workspace、Formal Integrationの採用候補、並列コンテキストのpeer活動一覧。
- globalなcommand／shortcut、Session選択、保存したchat draft、URL直アクセス、プレビューからの復帰。
- root／childのAgentRun操作、script実行、queue、終了後の自動処理。
- host-scoped APIと共有フロントエンドのキャッシュ、生成型、既存ストリームconsumer。

実行専用を「最近使った通常作業」として選んだり、peerの通常開発成果と混同したりしない。一方、ownerが必要とする実行情報や、既存の明示的な読取参照まで全件フィルターで消さない。

Interactiveの通常Workflow／Arenaは従来どおり利用できる。実行方式、template source、起動者が人か自動か、カードの有無だけで利用契約を変えない。

## 10. 実装方針と非目標

推奨する最小順序:

1. 作成元・操作経路・cleanup・一覧consumerを対応表にし、利用契約とowner参照の格納先を決める。
2. Workspaceモデル、非破壊migration／backfill、API・生成型・共通policyを実装する。
3. 三つの内部作成元、正当なowner dispatch、公開操作の拒否、停止・復旧を接続する。
4. 通常一覧と実行・履歴、閲覧専用詳細、非作成のファイル読取、他consumerを更新する。
5. 自動テスト、差分レビュー、MCP確認、確認済み関連不具合の修正、運用文書更新を行う。

一般化するのは利用契約と共通の操作境界であり、Wiki・Integration・Workflowの状態機械ではない。以下は非目標。

- 新しいscheduler、DAG、汎用artifact／history store、共有会話DB、権限プラグイン基盤。
- OpenWikiのfork／patch、生成promptやWiki品質の再設計、旧 `.llm-wiki` の復活。
- 全WorkspaceへのOSレベルsandbox追加やユーザー権限・認証の変更。
- 自動的なarchive／削除、専用環境から通常開発用への変換、成果物救出ツール。
- 通常Workflowを別の実行モデルへ変更すること、全API／全画面の無関係な再設計。

## 11. 自動テストとquality gates

本番の作成・dispatch・policy・読取接続点を通すテストを優先する。文字列snapshotやUIのdisabled確認だけではバックエンド契約を証明しない。実モデル呼出は自動テストに必須としない。

### 11.1 モデル・migration・作成

- 通常作成、通常Workflow／System由来template、Arena、direct-folder、複数RepoはInteractiveを維持。
- Bootstrap／Sync／Formal Integrationは作成時からExecution-only。準備失敗にも一時的な書込許可窓がない。
- 完了・取消・再起動・lease解放後も属性とowner対応が残る。
- 旧データbackfillの確実な対応、名前だけ似た通常Workspace、証拠欠落、owner矛盾、繰返し実行。
- 移行後もSession、Audit、リンク、ユーザー変更、既存archive／pin情報を失わない。

### 11.2 操作とowner

- 第5節で実在する各書込経路を直接呼び、拒否と副作用なしを確認する。特にSession作成、follow-up、Goal／queue、terminal、script、Git操作、Workspace再バインド。
- owner IDやinternalフラグの偽装、別Workspace／Session／runの指定、終端owner、所有者不明のdispatchを拒否。
- 正当なBootstrapのPASS／REFINE、通常Syncの更新／no-op、Integrationの検証／反映／後処理が進む。ReviewerのReadOnlyとSession分離を維持。
- ownerの正規Stop、承認／入力対応が利用可能。取消とpublicationの競合、projection degraded時の停止を既存契約に沿って検証。
- 新規queue拒否と、既存owner配下の正規な継続を区別する。旧queueや終了callbackが自由な追加開発を再開しない。
- 採用元Workspaceの予約解除後は通常開発を再開できる。実行専用自身は再開できない。

### 11.3 閲覧・一覧・履歴

- 通常／実行・履歴の絞込、検索、件数、archive、再接続、直URL、host切替。
- 読取専用詳細の全入口でチャット／新Session／編集導線が使えない。読み込み中にも有効化しない。
- ファイル・Wiki・diff取得や画面mountでworktree／Session作成、script、unarchive、publicationが起きない。
- 物理ファイルがない履歴、未準備、読取エラー、複数Repo、path traversal／symlink境界。
- 実行中から完了への表示遷移で詳細や選択を失わない。古いSyncと新しいSyncの状態を混同しない。
- 新規作成の既定値や通常peer一覧へ実行専用を混入させない。
- 期限cleanupがactive owner／終了未確認process／publication復旧を壊さず、履歴化自体ではarchive／削除しない。

### 11.4 実行する検証

- `pnpm run format`
- 関連Rust unit／integration tests、関連Vitest、必要な範囲の既存Workflow／Arena／OpenWiki／Integrationテスト。
- Rust型変更時の `pnpm run generate-types` と `pnpm run generate-types:check`。必要なSQLx metadataは既存生成手順を使う。
- `pnpm run check`、`pnpm run lint`、変更範囲に必要な開発用コンパイル。

generated filesを手編集しない。通常検証の対象外であるprivate billing依存付きの独立 `crates/remote` Cargo検証は要求しない。remote-webとremote Rust formatは維持し、影響する未検証範囲を明記する。

## 12. Chrome DevTools MCPによる受入確認

修正版EVKをサンドボックス外から `localhost:4020` で利用できるようにし、Chrome DevTools MCPから通常UIを操作する。MCPの現在の接続、Chrome、起動中サーバーの実行コードを確認する。過去の接続実績を今回の証拠にしない。

### 12.1 対象と検証規模

既存の実行履歴・生成済みWikiを閲覧確認に利用できる。ユーザーの実行中runを停止しない。既存データの削除・用途の手動書換で成功を作らない。

正当なownerの新規作成と実行制御も確認するため、**少なくとも一つの新しい実行専用実行を、修正版の通常UIから開始する**。小さな隔離テストrepositoryを優先し、OpenWikiまたはFormal Integrationの既存導線を使う。三つの作成元すべての契約は自動テストで検証するが、実機で全経路の大規模再生成を繰り返すことは要求しない。

この仕様を参照する実装Goalでは、隔離対象に限り、既存認証済みCodex／OpenWikiの必要最小限の実モデル呼出、試験source commit、既存機能による統合・Wiki publicationを許可する。別API-key経路は追加しない。Wikiの品質比較やEVK全体のWiki再生成は今回の達成条件ではない。

### 12.2 確認項目

1. 通常一覧がInteractive中心になり、明示的に実行・履歴を開くと内部環境が見える。Project／Cardの通常Workspaceは利用可能なまま。
2. Bootstrap／Sync／Integrationの利用可能な既存履歴を、元の機能画面と直URLから開く。対応するownerと状態を確認する。未存在の種類は自動テスト結果と区別する。
3. 実行専用詳細で会話／childのログ、ファイル、diff、Wikiを読み、内部リンクと再読込を確認する。表示元と未公開状態を識別する。
4. チャット、新Session、Goal resume、terminal、通常merge等の開発操作が利用できず、通常Workspaceへ戻ると従来の操作が利用可能であることを確認する。危険な操作の直接API拒否は隔離対象の自動テストで補完する。
5. 新しい実行が作成時から実行専用として現れ、通常一覧を汚さず、正当なownerが進行する。完了まで確認し、ownerの結果と履歴表示が一致する。開いていた画面が勝手に閉じない。
6. 停止確認用に所有する別の小さな試験実行を開始し、UIから正規Stopを行い、停止要求と最終結果、cleanup／予約解放を確認する。停止できないpublication境界を試験都合で改変しない。
7. 再読込、別Workspaceへの往復、必要な再起動後にも用途と表示が維持される。通常Workspaceのchat draftやSession選択を壊さない。
8. 欠落worktreeの閲覧が非作成であることは自動テストで必須確認する。実機に自然に該当履歴があれば確認し、試験のためにユーザーのファイルを削除しない。

新しい試験対象は衝突しない隔離repositoryまたは `test/workspace-usage-*` branchとし、publication先も試験用であることを確認する。開発branchやmainへ試験成果を反映しない。初期準備や途中失敗時も履歴を残す。

実行revision／未commit差分、対象source SHA、Workspace／Session／owner／AgentRun ID、開始・終了時刻、MCP操作記録またはスクリーンショット、必要なAuditと最終結果を記録する。秘密情報を含めない。UIだけの確認と、実モデル・publicationまで確認した経路を区別する。

## 13. 関連不具合の修正と検証のやり直し

今回の回帰と、本仕様の作成・操作制限・所有者実行・閲覧・履歴・cleanupに直接関係する確認済み既存不具合は、原因調査、修正、回帰テスト、MCPでの元操作再確認までを対象にする。

同じmoduleにあるだけでは関連としない。症状、証拠、根本原因、本仕様との因果関係、修正、検証を短い実装記録へ残す。疑いだけで変更しない。無関係な問題は記録に留める。

可能なら修正前に失敗する自動テストを追加する。実dispatch、owner、停止、publicationへ影響する修正後は、新しい試験実行で該当する経路を再確認する。表示だけの変更は既存成果物で確認してよいが、再利用できる理由を記録する。異なるコードrevisionの成功を混ぜて、最終版がすべて通ったと報告しない。

MCP試験をAPI-onlyや手順書に読み替えない。外部要件の不足と実装不具合を区別し、修正可能な関連不具合を未解決のまま達成扱いしない。

## 14. 受入条件

| ID   | 達成条件                                                                                                                                    |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| AC01 | 利用契約が永続化され、表示設定・物理的所有権・一時予約と分離されている。未来のownerにも同じ共通policyを適用できる。                         |
| AC02 | 三つの内部作成元が初めから実行専用になる。通常Workflow／Arena／direct-folder／通常作成は従来の利用を維持する。                              |
| AC03 | 根拠に基づく非破壊backfillがあり、既知の内部履歴を分類できる。根拠不明・不整合は明示され、名前等による誤分類や履歴削除がない。              |
| AC04 | 通常一覧と実行・履歴を区別でき、実行中・要対応を見失わない。直URLや元機能のリンクから閲覧できる。                                           |
| AC05 | 実行専用で自由な追加開発がUI／API／実dispatchから拒否される。拒否前の副作用やowner偽装bypassがない。                                        |
| AC06 | 正当なownerのphase・書込・停止・承認／入力・復旧・publicationが維持される。終了やlease解放で自由編集に変わらない。                          |
| AC07 | 閲覧でworktree再作成、script、Session／AgentRun作成、unarchiveが起きない。欠落資料は正直に表示し、既存path境界を維持する。                  |
| AC08 | 履歴の状態はownerの結果に対応する。Agent成功とpublication成功を混同せず、後続Syncの状態を古い履歴へ転用しない。履歴化による自動削除がない。 |
| AC09 | 新規作成既定値、Card候補、peer一覧、通常Workflowの再利用などが実行専用環境に汚染されず、通常開発の操作・draftを維持する。                   |
| AC10 | 関連自動テスト、format、生成型整合性、check、lint、必要な開発用コンパイルが成功し、差分レビューのconfirmed findingsを修正済み。             |
| AC11 | 第12節のMCP実機確認が修正版で完了し、新しいowner実行の進行・完了、別試験実行の正規Stop、閲覧・内部遷移・再読込の証拠がある。                |
| AC12 | 確認済みの関連不具合を修正・再確認し、変更点、用途の意味、操作方法、移行結果、残る制限、試験資産を文書化している。                          |

受入条件と実行したテストを対応付ける。現行実装の同名機能が存在するだけ、過去runが成功しただけでは今回の検証成功としない。

## 15. 作業・停止条件と最終報告

既存ユーザー変更、Wiki、DB、履歴を保護する。実装変更の自動commit／stash、既存データ整理、main／開発branchへの試験反映、release build、publish、push、PR作成、GitHub Actions実行は行わない。第12節の隔離試験だけが試験commit／publicationの例外であり、試験資産は確認用に残す。

通常の実装判断は自律的に行い、目的・安全性を維持する最小の適応を実装記録に残す。段階ごとに進捗、検証結果、残課題を報告する。一度の失敗では止まらず、修正可能な問題を診断して続ける。

全受入条件を検証できた場合だけ達成とする。安全な調査・代替手段を尽くしても、MCP未接続、認証不足、利用枠、外部依存、追加権限等により必須検証ができない場合は、可能な実装・ローカル検証を完了し、**ブロック／受入未完了**として報告する。未修正の不具合を環境制約と呼ばない。破壊的な移行や新しい重大な権限変更が必要なら、勝手に範囲を広げず最小の判断点を示す。

最終報告には以下を含める。

- 実装した利用契約、owner対応、共通policy、主要変更ファイル。
- 移行・backfillの結果と根拠不明データ、通常Workflow等への互換性。
- 通常一覧、実行・履歴、閲覧専用詳細の使い方。ReadOnlyがOS権限の意味ではないこと。
- サーバー側で防御した操作と、所有者が引き続き行える操作。
- 副作用のない読取、完了判定、cleanupとの関係と保持の限界。
- 自動テスト・quality gatesのコマンドと結果、最終コードで有効なMCP証拠。
- 関連不具合の再現、原因、修正、再確認。無関係・未確定の事項は区別。
- 残した試験対象、ユーザーデータ保護、制限、未完了なら最小の再開手順。

Goalの進め方は、単一目的・検証可能な終了条件・checkpointを定める [OpenAIのGoalガイド](https://learn.chatgpt.com/use-cases/follow-goals) を参考にする。製品の具体的な契約と検証範囲は本書を正とする。
