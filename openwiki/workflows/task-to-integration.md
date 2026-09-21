---
type: guide
title: Issue から実行・レビュー・統合まで
description: 通常作業をまたぐ作成、Setup gate、会話、レビュー、Git/PR、Issue 状態と repository memory の更新順序を説明する。
tags: [workflow, setup, review, git, pull-requests, scheduling]
sources:
  - id: openwiki-source-29ff4fcd8f66b24767de429c
    resource: repo://crates/git-host/src/lib.rs
  - id: openwiki-source-bd15422ef1716eca23408f1f
    resource: repo://crates/git/src/lib.rs
  - id: openwiki-source-209cf31759691c1167d30e1b
    resource: repo://crates/git/tests/git_ops_safety.rs
  - id: openwiki-source-c5905fd27c9b8c0da4a7cf5e
    resource: repo://crates/remote/src/db/issues.rs
  - id: openwiki-source-393b5e3a8041c755c711162c
    resource: repo://crates/remote/src/routes/attachments.rs
  - id: openwiki-source-7943a218d28bcc247061fabd
    resource: repo://crates/remote/src/routes/github_app.rs
  - id: openwiki-source-1afa1d084d6edbf88ae96941
    resource: repo://crates/remote/src/routes/workspaces.rs
  - id: openwiki-source-a13fe4db1eee073d0a7e2c4d
    resource: repo://crates/server/src/main.rs
  - id: openwiki-source-b393e75029374eaf9cba5f7d
    resource: repo://crates/server/src/routes/integrations/runtime.rs
  - id: openwiki-source-391f34050223d13f679b3793
    resource: repo://crates/server/src/routes/parallel_context.rs
  - id: openwiki-source-e2a1cc763db4a100d48eac0a
    resource: repo://crates/server/src/routes/scheduled_tasks.rs
  - id: openwiki-source-2b4c7ad36a9b595e1991b189
    resource: repo://crates/server/src/routes/sessions/review.rs
  - id: openwiki-source-1043187ef1285c84335110a0
    resource: repo://crates/server/src/routes/sessions/setup_gate.rs
  - id: openwiki-source-a517c0f8e44d2cf0c440fbfc
    resource: repo://crates/server/src/routes/workflows.rs
  - id: openwiki-source-bdb4dae5722fe76ec5e44001
    resource: repo://crates/server/src/routes/workspaces/create.rs
  - id: openwiki-source-303d74f74def684d4690dea9
    resource: repo://crates/server/src/routes/workspaces/git.rs
  - id: openwiki-source-27c2433bab843f01a51418b1
    resource: repo://crates/server/src/routes/workspaces/pr.rs
  - id: openwiki-source-5590f47e4cee001a35a42117
    resource: repo://crates/server/src/routes/workspaces/wiki.rs
  - id: openwiki-source-ed8c84278dba8a1f45af40e9
    resource: repo://crates/server/src/startup.rs
  - id: openwiki-source-04d5adb51b5929dacce999bc
    resource: repo://crates/server/src/workflow_runtime/bootstrap.rs
  - id: openwiki-source-72948db7f01925b4b2ff17af
    resource: repo://crates/services/src/services/pr_monitor.rs
  - id: openwiki-source-cbb62bf26f670475768fcc99
    resource: repo://crates/services/src/services/remote_sync.rs
  - id: openwiki-source-08b3f4dcb1a91c97b6d127f8
    resource: repo://crates/worktree-manager/src/worktree_manager.rs
  - id: openwiki-source-a83ea4cb3c0f336409ebd941
    resource: repo://docs/core-features/completing-a-task.mdx
  - id: openwiki-source-a3944588bb3b08c863d66298
    resource: repo://docs/workspaces/openwiki.mdx
  - id: openwiki-source-db035046d01e3f24722812c8
    resource: repo://packages/web-core/src/features/kanban/ui/IntegrationPanel.tsx
  - id: openwiki-source-1a337d16d294c4dc710f105d
    resource: repo://packages/web-core/src/pages/kanban/KanbanIssuePanelContainer.tsx
  - id: openwiki-source-ffc55e6e31e6ce477ca57329
    resource: repo://packages/web-core/src/shared/hooks/useWorkflowRun.ts
