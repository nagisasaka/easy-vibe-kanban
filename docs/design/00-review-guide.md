---
title: "EVK 並列開発・自動統合仕様案 — レビューガイド"
description: "コード照合の基準点、既存機構の再利用範囲、未決定事項を記録する。"
---

# EVK 並列開発・自動統合仕様案 — レビューガイド

作成・改訂日: 2026-09-17 / 文書版: 0.3 / 用途: コード照合レビュー反映済みの実装前仕様

> 初稿の作成者はソース取得に失敗し、READMEだけを確認していた。v0.3では別途行ったローカルコード照合の結果を反映した。ただし、確認したmainと未commitの開発変更を区別する。本機能の実装・実行試験が完了したという意味ではない。基準SHA、最新性の確認時点、調査の限界は第2節に記録する。

## 1. ファイルと読み方

| ファイル                                | 内容                                                                                                        |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `01-parallel-context-bootstrap.spec.md` | 入口: Wikiと未統合のChange Manifestを参照するブートストラップ                                               |
| `02-batch-auto-merge.spec.md`           | Cross-workspace ReadOnly統合と、出口: 複数カードの明示選択による正式Integration・検証・ローカルブランチ反映 |
| 本書                                    | 証拠の範囲、共通の設計原則、コード確認箇所、レビュー出力形式                                                |

レビュー担当は本書を先に読み、実際のmainと開発中ブランチを区別して確認する。本案に合わせるためだけの既存構造の全面変更をしない。既存実装で満たせる要求は再利用し、矛盾する記述は具体的なコード根拠とともに修正提案する。

## 2. 証拠の扱い

本文の分類は次のとおり。要求IDはレビュー指摘とテストを対応付けるための識別子であり、既存コード上の識別子ではない。

| 分類               | 意味                                                                      |
| ------------------ | ------------------------------------------------------------------------- |
| **合意**           | この会話でユーザーが支持した目的・UX・責任分界                            |
| **提案**           | 合意を実装可能にするために本案で補った設計。コードレビューで変更可能      |
| **確認済みREADME** | 公開READMEに記載があった事項。ソース実装の確認を意味しない                |
| **コード確認済み** | 第2.1節のmainまたは作業ツリーを静的に確認した事実。実行成功の証明ではない |
| **未検証**         | 現行コード・実行環境で確認が必要。存在・不存在を断定しない                |
| **要判断**         | 対応範囲・保証を選ぶ必要がある事項。縮小案をユーザー合意として扱わない    |

### 2.1 調査基準点と取得結果

対象: [nagisasaka/easy-vibe-kanban](https://github.com/nagisasaka/easy-vibe-kanban)

初稿作成時の取得結果（v0.2の来歴）:

| 確認対象                                            | 結果                                            |
| --------------------------------------------------- | ----------------------------------------------- |
| 公開トップページ・README                            | 閲覧できた。ブランチ表示はmain                  |
| `git clone --depth 1 --branch main --single-branch` | `Could not resolve host: github.com` により失敗 |
| ソース本文・アーカイブの代替取得                    | 取得できなかった                                |
| 最新mainのSHA・コミット日時                         | 未確定                                          |
| Rust/TypeScriptコード、マイグレーション、テスト     | 未読・未実行                                    |
| 実装担当の未マージブランチ                          | 未確認                                          |

クローン失敗はコードが存在しないことや非公開であることの証拠ではない。公開READMEも、最新mainと完全同期した実装保証としては扱わない。

v0.3のコード照合結果:

| 確認対象                              | 結果                                                                                                                                                    |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ローカルmain / origin/main / 作業HEAD | `9ad6b9ccf7ab535e8d416eb12ed767619d9197ed`                                                                                                              |
| コミット日時                          | `2026-09-17T04:04:49+09:00`                                                                                                                             |
| リモートmainの最新性                  | HTTPSの`git ls-remote`で同SHAを確認。最終確認は2026-09-17 20:29 JST。この時点以後の最新性は保証しない                                                   |
| 作業ブランチ                          | `feat/llm-wiki-updates-flow`。mainとのcommit差分は0/0                                                                                                   |
| 作業中の差分                          | 既存のOpenWiki更新一本化等の未commit変更あり。レビュー開始時は追跡済み51ファイル、追加1,203行／削除2,178行。未追跡ファイルもあり、今回の3仕様書も未追跡 |
| 調査方法                              | コード・モデル・SQL・既存テスト定義の静的確認。checkout/reset/stashや実装変更はしていない                                                               |
| 実行検証                              | 新機能のテスト、Agent実行、MCP試験、Git反映の実機検証は未実施                                                                                           |

以下、`M`は上記main、`W`はレビュー時の未commit変更を含む作業ツリーを指す。特に、旧LLM WikiプリセットはMに存在するがWでは廃止中である。実装開始時に差分を再確認し、WのOpenWiki一本化と統合するか、そのcommit済み後継版へ合わせる。旧経路を復活させて本案を実現しない。

### 2.2 READMEから得られた既存仕様の接点

以下はすべて[README「Card context and shared local files」および「Architecture」](https://github.com/nagisasaka/easy-vibe-kanban#readme)の記述に基づく。

| ID  | READMEで確認できたこと                                                                                                      |
| --- | --------------------------------------------------------------------------------------------------------------------------- |
| R1  | 新規カードにはLLM WikiとShared directoriesのコンテキストプリセットがある                                                    |
| R2  | 保存済み指示は維持され、カードの初回Workspaceリクエストに含まれる。毎メッセージへの注入や既存セッションへの遡及適用ではない |
| R3  | 管理Git Workspaceには `.evk-shared/persistent/` と `.evk-shared/cache/` のGit除外リンクがある                               |
| R4  | 同じ登録リポジトリの共有領域を参照する。プリセット無効化は共有ファイル削除やアクセス権剥奪ではない                          |
| R5  | Direct-folder Workspaceにはこのリンクがない。共有内容の並行書き込み保護も記載上はない                                       |
| R6  | PR・mergeの既存導線があり、Rustバックエンド／React+TypeScript構成。共有型はRustから生成される                               |

上表は初稿のREADME調査記録であり、現在の規範ではない。R1はWの旧Wiki廃止により変更され、既定Card contextはShared directoriesのみとなる。R2のカード本文と、実行ごとに更新されるRepository Memoryのidentity指示は別経路。R5の一般共有フォルダと、immutable eventのatomic publication／lockを備えるRepository Memoryも区別する。

### 2.3 コード照合で確認した再利用点と不足

パス・行はWのレビュー時点を示す。以後の変更では行番号でなくシンボルと呼出経路を追い直す。

| 領域            | 確認済みの事実と仕様への反映                                                                                                                                       | コード根拠                                                                                                                                                                                                                                                                                                                                                |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Card context    | マーカー付きの固定文面をIssue descriptionへ合成。設定OFFはACLでもMemory停止でもない                                                                                | [cardContext.ts](../../packages/web-core/src/features/pipeline/model/cardContext.ts#L46) `withCardContext` / `replaceCardDescription` / `defaultCardContext`                                                                                                                                                                                              |
| 初回と新Session | 初回はtitle/descriptionからpromptを作る。独立新Sessionは渡されたpromptで開始し、カード指示の自動継承はない                                                         | [workspaceCreateState.ts](../../packages/web-core/src/shared/lib/workspaceCreateState.ts#L27) `buildWorkspaceCreatePrompt`、[useCreateSession.ts](../../packages/web-core/src/features/workspace-chat/model/hooks/useCreateSession.ts#L38)                                                                                                                |
| 継続時のMemory  | Wではfollow-up等にも現在runのidentity・draft先を渡す。並列履歴の再注入禁止をこの更新禁止へ広げない                                                                 | [agent_run_port.rs](../../crates/local-deployment/src/agent_run_port.rs#L724) `execution_env`、[codex/client.rs](../../crates/executors/src/executors/codex/client.rs#L270) `thread_resume`                                                                                                                                                               |
| 保存とauthority | 既存共有領域の`persistent/knowledge`にevents/integrations/receipts等。immutable event、atomic publication、短いlockがある。peer Workspace Memoryは公開対象ではない | [utils/repository_memory.rs](../../crates/utils/src/repository_memory.rs#L77) `ChangeManifest` / `RepositoryMemoryStore` / `publish_limited`                                                                                                                                                                                                              |
| Manifest生成    | 成功した通常coding runの終了処理でsourceを確定しdraftをevent化。中断・draft欠損・Memory無効時の完全な進捗台帳ではない                                              | [services/repository_memory.rs](../../crates/services/src/services/repository_memory.rs#L152) `complete_coding_run_inner` / `finish_source_publication`                                                                                                                                                                                                   |
| CardとWorkspace | 現代のボードはIssue。Workspaceはcardなしでも作れる。Workspace:Sessionは1:N、Workspace:Repoも複数。legacy `task_id`をIssue IDとみなさない                           | [workspace.rs](../../crates/db/src/models/workspace.rs#L62)、[session.rs](../../crates/db/src/models/session.rs#L23)、[workspace_repo.rs](../../crates/db/src/models/workspace_repo.rs#L12)、[create.rs](../../crates/server/src/routes/workspaces/create.rs#L32) `create_workspace_record`                                                               |
| リンクとDone    | `insert_workspace_link`はworkspace_id単位のupsert。統合Workspaceを全Cardへリンクすると前の関連を上書きする。既存Issue更新は条件付き完了CASではない                 | [local_remote.rs](../../crates/server/src/routes/local_remote.rs#L2072) `insert_workspace_link` / `update_local_issue`                                                                                                                                                                                                                                    |
| 手動merge       | source確定、squash、新commit、source ref更新、Merge記録、非同期remote Done、archiveという副作用がある。検証済みRの公開にそのまま使わない                           | [workspaces/git.rs](../../crates/server/src/routes/workspaces/git.rs#L179) `merge_workspace`、[git/lib.rs](../../crates/git/src/lib.rs#L678) `merge_changes`                                                                                                                                                                                              |
| Git低レベル     | common-dir取得等は再利用可能。ただし既存`update_ref`にはexpected-old引数がなく、exact-R反映＋checked-out worktree整合は追加契約                                    | [git/lib.rs](../../crates/git/src/lib.rs#L185) `get_common_dir`、[git/cli.rs](../../crates/git/src/cli.rs#L739) `update_ref`                                                                                                                                                                                                                              |
| Orchestration   | persist-before-dispatch、outbox/inbox、idempotencyを再利用できる。product kindはWorkflow/Arena。dispatcher leaseは再起動時に削除され、長時間の業務予約ではない     | [services/orchestration.rs](../../crates/services/src/services/orchestration.rs#L87) `start_run`、[runtime/orchestration.rs](../../crates/executors/src/runtime/orchestration.rs#L27)、[db/orchestration.rs](../../crates/db/src/models/orchestration.rs#L1389) `reconcile_startup`                                                                       |
| 終了・検証      | 通常runの成功処理はMemory source commitとqueued follow-upへ進む。統合のR固定や試験専用実行とは目的を区別する。Script実行、exit code、前後HEAD記録は再利用可能      | [container.rs](../../crates/local-deployment/src/container.rs#L349) `handle_agent_run_terminal`、[actions/script.rs](../../crates/executors/src/actions/script.rs#L31) `ScriptRequest`、[execution_process_repo_state.rs](../../crates/db/src/models/execution_process_repo_state.rs#L8)                                                                  |
| Sync接続        | 既存pending event→自動Syncを維持する。`prepare_integration`は単一Workspace用で、固定したbatch source集合の照合をそのまま満たさない                                 | [repository_memory.rs](../../crates/services/src/services/repository_memory.rs#L324) `prepare_integration` / `pending_events`、[routes/openwiki.rs](../../crates/server/src/routes/openwiki.rs#L349) `recover_repository`                                                                                                                                 |
| 権限・cleanup   | danger-full-accessや共有writable rootsはOS上のReadOnly保証ではない。cleanupはIntegration予約をまだ知らない。peer読取でworktree再作成を誘発しない                   | [codex.rs](../../crates/executors/src/executors/codex.rs#L975)、[shared_resources.rs](../../crates/workspace-manager/src/shared_resources.rs#L139)、[workspaces/files.rs](../../crates/server/src/routes/workspaces/files.rs#L295) `resolve_workspace_repo_root`、[workspace.rs](../../crates/db/src/models/workspace.rs#L326) `find_expired_for_cleanup` |

この表は「機構が存在する」ことと「本案の保証が既に実装済み」を区別するためのもの。特にexact-R publication、複数Cardの条件付きDone、業務予約、batch event選択は追加が必要である。

## 3. 二つの仕様に共通する設計原則

**合意 C01 — EVKは事実・アクセス手段・実行整合性を提供し、意味解釈はエージェントに委ねる。**

依存関係、競合の意味、実装意図の両立可否をEVK独自の永続的意味モデルとして再実装しない。Wiki、既存Manifest、Git・カード・Workspaceの機械的情報、用途の説明を用意し、Codexが必要な文脈を読む。

**合意 C02 — 開発単位と認知の共有単位を分離する。**

目的ごとにカード／Workspaceを分け、サーバー機能同士も並列化する。他Workspaceを認識できることは、そのコードが自分のworktreeに存在すること、あるいは他Workspaceを編集してよいことを意味しない。

**合意 C03 — 初版のAuto Mergeはボード上の一括操作。**

複数カードと採用Workspaceを明示選択し、１回の統合実行にまとめる。Auto Merge列へのドラッグを発火条件にしない。成功後に対象カードをDoneへ移す。Doneへの他の経路は維持する。

**合意 C04 — 選択集合は黙って変えない。**

選択された成果物をまとめて統合する。未選択成果の追加、選択成果の部分的な除外、仕様の勝手な変更はしない。統合すべきでない場合は理由を示して保留する。

**合意 C05 — 反映前キャンセルと反映後の撤回を分離する。**

Done→In progressは作業再開でありGitのUndoではない。反映済みの変更撤回は別タスク。ローカル統合はPR不要で成立し、既存の手動PR導線は残す。

**合意 C06 — テストと自然言語の意図を両方使う。**

各Workspaceの検証と統合後の全体検証を行う。ただし全テスト成功を「未検証領域も含めてデグレがない証明」とは表示しない。

**合意 C07 — 他Workspaceは初版ではReadOnlyなsource。**

通常Sessionは他WorkspaceのManifest・diff・確定済みcommitを参照し、現在Workspaceまたは一時統合環境で組み合わせ検証できる。ただし他Workspaceのworktree/source branchを直接変更しない。他Workspace側の変更が必要なら、必要変更を説明して保留する。

**合意 C08 — Auto Merge / Integrateは「マージ能力」ではなく正式昇格ワークフロー。**

通常Sessionでのcross-workspace検証と、Boardから複数成果をtargetへ正式反映してCardをDoneへ進めるIntegrationを区別する。将来のDelegationやMCPはこのReadOnly境界を保った拡張候補であり、初版の必須ではない。

## 4. 用語は論理モデルであり、DB再設計の指示ではない

| 用語            | この文書での意味                                                                       |
| --------------- | -------------------------------------------------------------------------------------- |
| Card            | 現代のボードのIssue。local/remoteの保存・更新経路を区別する。legacy Taskとは別         |
| Workspace       | 採用成果を含む作業環境。複数Repositoryを持ち得る。各WorkspaceRepoにtarget branchがある |
| Session         | エージェントとの作業・会話の単位。Git成果そのものではない                              |
| Manifest        | 既存のChange Manifest。自然言語の報告とGit事実は区別する                               |
| Integration Run | 複数成果を１回の対象固定・検証・反映として扱う論理単位                                 |
| Target          | 今回反映する単一のローカルブランチ。mainに限定しない                                   |

Issue:Workspace=1:N、Workspace:Session=1:Nはコードと整合する。一方、現在のリンク機構ではWorkspaceのリンク先Issueは一つで、付替えが可能。Integration Runと複数Cardの結果対応は固定source集合へ記録し、このリンクを多対多化したり架空Cardを作ったりして代用しない。

## 5. 現行コード照合の順序

最初に実際のレビュー対象を固定する。既存の作業ディレクトリを勝手にcheckout/reset/stashしない。新しいfetchも環境の権限・ネットワーク条件に従い、確認できたリモートmainのSHAと現在作業中のHEADを別々に記録する。最新性を確認できなければ、その旨を明記する。

下表の検索語は**探索の入口**であり、実在するシンボル名・ファイル名を断定していない。

| 優先度 | 確認領域     | 探索語・追うべき経路                                           | 必要な結論                                                              |
| ------ | ------------ | -------------------------------------------------------------- | ----------------------------------------------------------------------- |
| P0     | Card context | `LLM Wiki`, `Shared directories`, `card context`, `evk-shared` | プリセット保存→Workspace作成→prompt組立→executor起動の実際の経路        |
| P0     | Manifest     | `change_manifest`, `change-manifest`, `manifest`, `openwiki`   | writer、reader、保存先、履歴、失敗・中断時、source commitとの紐付け     |
| P0     | Wiki         | `wiki`, `openwiki`, `initialize`                               | 設定ブランチ・参照方法・生成元revision・無効時の処理                    |
| P0     | データ関係   | Card/Issue/Task, Workspace/Attempt, Session                    | 1:N対応、multi-repo、既存状態遷移、Done更新の実体                       |
| P0     | 手動merge    | merge/rebase/target branch                                     | 現行Gitサービス、worktreeの扱い、テストの有無、完了・アーカイブの副作用 |
| P0     | 実行制御     | executor, follow-up, goal, process, cancel                     | 継続実行・自動再起動も含めた書き込みプロセスの停止・予約・再開          |
| P0     | 書き込み境界 | sandbox, permissions, cwd, env, mounts                         | 別worktree以外に元リポジトリ更新を制限できる既存機構があるか            |
| P0     | Git反映      | branch checked out, dirty, update_ref                          | checkout済みターゲットのindex・実ファイルも整合させる方法               |
| P1     | キュー・復旧 | job, workflow, execution, lease, lock                          | 流用可能な実行記録・直列化・再起動復旧。新しいDAGは不要                 |
| P1     | UI           | board toolbar, selection, dialogs, badges                      | 既存一括操作、カードの予約表示、Sessionリンクの再利用                   |
| P1     | 設定・型     | migrations, generated types                                    | スキーマ・設定version・型生成・多言語化・テストの既存慣行               |
| P1     | 削除・保存   | cleanup, archive, prune                                        | 統合中Workspaceや結果記録を既存cleanupが消さないか                      |

最低限の読み取り確認の例:

```sh
git status --short
git rev-parse HEAD
git log -1 --format='%H %cI %s'
git remote -v
git show-ref --verify refs/remotes/origin/main
rg -n -i 'evk-shared|shared directories|card context|change.?manifest|openwiki' . \
  --glob '!pnpm-lock.yaml' --glob '!Cargo.lock'
```

上記は最新リモート取得を保証しない。mainの情報がローカルにない／古い場合は、無理に最新と呼ばない。API名やテーブル名は探索後に記録する。

## 6. レビューで特に見つけてほしい矛盾

1. ブートストラップの大半が既存プリセットで実現済みなのに、別のコンテキスト基盤を作ろうとしていないか。
2. カード保存時の固定指示と実行時の最新メタデータ解決を混同していないか。
3. Manifestを「最新かつ完全で検証済みのGit事実」と誤認していないか。
4. 「別Workspaceだから未取り込み」「Doneだからターゲットへ統合済み」という誤判定をしていないか。
5. 既存manual mergeが直接ターゲット変更・カードDone・cleanupを行う場合、そのまま統合準備に流用していないか。
6. 通常Sessionの終了・workflowの完了・goalの終了が、統合実行の成功やカードDoneと誤って連動しないか。
7. 検証対象コミットと反映コミットが一致するか。Git refsだけ更新してcheckout済みworktreeを壊さないか。
8. target更新とDB更新の間でクラッシュした場合、二重mergeや誤った再実行をしないか。
9. unrestrictedなエージェントに対し、worktree分離だけを安全境界として説明していないか。
10. ワークスペース、共有キャッシュ、DB、テスト用DB・ポートなど、Git以外の共有状態の干渉を見落としていないか。
11. 通常Sessionから他Workspaceと組み合わせ検証するために、source Workspaceを直接編集する設計になっていないか。初版のpeer sourceはReadOnly。
12. Auto Mergeを「workspace間mergeそのもの」と実装し、通常Sessionで再利用できる低レベルGit/Workspace能力と、target反映・Done化の正式ワークフローを密結合していないか。
13. DelegationやMCPを初版の必須条件にして過剰設計していないか。必要なら後付けできるサービス境界だけを検討する。

## 7. レビュー出力形式

最初のレビューでは実装しない。次の順に提出する。

### A. 対象と調査範囲

mainのSHA／コミット日時／取得方法、作業中HEADとの差分、読んだファイル、実行した検証、未確認箇所を記載する。

### B. 要求との対応表

| 要求ID      | 判定                                       | 実際のファイル:行・シンボル | 指摘               | 最小修正案 |
| ----------- | ------------------------------------------ | --------------------------- | ------------------ | ---------- |
| B-xx / A-xx | 実装済み／流用可能／追加必要／矛盾／要判断 | 確認した根拠のみ            | 何が一致・不一致か | 再利用優先 |

### C. 優先度順の指摘

P0=データ損失・安全性・意図しない統合・根本的矛盾、P1=主要UXや互換性、P2=改善。各指摘は「仕様の該当箇所→現行事実→影響→修正案→受入テスト」で説明する。

### D. 最小実装案と改訂仕様

ブートストラップは文章・公開経路の追加だけでどこまで成立するか、Auto Mergeは既存のWorkspace/Session/merge機構をどこまで使えるかを示す。必要なら本案の文書だけを改訂するが、合意C01〜C08を変える場合は明示する。

独立したコンテキストDB、意味的依存グラフ、専用DAG、汎用ジョブ基盤を新規作成する案は、既存機構では不可能な根拠とともに代替案として比較する。先回りして必須にしない。

## 8. Gitの設計根拠

以下はEVKコードの確認結果ではなく、Git自体の動作に関する一次資料である。

- [G1: git-worktree](https://git-scm.com/docs/git-worktree): worktree間で共有されるrefs等の説明。別worktreeは権限隔離ではない。
- [G2: git-merge](https://git-scm.com/docs/git-merge): fast-forward、`--ff-only`、squashの違い。
- [G3: git-update-ref](https://git-scm.com/docs/git-update-ref): expected old OIDによる参照更新。
- [G4: git-revert](https://git-scm.com/docs/git-revert): 反映後の取り消しと、mergeをrevertした場合の後続mergeへの影響。

## 9. この段階で保証していないこと

静的な適合性の確認を実装・試験済みと扱わない。必要改修量、Codexの推論精度、すべてのデグレの検知、任意の外部Git操作との競合安全性は保証しない。機械的な契約は受入テストへ、意味判断はシナリオ評価へ分ける。local/remote Board、multi-repoの完了範囲、協調実行と強隔離の選択は02の第2.2節に残す。対応範囲を狭める案を黙って合意済みにしない。

## 10. 汎用Git動作の補助検証

以下は初稿作成者が `git version 2.47.3` と模擬リポジトリで確認したと記録した結果である。v0.3のコード照合担当は再実行していない。**EVKのソース・テスト・実行環境の検証結果ではない。**

| 確認した挙動                                                | ローカル試験結果                                           |
| ----------------------------------------------------------- | ---------------------------------------------------------- |
| linked worktreeから共有のmain refを更新できる               | 確認。worktreeの分離だけでは直接更新を禁止できない         |
| checkout済みmainのrefだけ更新しても実ファイルは更新されない | 確認。worktree/indexとの不整合が残る                       |
| expected old OIDが違えばupdate-refは失敗する                | 確認。古い基点からの更新検出に使える                       |
| targetが途中まで進んだ後でもff-onlyが成功する場合がある     | 確認。ff-onlyだけでは「targetが元のOIDのまま」を保証しない |

元の記録では試験を模擬repo内に限定している。EVKの追加反映経路が同じ不変条件を満たすかは、受入テストAT-19〜AT-32等で別途確認する。

## 11. v0.3で必須契約へ反映した事項

| 指摘                                                       | 反映先                                |
| ---------------------------------------------------------- | ------------------------------------- |
| 旧Wikiプリセットと新Memory指示の混同、継続identityの欠落   | B-03、B-11〜B-15、BT-17〜BT-18        |
| eventなし活動・破損レコード・歴史的Card対応・Wiki鮮度      | B-08〜B-10、B-20、BT-16、BT-19〜BT-21 |
| 試験専用操作が通常終了処理でcommit／target統合される可能性 | A-06、A-08、A-18、A-27、AT-40〜AT-41  |
| 既存mergeのsquash／source書換え、checked-out target整合    | A-30〜A-31、AT-26〜AT-27、AT-39       |
| dispatcher leaseと業務予約、queued継続、cleanupの混同      | A-14〜A-16、A-39〜A-42、AT-44〜AT-45  |
| 多Cardリンク上書き、Doneの条件付き更新・後処理復旧         | A-10、A-18、A-36、A-40、AT-42〜AT-43  |
| Agent自己報告だけの合格、検証後のR変更                     | A-24〜A-27、AT-40、AT-46              |
| batch Manifest選択と既存自動Wiki Syncの退行                | A-43、AT-47〜AT-48                    |

本改訂は仕様変更のみ。表の受入テストは今後実装・実行する要求であり、通過済みの一覧ではない。