generated: { by: "codex", at: "2026-09-21T08:16:36.701Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-21T08:16:36.701Z
---

# Issue から実行・レビュー・統合まで

[Issue](../concepts/project-and-issue.md) は作業の要求、[Workspace](../concepts/workspace.md) は変更するファイルの場所、[Session / AgentRun](../concepts/session-and-agent-run.md) は会話と実行を表す。一つの Issue に複数 Workspace や WorkflowAttempt が関わり得るため、「作業が成功した」をどの対象の状態として扱うかを区別する。

## 1. 作業先を作り、起動を予約する

通常の create-and-start 経路は Workspace の作成、Issue link、作業ディレクトリ準備、Session 作成の順に進む。Issue 添付の import / 関連づけは警告して続ける分岐もある。後段の起動が失敗しても全体が一つの DB/Git transaction で巻き戻るわけではない。再試行時は既存の Workspace/link/Session を先に確認する。[作成と添付](../../crates/server/src/routes/workspaces/create.rs#L386-L447)

Cloud の Issue/Comment 添付は、upload の confirm だけでは親への関連づけが確定しない。Issue 保存後に本文が参照する Attachment を commit してから、Workspace 作成側の取り込みへ渡る。[Attachment と Blob の段階・期限](../concepts/issue-collaboration.md#attachment-と-blob) がこの共有データ側の契約を説明する。local File への変換・prompt 書換えは [Workspace 側の取り込み](../operations/workspace-inspection.md#session-の添付と-cloud-attachment) を参照する。

入力の user description と再利用する context は [Card context と LLM Wiki](../concepts/card-context-and-llm-wiki.md) で区別する。Workflow の graph を実行する場合は、通常 Session の起動と別に [WorkflowAttempt の準備・Run snapshot](../concepts/workflow-attempt.md) を作る。

## 2. Setup の依存を満たす

Repo setup script の扱いは三つに分かれる。

| 条件 | Agent 起動との順序 |
| --- | --- |
| DirectFolder、または setup なし | setup を待たず起動 |
| setup のある Repo がすべて parallel_setup_script | script を開始し、完了を待たず Agent も開始 |
| 一つでも非 parallel | AgentRun を先に予約し、永続 setup gate の成功後に起動 |

全 parallel 経路では script の開始失敗を警告し、Agent の開始へ進む。依存物が必須の setup を「parallel」にすると待機保証がなくなる。[分岐](../../crates/server/src/routes/workspaces/create.rs#L448-L528)

gate は WaitingSetupStart / WaitingSetup / Satisfied / Failed を保存する。setup が Completed かつ exit 0 で、next_action がない最後の action まで終わったときだけ成功とする。Failed / Killed / 非0終了は予約した AgentRun を失敗にする。[成功判定](../../crates/server/src/routes/sessions/setup_gate.rs#L31-L61)・[gate の反映](../../crates/server/src/routes/sessions/setup_gate.rs#L157-L250)

completion event を取りこぼした watcher は DB の gate から追いつく。逐次 chain の途中成功では起動しないことと、失敗時に進めないことには focused test がある。[回復](../../crates/server/src/routes/sessions/setup_gate.rs#L289-L311)・[tests](../../crates/server/src/routes/sessions/setup_gate.rs#L365-L400)

## 3. 会話を続け、成果物を確認する

実行中の approval/input、steer、cancel、terminal 後の retry は [Session と AgentRun](../concepts/session-and-agent-run.md) の契約に従う。同じ Workspace に別 Session を作ってもファイルは共有される。queue に入れた follow-up の保持や成功後の source checkpoint も、通常の会話操作と結びつく。

差分・実ファイル・動作確認には [Workspace inspection](../operations/workspace-inspection.md) を使う。人の inline comment から Agent に修正を依頼する操作は [Reviewing Code](../../docs/reviewing-code.mdx) にまとまっている。

Agent の review API は Workspace 内に active AgentRun があれば拒否する。use_all_workspace_commits は Repo ごとの fork point を review context に使い、intent=Review、selected_skills なしで起動する。OpenWiki の independent Review と同じ隔離プロトコルではない。[review の起動](../../crates/server/src/routes/sessions/review.rs#L39-L115)

## 並列変更を試してから正式統合する

他 Card の文脈や branch を調べるときは、[Shared dirs と並列 context](../concepts/card-context-and-llm-wiki.md)で関係を発見し、採用する commit を固定する。発見された最新 HEAD や他 Workspace の未統合 Memory を、既に統合済みの仕様として扱わない。

組合せ試験には `POST /api/workspaces/<id>/integration/preview` がある。自分の base commit と peer の commit を明示し、Repo membership、各 commit がその Workspace branch に帰属する祖先であること、自分の checkout が指定 base に一致して clean なことを確認する。古い固定 commit は許すが、自動的に新 HEAD へ追従しない。自分を含む source は最大100件である。[preview の受付](../../crates/server/src/routes/parallel_context.rs#L105-L166)

API が作るのは base からの detached worktree と固定 peer 一覧であり、peer の merge や tests を自動実行する API ではない。返す指示に従ってその試行先で組み合わせを検証し、元 Workspace の reset/stash、peer の修正、target/Card 更新、Wiki 執筆を持ち込まない。試行は通常 Workspace cleanup の外側の sibling directory に残し、調査後に明示的な worktree cleanup を行う。正式統合成功の証拠にはならない。[試行の用途](../../crates/server/src/routes/parallel_context.rs#L162-L171)、[保持と source ref 不変の test](../../crates/worktree-manager/src/worktree_manager.rs#L55-L146)

local Board の **Integrate / Auto Merge** では、Repo・local target と各 Card の採用 Workspace を選び、Code 実行として送信する。通信再試行は同じ request ID と入力を保持する。source は受付時、target base は queue 先頭で実行を始める時点に固定する。実行用 Workspace の履歴で進行・検証・失敗を確認する。[Board の選択と送信](../../packages/web-core/src/features/kanban/ui/IntegrationPanel.tsx#L74-L184)

この [正式 Integration](../concepts/formal-integration.md)は、選択要件と source を保存・予約し、専用環境で host が検証した結果 commit を local target へ公開する。Git 公開、条件付き Card Done、後続 Wiki receipt は別の結果なので個別に確認する。取消できる phase と recovery_required の扱いも所有フローに従う。詳細な受付・検証・公開契約は同ページに集約し、以下では既存の手動 Git/PR 経路を説明する。

## 4. Rebase と direct merge

Git 操作は Repo 単位で target_branch を持つ。Rebase API は新 branch の存在確認後に WorkspaceRepo の target を更新し、それから Git rebase を行う。conflict は file と operation を持つ構造化エラーになり、continue / abort の経路がある。エラー時には、Git 状態だけでなく更新済み target 設定も確認する。[Rebase の順序](../../crates/server/src/routes/workspaces/git.rs#L771-L854)・[操作 router](../../crates/server/src/routes/workspaces/git.rs#L137-L148)

Git safety tests は通常の untracked file の保存、tracked dirty change の拒否、base に上書きされる untracked file の保存を検証する。conflict を「とりあえず reset」で消すことを前提にしない。[Rebase tests](../../crates/git/tests/git_ops_safety.rs#L442-L505)

手動 Direct merge は正式 Integration とは別の入口で、その Repo に open PR がある場合と、target が remote branch の場合に拒否する。実際の統合は squash であり、base が先行していれば拒否する。base が別 checkout にあればその場所で CLI merge、checkout がなければ libgit2 の ref 操作を使う。統合後は task branch ref も新 squash commit へ更新し、続きの作業をそこから行えるようにする。[API の制約](../../crates/server/src/routes/workspaces/git.rs#L179-L214)・[Git の統合](../../crates/git/src/lib.rs#L678-L776)

checkout 済み base の staged changes は拒否する。専用 test は staged file を残したまま merge が失敗することを検証する。[test](../../crates/git/tests/git_ops_safety.rs#L587-L605)

memory 有効時は source checkpoint と明示 event の integration 記録を準備してから Git を実行し、成功した commit を保存する。その後で local Merge record、remote 同期、Workspace archive が続く。Git 成功と後段の記録保存は一つの transaction ではない。[統合順序](../../crates/server/src/routes/workspaces/git.rs#L226-L308)

## 5. Push と Pull Request

Push と PR 作成は、外部公開より前に未完了の source checkpoint を処理する。PR 作成は push remote と base remote を解決し、base branch の存在を検証、source を準備、branch を push、その後 GitHost API で PR を作る。push 後の PR 作成が失敗した場合も、branch の push は既に済み得る。[Push](../../crates/server/src/routes/workspaces/git.rs#L322-L358)・[PR 作成](../../crates/server/src/routes/workspaces/pr.rs#L239-L334)

GitHostService の現行 provider は GitHub と Azure DevOps。未知 URL は UnsupportedProvider である。[provider 境界](../../crates/git-host/src/lib.rs#L54-L66) provider 固有の CLI 認証や使い方は [GitHub integration](../../docs/integrations/github-integration.mdx) と [Azure Repos integration](../../docs/integrations/azure-repos-integration.mdx) を参照する。

PR の外部作成成功後、local PR record の保存エラーはログに残し、remote 同期は非同期、ブラウザー起動と任意の description follow-up の失敗は警告して成功応答を返す。「PR が作成された」と「全ての付随処理が成功した」を区別して診断する。[作成後の部分失敗](../../crates/server/src/routes/workspaces/pr.rs#L336-L398)

組織に接続した GitHub App が PR opened や `!reviewfast` コメントから起動する [Cloud Review](../integrations/github-review.md) は、ここでの手動 PR 作成とは別の実行経路である。App 設定・repository 有効化・R2・外部 worker を要し、webhook の受理はレビュー完了を意味しない。[起動条件](../../crates/remote/src/routes/github_app.rs#L821-L872)

## 6. Issue の完了と Workspace archive

正式 Integration では、公開後も受付時の Card 内容・revision・リンクが一致する場合にだけ local Card を Done にする。以下の手動 merge/PR の remote 同期とは別の契約である。[条件付き Done と独立した Wiki 結果](../concepts/formal-integration.md#gitdonewiki-は独立した結果)

Direct merge 後は pinned でなければ Workspace archive を試みる。この条件はその Repo の merge 後に評価される。一方 PR monitor は merged を観測した際、Workspace の open PR が0で、かつ pinned でない場合に archive する。複数 Repo の作業では両者の条件が異なる。[Direct merge 後](../../crates/server/src/routes/workspaces/git.rs#L288-L308)・[PR 観測](../../crates/services/src/services/pr_monitor.rs#L128-L207)

[Completing a Task](../../docs/core-features/completing-a-task.mdx) は merge 後に Done へ移る操作意図を説明するが、現行実装は条件付きの複数処理である。direct merge の remote Issue 状態同期は非同期で、未認証や remote Workspace 不在なら skip する。local Issue を常に Done へ直接更新する処理として読まない。[remote 同期の失敗](../../crates/services/src/services/remote_sync.rs#L85-L114)

Remote の Issue 状態同期は、WorkMerged で関連 PR がすべて Merged の場合に、project 内の名前が Done の status を探して更新する。status が存在しなければ変更しない。ReviewStarted は In review を使う。独自 status 名、closed-but-unmerged PR、remote link 不在は自動遷移を評価するときの重要な条件になる。[状態同期](../../crates/remote/src/db/issues.rs#L520-L598)・[Workspace link の解決](../../crates/remote/src/routes/workspaces.rs#L162-L193)

## 7. 統合後に canonical memory を更新する

[Repository memory](../concepts/repository-memory.md) が有効な場合、成功した coding run の semantic draft と Git 証拠から Change Manifest を作り、統合された event を maintenance の入力にする。source merge の成功と Wiki 更新の成功は別であり、Wiki 失敗は source を巻き戻さない。

EVK 自身の手動 direct squash と正式 Integration はそれぞれ明示的な event ID を記録できる。外部 PR の squash/rebase は元 commit の ancestry を証明できない場合があり、Workspace ID や時刻だけで event を消費しない。統合先 local branch を準備し、必要なら [Sync Wiki](../operations/openwiki-maintenance.md) で実 source を再確認する。[記録された運用契約](../../docs/workspaces/openwiki.mdx)・[外部 PR の制約](../../docs/workspaces/openwiki.mdx)

生成物は [Wiki Viewer](../operations/workspace-inspection.md#wiki-の本文ツリーと表示元を確認する)で確認できる。ただし Viewer は選択 Workspace の現在のファイルを読むため、maintenance の未公開成果物も見える。Wiki 本文を開けたこと、Agent が完了を宣言したこと、Workflow と target branch への publication が成功したことを分けて確認する。[表示元の実装](../../crates/server/src/routes/workspaces/wiki.rs#L148-L169)、[publication の検証](../../crates/server/src/workflow_runtime/bootstrap.rs#L603-L651)

## 定時に Workflow を起動する

ScheduledTask は既存 Workflow template を日次・週次で起動する設定である。現行 target は Workflow のみ、concurrency policy は SkipIfRunning のみ。時刻になった task を期限付き DB claim で取得する。[型](../../crates/server/src/routes/scheduled_tasks.rs#L42-L59)・[claim](../../crates/server/src/routes/scheduled_tasks.rs#L288-L337)

毎回 template の Agent node の session_id を消した graph から新しい WorkflowAttempt を作る。実行は [通常の Workflow lifecycle](../concepts/workflow-attempt.md) に入り、trigger_source は schedule / schedule_manual を使う。[起動](../../crates/server/src/routes/scheduled_tasks.rs#L445-L487)・[Session binding の初期化](../../crates/server/src/routes/scheduled_tasks.rs#L926-L937)

前回の last_run_id が pending / running / awaiting_human / awaiting_arena なら今回は skipped とし次回時刻へ進める。人や Arena の待ち時間も「前回が active」に含まれ、同じ slot を無期限 queue に積む契約ではない。[skip](../../crates/server/src/routes/scheduled_tasks.rs#L388-L404)・[active の範囲](../../crates/server/src/routes/scheduled_tasks.rs#L727-L744)

時刻計算は IANA timezone を用い、daily/weekly と DST の focused tests がある。server のローカル timezone や固定 UTC offset だけで schedule を再実装しない。[tests](../../crates/server/src/routes/scheduled_tasks.rs#L1218-L1274)

定時 task の起動と、その後に Agent 完了を検知して graph を進めることも別である。この checkout では completion watcher は embedded backend の startup で登録され、standalone main には登録がない。GET/SSE は reconciliation を行い、UI は active run を既定 4 秒で poll する。無人の standalone 運用で予定通り起動したことだけを根拠に、後続段階まで自動進行すると判断しない。[起動形態と前進の契機](../architecture/workflow-runtime.md#起動形態と前進の契機) に登録箇所・read 側の処理・文書の自動連鎖という意図との差をまとめる。[embedded 登録](../../crates/server/src/startup.rs#L214-L225)、[standalone 入口](../../crates/server/src/main.rs)、[poll](../../packages/web-core/src/shared/hooks/useWorkflowRun.ts#L26-L55)
