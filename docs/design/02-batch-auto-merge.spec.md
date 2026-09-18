---
title: "EVK Workspace Integration / 複数カードのAI自動統合仕様案"
description: "固定source集合の統合・検証と、検証済みコミットの正式反映・Done更新を定義する。"
---

# EVK Workspace Integration / 複数カードのAI自動統合（Integrate / Auto Merge）仕様案

文書ID: EVK-AUTO-MERGE / 版: 0.3 / 作成・改訂日: 2026-09-17

状態: **第2.2節の採用範囲を実装・検証済み（2026-09-18）。試験証拠と保証範囲は[実装記録](parallel-integration-implementation.md)を参照**

> 仕様改訂時の基準main・既存シンボル・静的調査の限界は [レビューガイド](00-review-guide.md) 第2節を参照。今回の実装・実機試験は上記の実装記録に分離する。既存manual mergeのsquash／source ref更新／完了通知を、そのまま正式Integrationの反映処理へ流用しない。

## 1. 目的と責任分界

### 1.1 ユーザーが行う操作

本書では、Workspace間で成果を組み合わせる一般能力と、ボードから成果物をtarget branchへ正式採用するワークフローを区別する。

通常のWorkspace Sessionでは、ユーザーが「Workspace Bの変更と合わせてテストして」のように指示し、別Workspaceの確定済み成果をReadOnlyなsourceとして参照して、現在Workspaceまたは一時統合環境で組み合わせ検証できる。これは開発中の操作であり、target branchやCard statusを自動変更しない。

一方、ユーザーはボードのIntegrate / Auto Merge操作から、終わらせたい仕事のカードと採用するWorkspaceをまとめて選ぶ。EVKは専用の統合作業環境とSessionを用意し、AIが変更意図・実コード・テストを見て統合を行う。検証できた成果を指定したローカルブランチへ正式反映し、対象カードをDoneにする。

人間はカードごとのGitコンフリクト処理を逐一操作しない。AIが今は統合すべきでないと判断した場合には、ターゲットへ反映せず、理由と解決に必要な事項を示す。

### 1.2 責任分界

| 主体              | 担当                                                                                     |
| ----------------- | ---------------------------------------------------------------------------------------- |
| ユーザー          | 対象カード／採用Workspace／ターゲット／実行設定を選ぶ                                    |
| EVK               | 対象固定、予約、実行状態、同一ターゲットの直列化、検証記録、反映と復旧の整合性           |
| 統合エージェント  | Wiki・Manifest・実コードの解釈、統合順、競合解消、必要な修正、テスト選定・実行、保留判断 |
| Git・既存実行基盤 | コミットとworktreeの操作、プロセスの実行結果、参照の安全な更新                           |

**A-01［合意］意味的統合エンジンをEVKへ新規実装しない。**

通常のWorkspace／Session／executor／Gitサービスをできるだけ再利用する。独立したDAG、依存関係DB、Card別merge bot、専用の共有記憶ストアは不要。Integration Runは対象と結果を結びつける論理的な実行記録であり、直ちに新しいテーブル群を作る指示ではない。

**A-01A［合意］Workspace Integrationと正式昇格ワークフローを分離する。**

別Workspaceのcommitを参照・統合・検証する能力そのものは、Auto Mergeボタン専用の特殊機能ではない。通常Sessionからもユーザー指示に基づいて利用できる。本機能内ではtarget branchの更新とCardのDone化をボード側の正式昇格へ限定するが、既存の手動merge／PR経路を廃止・禁止するものではない。

**A-01B［合意］初版のcross-workspace sourceはReadOnly。**

Workspace AのエージェントはWorkspace BのManifest、diff、確定済みcommitを参照できるが、Bのworktreeやsource branchを直接編集しない。A+Bの組み合わせ検証が必要なら、Aまたは一時統合環境でBをsourceとして取り込む。B側そのものの修正が必要と判断した場合は、その必要変更を報告して停止する。

「必要ならBも直して」という自然言語指示だけでcross-workspace write権限へ昇格しない。B側のCodexへ仕事を委譲するDelegationや、MCP経由のAgent-to-Agent操作は将来拡張とする。

## 2. 合意済みのUXと初版範囲

**A-02［合意］ボード上のIntegrate（UI名はAuto Mergeでも可）ボタン＋複数選択を正式昇格の入口にする。**

Auto Mergeステータス列への移動は初版の実行トリガーにしない。ここでいうボタンは「単にmergeする」機能ではなく、選択成果をtargetへ統合・検証・正式反映する操作である。ユーザーが意図した集合を一度に指定することで、投入タイミングのずれとバッチ境界の曖昧さを避ける。

**A-03［合意］選択集合単位で反映する。**

A・B・Cが選択されたら、原則としてA・B・Cを合わせて検証し、まとめてターゲットへ反映する。AIが勝手にCを外してA・Bだけ反映したり、未選択のDを追加したりしない。必要な集合の変更を提案して保留できる。

ここでいう「まとめて」は、単一ターゲットrefへの最終反映単位を意味する。Git履歴を１コミットにsquashすることや、テストの外部副作用までロールバックできることを意味しない。

**A-04［合意］完了状態とUndoを結びつけない。**

成功したカードをDoneへ移す。Done→In progressは作業再開であり、過去のmergeを取り消さない。調査等のカードを手動Doneにする経路も残す。Doneは「必ずAuto Merge済み」という状態には変更しない。

**A-05［合意］ローカル統合と既存手動PRを両立させる。**

remoteやGitHub認証がなくても、ローカルターゲットへの統合が成立すること。既存の手動merge／PR作成導線は保全する。PRを自動化しないのは本版のスコープ判断であり、技術的に自動化不可能という意味ではない。

### 2.1 初版に設ける制限の提案

| 項目                                                | 初版案                                                           |
| --------------------------------------------------- | ---------------------------------------------------------------- |
| １実行の反映先                                      | 単一Gitリポジトリの単一ローカルブランチ                          |
| １カードの採用成果                                  | 原則１Workspace。複数候補ならユーザーが明示選択                  |
| 複数の相補的Workspaceを１カードから採用             | 初版の必須にしない。未対応時は明示し、勝手に最新１件へ縮約しない |
| dirtyな成果・書き込みプロセス実行中                 | そのまま投入しない。変更確定・停止後に再選択                     |
| squash／source branchの履歴書き換え                 | 初版では行わず、sourceの履歴を保つ統合を基本とする               |
| 反映後Undo、auto push、auto PR、auto deploy         | 対象外                                                           |
| 通常Sessionからの「他Workspaceと合わせてテスト」    | 対応対象。source WorkspaceはReadOnly、Card/targetは変更しない    |
| 通常Sessionから他Workspaceそのものを直接修正        | 対象外。必要変更を報告して保留                                   |
| 他WorkspaceのCodexへのDelegation／MCP経由の修正依頼 | 将来拡張。初版は対象外                                           |
| 同一ターゲットへの複数Run                           | 直列化。機能開発そのものは他Workspaceで並行できる                |
| 複数EVKインスタンスから同じrepoを同時操作           | 現行が保証していなければ初版の対応範囲外と明記                   |

multi-repo Workspaceは現行に存在する。他repoにも未統合変更があるカードを一つのrepoだけ統合して自動Doneにしてはいけない。既存モデルにbatch用の横断完了判定があるとは仮定せず、次の範囲判断を明示する。

### 2.2 合意済みUXとは区別する実装前の範囲判断

2026-09-17の実装依頼により、次の最小案を今回の範囲として採用する。正式Integrationはlocal Board・単一repo/target・原則1Card1採用Workspace。他repoに未統合成果があるCardは拒否し、部分反映で全体をDoneにしない。通常のmulti-repo Workspaceは維持する。単一EVKサービスの協調的ローカル実行を対象とし、既存権限は拡大しない。同じGitストレージの二重登録による排他回避は防ぐ。remote Boardの新機能・複数サービス保証・強いOS隔離は対象外。下表のremote拡張等は将来の判断候補である。実装・試験は[実装記録](parallel-integration-implementation.md)へ記録する。

| 論点                 | 現行の事実                                                                                                            | 最小案／必要な判断                                                                                                                                             |
| -------------------- | --------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| local / remote Board | local Issue更新とremote同期は別経路。既存merge後のremote Doneは非同期best-effortであり、local Boardの完了保証でもない | local Boardから実装する案。remoteも対象なら条件付き完了と冪等な結果照合をremote契約へ追加する。未対応Boardでは開始前に理由を表示し、公開後に初めて判明させない |
| multi-repo Card      | WorkspaceRepoごとにtargetがあり、１Card全体の完了と単一repoの反映は異なる                                             | 他repoに未統合成果があるCardを初版で拒否する案。部分反映を提供するなら、Card全体をDoneにするUXとの違いを明示して決める                                         |
| 書込保証             | 現行Codex設定にはdanger-full-accessがあり、別worktreeだけでは強隔離にならない                                         | A-19の協調的ローカル実行が最小案。強隔離を必須にする場合は既存sandboxで実現可能かを先に確認し、巨大基盤を暗黙に追加しない                                      |
| 複数EVKインスタンス  | dispatcher leaseだけで共有Gitへの全writer排他は保証できない                                                           | 単一EVKサービスを対象とする案。対象外の外部writerは検出限界を明示。複数サービス対応を表示するならプロセス間の共通lockを実証する                                |

どの範囲を選んでも、未選択成果の追加禁止、ユーザー変更の保護、検証済みRと反映Rの一致、Git反映後の復旧は省略しない。

## 3. 開発中のCross-workspace Integration（通常Session）

**A-06［合意］通常Sessionでは、他Workspaceの成果をReadOnlyなsourceとして組み合わせ検証できる。**

例: Workspace Aでユーザーが「Aの実装をWorkspace Bと合わせてテストして」と指示した場合、CodexはBのManifest・diff・確定済みcommitを調査し、Aまたは必要なら一時的な統合作業環境でA+Bを構成してテストできる。Bのworktreeやsource branchは変更しない。

この操作は開発中の補助手段であり、次を自動では行わない。

- target branchの更新
- CardのDone化やstatus変更
- BのCard要求・Manifestの書き換え
- Bのsource branchへのcommit/rebase/force update
- 未選択の別Workspaceへの連鎖的な書き込み

「合わせてテストするだけ」と「BをAの開発成果へ取り込んで残す」を区別する。前者は通常coding runの終了時source commit／Manifest発行に検証用変更が紛れ込まない試験専用経路とする。後者をユーザーが求めた場合はAへの通常の変更確定を許容するが、peer Bやtargetの変更、Done化を伴わない。AがDirect-folder等でtargetそのものをcheckoutしている場合、target非変更の契約を満たすため一時環境を使う。

**A-07［合意］B側の修正が必要なら、初版ではBを直接直さず要求を返す。**

A+Bのテストで、Bの実装がAの新しい契約に追随しないと成立しない等、source B自体の修正が必要と判断した場合、Codex Aは「Bで何をどう修正する必要があるか」「なぜ必要か」「どのテスト／契約で判明したか」を具体的に報告し、その統合作業を保留する。

単純なGit conflictを一律に「B側修正必須」とみなさない。一時統合環境だけの競合解消で双方の既存要求を保てる場合は、sourceを変更せず解消・検証してよい。Bの要求・実装そのものを変えなければ成立しない場合が `peer change required` である。

**A-08［提案］通常Sessionの統合方法は既存Git/Workspace機構を優先し、Auto Merge専用基盤へ依存させない。**

現在Workspace上でユーザー変更と終了処理を安全に区別できる場合はそれを使える。検証だけの操作では既存worktree管理による一時環境を最小案とし、自動stash/resetでAを戻す設計にはしない。再利用できるsource objectは固定OIDで読み、peer worktreeを作り直す副作用のあるfile APIを読取入口にしない。object不在は読取不可として示す。

既存`merge_workspace`／`GitService::merge_changes`はtargetへのsquash反映とsource ref更新を伴うので、この準備・試験には使わない。Git低レベル操作とworktree managerを再利用し、試験専用の終了処理は既存の実行目的／`finalize_source`境界へ最小限追加する。通常coding run全体の自動commitを止めて解決しない。

将来Delegationを追加する場合は、Workspace AがBを直接編集するのではなく、B側のWorkspaceに新しいSessionを起動し、Bの文脈と所有権の中で修正して結果を返す方式を優先する。MCPはその構造化インターフェース候補だが、初版の必須要件ではない。

## 4. 候補選択UI

### 4.1 選択パネル

**A-09［提案］「統合可能」ではなく「統合候補」と表示する。**

機械的に選べる状態と、AIが意味的に統合してよいと判断する状態は異なる。

```text
Integrate / Auto Merge

Repository: selected-repository
Target:     dev                         [変更]
Agent:      既存のCodex実行設定           [変更]

選択  カード                 採用Workspace           状態
 ☑   Device検索API           ws-device-search        選択可
 ☑   Pagination追加          ws-pagination           選択可
 ☑   認証処理の改善          ws-auth-v2          ▾   候補2件
 □   ログ出力の改善          ws-logging              Session実行中
 □   DB変更                 ws-schema               未コミット変更あり

選択した3件を1回の統合として処理します。
検証に成功した場合のみdevへ反映し、カードをDoneにします。

[閉じる]                            [統合を開始]
```

表示する実際の文言・選択コンポーネントは既存UIに合わせる。この図は新しい画面フレームワークの導入指示ではない。

### 4.2 選択規則

**A-10［提案］Card選択を、明示されたWorkspaceとコミットの選択へ変換する。**

候補が１つなら自動選択してよい。複数なら採用Workspaceを指定させる。「最終更新が新しい」「AIが好みそう」という理由だけで自動採用しない。１つのWorkspace内の複数Sessionは別のmerge対象にしない。

開始時に採用するのはWorkspaceの確定したcommitであり、カード名やブランチ名という可変ポインタだけではない。表示中にHEADが変わった場合は再確認し、見ていた成果と別の成果を黙って投入しない。

Cardの実体はlocal/remoteのIssueとして特定し、legacy Task IDと混同しない。固定集合にIssueの保存先・project・ID、採用WorkspaceRepo、現在のリンク関係、要求snapshotを記録する。Integration WorkspaceはCardなしで作り、全CardからはRun結果への参照で辿る。`insert_workspace_link`を全Card分呼ぶとworkspace_idのupsertで前のリンクが上書きされるため、この用途へ流用しない。

**A-11［提案］Cardのステータスだけで候補を限定しない。**

In progressでも、実装が終わりエージェントが停止していれば選べる。手動Doneだが未統合の成果も、明示操作で候補にできる。逆にDoneでも、そのtargetへの取り込み済みをGit・既存統合記録等で確認できなければ、取り込み済みとは表示しない。

同じCardの未採用Workspaceが実行中の場合、カード完了と矛盾するため初版では投入を保留する案を基本とする。複数成果・複数repoがある場合に、何をもってカード全体が完了するかを曖昧にしない。

**A-12［提案］選択される差分の範囲を隠さない。**

「カードの名前に対応する変更だけ」をGitが自動で切り出すとは扱わない。採用HEADには、長期利用したWorkspaceの全変更や、別カードを基点にした未統合の祖先コミットも含まれ得る。

AIは実際のtargetとの差分と共有Manifestを読み、既知の未選択成果が実質的に取り込まれるなら、それを明示して選択集合の見直しを求める。Bの祖先にAがある場合、Bだけを選んでもAが入ることを見落とさない。EVKに意味的な所有権判定DBを作るのではなく、Git上で確認できる祖先・差分とAIの調査を使う。

## 5. 対象固定・予約・キュー

### 5.1 固定する情報

**A-13［合意＋提案］投入時にsourceを固定し、処理開始時にtargetの基点を固定する。**

| 情報                                                 | 固定・記録するタイミング                                                           |
| ---------------------------------------------------- | ---------------------------------------------------------------------------------- |
| source repository／Card／Workspace／commit OID       | ユーザーの開始操作を受け付ける時点                                                 |
| Cardの要求・採用判断のrevisionまたは対応するsnapshot | 同上。完了判定対象を明確にする                                                     |
| targetのrepo・完全なbranch ref                       | 同上                                                                               |
| UIに表示したtarget OID                               | 参考として記録する                                                                 |
| 実際のtarget基点 `B`                                 | キューから取り出し、そのtargetの実行権を取得した時点                               |
| 最終的な統合候補 `R`                                 | 実装・修正が完了し、最終検証に入る前                                               |
| 検証対象と結果                                       | `R`に対する実行結果として記録                                                      |
| sourceに対応するManifest event集合                   | 固定source OIDとの対応を確認して記録。後から出たeventを自動追加しない              |
| 必須検証plan                                         | repo規約と選択要求から確定し、最終検証前にhost側へ固定。変更時は理由と再検証を記録 |
| 反映前後OID                                          | `B → R`の反映時に記録                                                              |

キュー待ち中に先行Runがtargetへ統合することは正常である。そのため「実行開始時の最新targetを基点にする」とUIで説明する。実行開始後にtargetが変わる場合は別の競合として扱う。

コミット保持には既存のref／Workspace寿命管理を利用する。保持用refが必要なら導入理由を示す。成果物をbranch名だけで追跡してsourceが進むたびに内容を変える設計にはしない。Cardの単なる表示更新時刻だけで要求revisionを代用せず、要求・採用判断・関連付けに関わるsnapshot/digestを使う。

### 5.2 機械的な投入条件

**A-14［提案］次の条件を満たさなければ、AIを起動する前に理由を返す。**

- repo・target・source commitが実在し、対象repoに属する。
- 選択成果がdirtyではなく、merge/rebase途中でもない。関連する未追跡成果を無視しない。
- 採用Workspaceおよびカード完了に影響する他Workspaceに、継続goal、queued follow-up、script等の書き込み処理・起動予約がなく、Memory source確定処理も終了している。
- 同じカード／成果が別の未完了Runで予約されていない。
- 初版の単一repo等の対応範囲内である。

無関係なignoredビルドキャッシュがあるだけで候補から外す必要はない。一方、実装がignoredファイルや共有生成物だけに存在するなら、commitだけでは成果を受け渡せないことを明示する。dirtyを解消するための自動stash、強制reset、他人の成果の自動commitは行わない。

### 5.3 予約と操作制限

**A-15［提案］UIのグレイアウトだけでなく、EVK経由の競合操作をサーバー側でも制限する。**

投入からsource固定・検証・反映の確定まで、選択成果・カードの対象変更を予約する。関係するSessionの新規開始／follow-up／goal自動継続、branch変更、削除、対象Cardの仕様変更・Done移動等は、競合するときに拒否し、Runへのリンクを返す。読み取りやログ閲覧は維持する。

既存機構で予約を表現できる場合は流用する。外部エディタや手動GitまではUI予約で防げないため、Git状態の再確認も必要である。予約解除はエージェントの子プロセスや再起動予約が止まったことを確認してから行う。

現行のSession単位active checkや、１Session分のメッセージを保持するin-memory queueはこの予約の代わりではない。対象確認と予約取得を競合しない永続処理にし、start／follow-up／goal／queue消費／script／rebase／manual merge／リンク変更／削除等の関係するmutation境界で共通guardを確認する。dispatcher lease消失やTTL満了だけで業務予約を解除しない。未反映のblocked/failed/cancelledでは停止確認後に解除できるが、反映の有無が不明なら復旧照合まで保護する。

Git反映と作業環境の整合が確定しwriterが停止済みなら、Card後処理だけの失敗で開発を無期限に凍結する必要はない。反映事実・Card別期待条件・後処理状態を永続化してから実行予約を解除できる。その後の再送はA-36の条件付き更新とし、cleanupに必要な記録保護とは別に扱う。

**A-16［提案］同じターゲットの統合処理を直列化する。**

ロックの論理キーはcanonicalなGit common-dir等のストレージidentityと完全なtarget refである。既存`get_common_dir`を再利用できる。同じrepoを二重登録した場合に別IDだから同時反映できる設計にしない。一方、Manifestの参照範囲は登録repository IDであり、この二つのidentityを混ぜない。

初版でrepo全体のAuto Mergeを１本ずつにする保守的な実装も許容する。これは通常の機能開発並列数を減らすことではない。

長時間のIntegration同士の実行予約と、短いGit反映lockを分ける。既存Memoryのsource-integration lockは登録repo単位で、Memoryが無効なら取得しない経路もある。そのまま唯一の排他にせず、Memory無効時・二重登録時にも機能する共通Git反映guardへ寄せる。LLM実行中にこの短いlockを握り続けない。

手動merge、OpenWiki publication等も同じ短いGit反映guardへ参加させる。長い統合中に別の正規操作がtargetを更新した場合はA-32で保留する。外部Git操作は別途変更検出の対象であり、完全な排他があると偽らない。

## 6. 統合用WorkspaceとSession

**A-17［合意］１Runに１つの統合用作業環境を作る。**

sourceごとの統合用Workspaceを作らない。target基点`B`から１つとfresh Sessionを作り、そこで選択集合の統合・修正・検証を行う。同じSession内の修正・再検証には複数AgentRunを使ってよく、「１Run」を１回のモデル呼出しとは定義しない。

反映前にblocked/failedで終端化し、新target／再選択でやり直す場合は新しいIntegration Runを作る最小案とする。途中node resume基盤は要求しない。反映済み・完了処理待ちは新しいRunで再mergeせず、同じRunの後処理を冪等に再実行する。旧processが生存する場合は先に所有権と停止状態を確認する。

論理名の例は `integration/<run-id>`。実際のbranch名、Workspace名、保存場所は既存命名規則に従い、ユーザーの既存branchを上書きしない。

**A-18［提案］通常Workspaceの副作用を確認してから再利用する。**

現行`Workspace.task_id`はOptionalで、`create_workspace_record`にCardなし作成経路がある。これを使い、架空Issueを作らない。ただしcardlessであることだけでは通常終了処理の副作用は消えない。既存実行設定にIntegration／試験専用という目的を識別できる最小情報を持たせる。

通常のSession終了、workflow完了、agent終了を理由に、元カードを先にDoneへ移さない。既存manual mergeがDone化やarchiveを行う場合、統合途中の処理へそのまま呼ばない。

`handle_agent_run_terminal`は成功時にMemory source確定とqueued follow-upを進め得る。試験専用では成果publicationを行わず、正式Integrationでは検証後の追加commitや未確認follow-upでRが変わらないようにする。既存writerをcleanなR上で使うことは可能だが、R以後の変更があれば成功を失効させ、再commit・再検証する。source Workspaceの自動archiveもbatch成功の副作用にしない。

### 6.1 worktree分離と権限制限は別

**A-19［提案・実装前P0確認］保証範囲を明示する。**

Git worktreeではbranch refs等が共有される。別worktreeを作るだけでは、そこで動くプロセスによるtarget直接更新を技術的に禁止できない。[Gitの一次資料G1](https://git-scm.com/docs/git-worktree)

本案の最小構成が保証するのは、EVK自身の実行・反映経路の整合性である。unrestrictedなエージェントが同一権限でGitや元リポジトリへアクセスできる環境では、「AIが誤った任意コマンドを実行しても絶対にtargetへ影響しない」という保証にはならない。

実装担当は次の二つを混同せず判断する。

| モード             | 条件と表示                                                                                                           |
| ------------------ | -------------------------------------------------------------------------------------------------------------------- |
| 協調的ローカル実行 | エージェントが指示に従う前提で、targetをEVKのpublish経路からだけ更新する。範囲外の直接操作を完全防止する保証はしない |
| 強い書き込み隔離   | 既存sandbox／プロセス権限／隔離Gitストア等でsource・targetへの直接書き込みを制限できる場合に限り、その範囲を保証する |

隔離cloneだけでも、元repoが同じ権限で書けるなら十分ではない。新しい巨大なセキュリティ基盤を暗黙に必須化せず、現行executorで可能な方法と、達成できない保証をレビューで明示する。UI説明は実装したモードに合わせる。

## 7. エージェントの仕事

### 7.1 入力

**A-20［合意＋提案］統合Sessionへ渡す情報を限定・固定する。**

入力は選択したCard要求、採用Workspaceと固定source OID、target基点`B`、関連Wiki・Manifestへの入口、既存の検証手順、作業環境、権限境界、停止条件である。

固定sourceに対応するevent ID、参照時点、hostが保持する検証planへの参照も渡す。大量Manifestやログをpromptへ全文複製する必要はない。他Workspaceの私的Workspace Memoryや会話履歴は入力に含めない。

他WorkspaceのManifestも背景として参照できるが、統合対象への自動追加権限はない。Manifestがない場合は欠損を明示し、要求と差分から調査できる範囲を調査する。情報が足りず選択成果の意図を保てない場合は保留する。

### 7.2 実行方針

**A-21［合意］コンフリクトは解消対象であり、それだけでは拒絶理由にしない。**

AIは実際の差分・要求・Manifest・テストを読み、選択された機能が両立するよう統合する。同じファイルを触っている、通常のGit mergeが止まった、という理由だけで直ちに人間へ差し戻さない。

一方、仕様そのものが相容れず、選択済み要求から解決方針を導けない場合、片方を捨てて通してはいけない。意味的な判断は自然言語の理由と対応箇所を残す。

**A-22［提案］sourceを変更せず、統合用branchで修正する。**

source branchのrebase／force update／履歴の書き換え、source Workspaceの直接編集を行わない。既存の履歴を保ったmergeを基本とし、必要な競合解消・互換修正は統合用branchに置く。Board側の正式Integrationでは、選択された要求を両立させるための修正を統合用branch上で行う権限がある点が、通常SessionのReadOnly cross-workspace検証との違いである。source Workspace自体はどちらの場合も変更しない。

複数sourceの祖先関係や共通変更を見て重複を避けるが、ソースの選択を無断で変更した扱いにしない。squashや選択commitの部分cherry-pickは初版の標準にしない。採用sourceのOIDと最終結果への対応を追跡可能に保つ。[Gitの一次資料G2](https://git-scm.com/docs/git-merge)

**A-23［提案］無関係な改善や未選択の仕様変更を混ぜない。**

統合に必要な互換修正や追加テストは許可する。広範なリファクタ、機能追加、依存パッケージの不要な更新、既存保護の無効化はしない。必要変更が選択集合の範囲を超える場合は、その理由を示して保留する。

## 8. 検証契約

### 8.1 「全テスト」の範囲

**A-24［合意＋提案］全テスト成功を無条件の安全証明にはしない。**

実施する検証は、targetが元々持つ回帰テスト、各sourceが追加・更新した受入テスト、統合によって必要となった横断的テストを含む、当該repoの完全な検証手順を基本とする。build、lint、typecheck等も既存規約に従う。

「全テスト」の具体的なコマンドと必要環境は、リポジトリの既存設定・AGENTS等の規約・各Manifestから調べる。EVK専用の言語横断テスト推論エンジンを作らない。既存の検証コマンド設定があるなら流用する。

必須／任意、コマンド、cwd、必要環境、根拠を最終検証前にhost管理のplanへ固定し、エージェントの最終報告だけで必須項目を減らさない。エージェントが調査・提案するテスト範囲の十分性と、hostが照合する実行事実を分ける。規約に正規の対象外がある場合は理由を記録して除外できる。たとえば本repoのAGENTSが通常検証から除くprivate billing依存のremote backend Cargo検証を必須化しない。対象領域を変更した場合の未検証範囲は別途記録する。

テストが存在しないこと、必要環境がないこと、実行をskipしたことは成功ではない。必須検証を実施できなければ自動反映しない。ユーザーが必要な設定やテストを整えて再試行するか、別の既存手動経路で判断する。

テストがないrepoを無条件で合格にも不合格にもせず、適用可能なbuild／静的検査／受入確認等の具体的なplanを定める。必須項目が未定・空で安全な判定ができなければ`VALIDATION_UNAVAILABLE`とし、任意の`exit 0`を代替証明にしない。

### 8.2 検証の二段階

**A-25［合意＋提案］Workspaceでの検証報告と、統合後の実行結果を区別する。**

各sourceのManifestにテスト成功とあっても、それを統合後の成功として再利用しない。source OIDと報告対象が対応しているか確認し、最終候補`R`で必要な検証を実行する。

各sourceが独立に通っていたテストが、他sourceの追加後にも通ることを確認する。これはテストの範囲内での回帰チェックであり、すべての振る舞いの証明ではない。

選択されたsource同士が補完関係にあり、単体では未成立でも集合では成立する場合は、独立テスト未成立と最終集合の成功を区別して記録する。単体の未検証を隠さず、最終集合の要件充足に必要な確認を行う。

### 8.3 テストを弱めて成功にしない

**A-26［合意＋提案］実装意図を壊して緑にする行為を禁止する。**

テスト削除、skip追加、期待値の弱体化、test selectorの縮小、CIやlint設定の無効化によって失敗を隠さない。同じテスト名や登録箇所の衝突で、片方のテストが発見されなくなるケースも確認する。

仕様上正当なテスト更新が必要な場合は許可するが、どの選択要求に基づく変更か、従来の振る舞いを保持／変更する理由は何かを記録する。判断できなければ保留する。任意言語の全assertionをEVKが意味解析するエンジンは不要。

### 8.4 検証対象の固定と記録

**A-27［提案］最終候補`R`を確定してから検証し、検証後にコードを変えない。**

コミットされていない修正を含めてテストしたまま、別内容をcommitして反映してはいけない。最終的に反映するcommitと検証対象commitを一致させる。検証後に修正やコード生成で管理対象ファイルが変わった場合は、新しい`R`を確定して必要な検証をやり直す。

実行結果として最低限、対象OID、コマンド、作業ディレクトリ、終了コード、必須／任意、実施／未実施／skip、ログ参照を残す。既存プロセス記録で満たせれば専用テストDBは作らない。

エージェントの「全テスト成功しました」という文章だけで、EVKが確認した実行成功として扱わない。既存実行ログで確認できる部分と、AIが自己報告した部分を区別する。任意のシェルコマンドの終了成功だけでも、必要なテスト範囲を十分検証した証明にはならない。

最小案は、確定Rに対する必須planを既存Script／ExecutionProcess経路でhostが実行し、exit code・ログ・前後HEADを照合すること。現在のScriptContextに統合検証の用途を小さく追加できる。AgentのNative Auditを使う場合も同じplan・OID・終了結果へ確実に対応付け、自己申告の`MemoryTest`だけでは代用しない。汎用の新しいtest runnerや評価DBは作らない。

検証開始前・終了後・publication直前にHEADと関連する追跡／未追跡変更を確認する。通常終了時のMemory finalizer、formatter、追加follow-up等がR後にsourceを変えた場合も同じ失効条件を適用する。合格後にRとは異なるR'を自動commitして反映してはいけない。検証がsourceを変更する必要があるなら変更を確定した新Rで再検証する。

**A-28［提案］Git以外の共有状態も検証条件に含める。**

テスト用DB、ポート、データディレクトリ、共有キャッシュが通常開発と干渉しないよう、既存の環境分離・run別設定を使う。本番環境への操作、deploy、push、認証設定の変更は本機能の暗黙の権限にしない。

worktreeが別でも外部副作用は取り消されない。共有資源を安全に使う方法がなければ、その検証を黙ってskipせず保留する。

## 9. ターゲットへの最終反映

### 9.1 公開条件

**A-29［提案］反映直前に、次を機械的に再確認する。**

| 確認           | 成功条件                                                             |
| -------------- | -------------------------------------------------------------------- |
| 選択集合       | 最初に承認された集合と一致。未選択の追加・欠落がないと調査結果に記録 |
| source         | 固定commitが維持され、対象の更新・dirty化・新たな書き込み実行がない  |
| カード要求     | 完了対象の要求・採用Workspace・Cardリンクが選択時から変わっていない  |
| 統合候補       | `R`が確定しており、作業環境に未反映の管理対象変更がない              |
| 検証           | 固定した必須planを満たす実施記録が`R`に対応している                  |
| target         | 作業基点`B`から変わっていない                                        |
| 実行状態       | Runがキャンセル・保留・失効しておらず、実行権を保持している          |
| target作業環境 | checkout済みの場合、対応worktree・indexの安全な更新条件を満たす      |

意味的な選択集合の保持はAI判断、OID・dirty・予約・実行記録等は機械的確認という区別を保つ。

### 9.2 最終反映の原則

**A-30［提案］ターゲットを検証済み`R`へ進める。反映時に未検証の新しい統合を行わない。**

統合はtarget基点`B`の上に行い、最終反映は基本的に`B → R`のfast-forwardとする。targetへ反映する段階でもう一度意味的mergeを行い、検証していない別内容を作らない。`--ff-only`はfast-forward不能時に拒否するための手段だが、単独では「想定した旧OIDのまま」の確認に代わらない。[Gitの一次資料G2](https://git-scm.com/docs/git-merge)

expected old OIDを指定する参照更新等を使い、外部更新を検出できる設計にする。[Gitの一次資料G3](https://git-scm.com/docs/git-update-ref)

現行`merge_workspace`はsource準備、`merge_changes`、Merge記録、非同期remote Done、archiveを行う。`merge_changes`はsquash commitとsource ref書換えを行うため、このAPIをRのpublicationとして呼ばない。既存Git crate内に、期待値B・確定R・target identityを受ける小さな反映操作を追加する。BがRの祖先であることを検証し、source refsは変更しない。既存のmanual mergeの意味は保ち、共通lock等の必要部分だけを共有する。

**A-31［提案・実装前P0確認］checkout済みtargetではrefsだけ更新しない。**

`git update-ref`相当の操作だけでbranchを動かして、既存worktreeのindex・実ファイルを古いまま残してはいけない。現行の`update_ref`にはexpected-old引数がなく、既存squash経路のdirty事前確認も本契約には不足する。exact-R反映・expected-B照合・worktree整合を一緒に扱う追加helperを既存Git crateへ設け、既存のGit実行／worktree列挙を再利用する。

| targetの利用状況                                     | 初版の方針                                                                      |
| ---------------------------------------------------- | ------------------------------------------------------------------------------- |
| どのworktreeにもcheckoutされていない                 | expected old OID付き更新を候補とする                                            |
| 管理下のcleanなworktreeにcheckout済み                | 追加する検証済みRの反映経路でexpected-Bを確認し、index・実ファイルもRへ更新する |
| dirty／merge途中／別の書き込み処理がある             | 保留する。自動stash/resetで解消しない                                           |
| 管理外のworktreeで利用中、または安全性を判断できない | 初版では保留を許容する。対応済みと表示しない                                    |

EVK内の予約とGitのlockは、外部プロセスのあらゆる並行操作を停止する保証ではない。対応範囲外の同時外部書き込みを検出した場合には、強制更新せず確認を求める。

clean判定はstagedだけでなくunstaged、衝突するuntracked、merge/rebase途中、実行中writerを含む。Git ref更新とworktree更新をSQLiteと原子的に扱えるとは仮定しない。途中障害ではref・index・実ファイルの実状態を照合し、整合が確認できるまでDone化せずA-40の復旧対象とする。ユーザー変更をreset/stashで消して整合させない。

### 9.3 targetが進んでいた場合

**A-32［提案］古い検証結果のまま反映しない。**

最終反映前にtargetが`B`から変わっていたら、ターゲットを変更せず`TARGET_CHANGED`で保留する初版案を基本とする。ユーザーの再試行で新しいtargetを基点に統合・検証し直す。自動追従は将来可能だが、必ず検証をやり直し、無限再試行にしない。

`R`がすでにtargetへ反映されたのか、それとも別操作でtargetが変わったのかは、復旧節の記録と照合する。targetを元の`B`へresetして再実行することは禁止する。

OpenWiki publication等の正規操作であっても、R反映前にBが変われば同じ検出対象とする。反映後に既存SyncがWiki-only commitを追加することは正常であり、targetがRの子孫になっただけで過去のsource反映を失敗へ戻さない。

## 10. 状態・キャンセル・保留

### 10.1 論理状態

**A-33［提案］カード状態とは別に実行状態を持つ。**

名称は既存ジョブモデルに合わせてよい。少なくとも以下の意味を区別する。

| 状態              | 意味                                         | 通常のキャンセル       |
| ----------------- | -------------------------------------------- | ---------------------- |
| queued            | 対象が予約され、実行待ち                     | 可                     |
| preparing         | 作業環境と入力の確認                         | 可                     |
| integrating       | AIによる統合・修正                           | 可                     |
| validating        | 最終候補の検証                               | 可                     |
| publishing        | 最終反映の確定区間                           | 不可                   |
| succeeded         | target整合と必要な完了後処理を確認済み       | 不可。撤回は別タスク   |
| blocked           | 意味的判断・検証条件・変更競合等で反映を保留 | 終了済み               |
| failed            | 起動失敗・実行基盤障害等                     | 終了済み               |
| cancelled         | 反映前に停止済み                             | 終了済み               |
| recovery_required | 反映の有無や実行所有者を確定できない         | 通常操作で再反映しない |

Cardの表示には「統合待ち」「統合中」「保留」「反映済み」等を返し、同じRunの画面へリンクする。新しい列を増やす必要はない。

Git反映状態、Cardごとの完了処理状態、Wiki Sync状態は別々に記録する。Git反映済み・Card完了保留を未反映や全件Doneと表示しない。Wiki SyncはA-43の独立した後続処理であり、その失敗だけでsource Integrationを未反映にしない。これらはIntegration固有の状態であり、通常AgentRunや既存Workflow全体へ同名enumを一律追加する要求ではない。

### 10.2 キャンセルの境界

**A-34［合意＋提案］キャンセルはRun単位の操作。**

queuedでは予約を解除する。実行中はエージェント・検証のプロセスと継続起動を止め、停止を確認してから予約を解除する。失敗調査のため統合用Workspaceとログを残してよい。

publishingへの移行とキャンセル受理はサーバー側で競合解決する。先にキャンセルが成立したなら反映しない。先にpublishingへ入ったならキャンセル不可と返す。UIのグレイアウトだけで決着させない。

タブを閉じる、通信が切れる、ボードを移動することはキャンセルと同義にしない。状態は再接続後に確認できる。

### 10.3 保留理由

**A-35［合意＋提案］拒絶は説明付きの保留として扱う。**

最低限、理由、関係カード／ファイル、実施済み作業、未実施検証、ターゲットを反映していないこと、再開に必要な操作を表示する。

理由コードの候補は `SEMANTIC_CONFLICT`, `UNSELECTED_DEPENDENCY`, `REQUIREMENT_UNCLEAR`, `VALIDATION_FAILED`, `VALIDATION_UNAVAILABLE`, `SOURCE_CHANGED`, `TARGET_CHANGED`, `TARGET_WORKTREE_BUSY`。実際のenumは既存モデルに合わせる。

単なるテキストコンフリクトはAIの解決対象とする。選択要求同士の矛盾や、未選択の基盤変更が必須と判明した場合などは保留する。AIに何でも解決した体裁を要求しない。

## 11. 完了・再開・撤回

**A-36［合意＋提案］Git反映を確認してからカードをDoneにする。**

カード要求と採用成果のrevisionを照合し、条件を満たすカードだけ完了更新する。通常経路では予約により変更を防ぎ、全対象をDoneにできる状態にする。

外部操作等で反映直後にsourceやカード要求が変わった場合は、反映済みの事実を消さず、カード完了だけを保留して理由を示す。Gitの成功とUI更新の不整合を一つの「merge失敗」に潰さない。

local Boardでは既存`update_local_issue`の無条件更新をそのまま使わず、予約・要求snapshot・採用／リンク・期待する状態を条件付きで照合する。remote Boardを対象にする場合も同等の契約が必要であり、既存の非同期remote sync成功を推定して済ませない。Cardごとの適用／既適用／条件不一致／再試行待ちを永続化し、後処理再送で利用者が再開・再リンクしたCardを上書きDoneにしない。Gitを再反映してDB不整合を直すことは禁止する。

**A-37［合意］Done→In progressでGit操作を発火しない。**

再開後に新たなcommitができたら、その新しい成果を別Runで統合できる。過去の統合履歴は保持する。同じcommitを再投入して二重反映することは避けるが、Cardが過去に一度Doneになったことを理由に新しい成果を排除しない。

**A-38［合意＋提案］反映後の撤回は新しい変更として扱う。**

初版では自動Undoを作らない。必要なら、統合結果への参照と撤回したい範囲を持つ通常カードを作り、最新target上で影響を調査する。過去のCardを戻すだけでrefを巻き戻さない。

Gitのrevertは新しい打ち消し変更であり、特にmergeのrevertは後続mergeにも影響する。ワンクリックの「過去をなかったことにする」操作として隠さない。[Gitの一次資料G4](https://git-scm.com/docs/git-revert)

## 12. 実行記録・冪等性・クラッシュ復旧

### 12.1 保存する最低限の意味

**A-39［提案］実装に必要な実行記録を既存機構へ寄せる。**

| 論理情報                                                             | 必要な理由                                   |
| -------------------------------------------------------------------- | -------------------------------------------- |
| run_id・重複投入識別子                                               | 二重クリック／リトライによる重複実行を避ける |
| source集合・固定OID・Issue保存先／要求／関連付けのsnapshot・event ID | 何を完了させる操作だったか再現する           |
| target repo/ref・基点`B`・最終候補`R`                                | 検証と反映の対応を確認する                   |
| Workspace／Session／実行プロセス参照                                 | 既存ログ・停止・再開・cleanupと連携する      |
| 状態・所有者・キャンセル要求・必要な時刻                             | 再起動後に二重workerを起こさない             |
| 固定した検証plan・結果参照・AI判断・保留理由                         | 成功・不成功の根拠を残す                     |
| publish意図・反映結果・カード更新結果                                | Git更新とDB更新の間の障害を照合する          |

すべてを新テーブルにする必要はない。既存のexecution recordや状態保存を流用する。揮発キャッシュだけにpublish記録を置かない。

現行Orchestrationのpersist-before-dispatch、outbox/inbox、command identityを再利用する。product kindは現在Workflow/Arenaであり、必要ならIntegration種別と固定plan／結果payloadを最小追加する。汎用scheduler、独立DAG、第三の監査ログを新設しない。一方、既存enumやpayloadで表現できない情報を「既にある」とも扱わない。

`reconcile_startup`が削除するdispatcher leaseは起動作業用で、Agentの全実行期間やGit publicationの業務予約を保護しない。予約は永続Runの状態から照合・復元し、host／子process／queued continuationの実状態が不明なら新しいwriterを起動しない。既存の一時的leaseを延命するだけでクラッシュ整合を保証しない。

### 12.2 GitとDBの間の障害

**A-40［提案・P0］反映前にpublishの意図を永続化し、再起動時に照合する。**

GitとアプリDBを一つの原子的トランザクションとして扱わない。少なくとも`B`、`R`、run_id、canonicalなGitストレージ／target ref、対応worktree、固定source集合、検証証明参照、反映開始状態をGit操作前に永続化し、実状態と比較して復旧する。既存Memory integration intent／marker回復の考え方を流用できるが、単一Workspace用のsource統合記録だけでCard完了やexact-Rの証明を代用しない。

| 再開時の状況                                       | 方針                                                                                           |
| -------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| targetが`B`のまま、反映記録なし                    | 反映済みとしない。実行状態・検証の有効性を確認してから明示的に再開                             |
| targetが`R`、対応するpublish意図がある             | 再mergeせず、必要なworktree/index整合も確認してDB／カード後処理を進める                        |
| targetが`R`の子孫、publish記録との整合を確認できる | 後続の変更を巻き戻さず、必要な完了処理だけを照合する                                           |
| targetが無関係な状態、証拠が不足                   | recovery_required。自動reset・自動再反映をしない                                               |
| refは`R`だがworktree/index更新が未完了・不明       | Git反映の事実を保持しつつrecovery_required。ユーザー変更を破棄せず、安全性確認までDoneにしない |
| worker停止後も子プロセスが残っている               | 先に所有権・停止状態を確認し、二重統合を始めない                                               |

`R`がtargetの祖先であるだけで、このRunが反映したという因果を断定しない。意図記録・結果・実際の状態を合わせて判断する。アプリ再起動が起き得る自分自身のEVK開発でも、この復旧を検証する。

**A-41［提案］二重クリックと同じ成果の再投入を安全に扱う。**

同じ開始要求の再送は既存Runを返す。別要求でも予約中の同じCard／Workspaceは競合として扱う。既に取り込み済みのsourceはGit履歴と既存統合記録等で判定する。squash・手動取り込みなどで判断できなければ、未統合／統合済みを推測で確定しない。`R == B`で差分が不要な場合も、要求充足と検証・完了条件を確認する。記録用の空commitを強制したり、検証後にmarkerだけの別commitを足したりしない。

### 12.3 保存とcleanup

**A-42［提案］実行中・復旧待ちの作業場所をcleanupから保護する。**

source、統合Workspace、保持commit、Sessionログ、Run記録の寿命を既存cleanupと整合させる。成功後も、何を統合したかと検証結果への参照は保持する。初版でsource branchを自動削除しない。

現行`find_expired_for_cleanup`のactive ExecutionProcess判定だけでは、queued／publishing／復旧待ちのIntegrationを保護できない。Runの業務予約をcleanup／archive／削除のguardにも使う。pinnedの表示やprocess停止だけを根拠に参照先を消さない。

保存期限・容量管理は既存方針を優先し、「共有領域なので永続バックアップ済み」とは扱わない。cache削除で反映履歴を失う構造にしない。

## 13. 統合結果を次の開発へ戻す

**A-43［合意＋提案］結果も既存Manifestの仕組みに載せる。**

Memory有効時は、統合用Sessionが追加した競合解消・互換修正・テスト・判断を既存draft／Manifest writerで記録する。sourceのevent本文をコピーして別eventへ再発行せず、選択したevent IDと統合固有のeventを参照する。Runの結果として、source OID、target、`B → R`、検証ログ、判断理由を発見可能にする。Memory無効時にも必要なGit反映・完了記録はOrchestration側へ永続化する。

Manifestの生成成功とGit反映成功を混同しない。反映前に必要な記録の保存条件を確認し、反映後の補助文書生成に失敗しても「Git未反映」とは表示しない。再生成可能な補助表示と、失ってはいけない反映記録を分ける。

既存`prepare_integration`は単一Workspaceのeventを扱い、固定した複数source／targetごとの選択契約ではない。共通storeとwriterを再利用し、固定source OIDに帰属する明示event集合を入力できる最小拡張を行う。source commitの到達可能性等で対応を確認し、選択外Workspace、固定OID後の新event、別targetの統合済み判定を混入させない。共有された祖先eventは重複させず、欠損は理由を残す。壊れた必須記録を黙って除外してpublicationを通さない。

設定targetへの反映後は、既存のintegration確認→pending event→自動OpenWiki Sync／publicationを維持する。「Wiki自動更新を新設しない」は既存の自動更新を止める意味ではない。通常Workspaceは引き続き`openwiki/`を直接変更しない。Sync失敗時はpendingとエラーを残し、source反映やDoneをGit Undoしない。

Memoryは現在repositoryごとに一つの設定Wiki targetを持つ。別targetへのIntegrationを「設定Wiki更新済み」と表示せず、今回の機能で多branch Wiki管理を新設しない。source反映・Card後処理・Wiki Syncの状態を区別する。入口bootstrapはこの結果を参照し、統合後の方針と各worktreeの参照版を判断する。元Sessionへのlive通知は必須にしない。

## 14. エージェント向け統合指示の案

**A-44［提案］指示の核心は以下。実際の入力は既存prompt組立へ載せる。**

```text
あなたはEVKの統合担当です。選択された成果物を、指定された統合用作業環境で統合してください。

入力:
- 選択されたCardの要求と採用Workspace・固定source OID
- 指定targetと今回の基点B
- Wiki、Change Manifest、既存検証規約への参照
- 固定sourceに対応するevent集合と、hostが保持する必須検証planへの参照
- このRunの作業場所・権限境界・検証条件

役割:
1. カード要求、Manifest、実際の差分を読み、選択集合とその祖先に含まれる変更を確認する。
2. 必要な順序で統合し、両方の意図を保ってテキスト・意味的コンフリクトを解決する。
3. 必要な互換修正と横断テストを統合用branchに追加する。
4. 最終候補Rをcommitした後、既存回帰テスト・各成果のテスト・必要な全体検証を実施する。
5. 結果、実行した検証、テストの変更理由、未検証事項を記録する。最終反映はhostがRと検証記録を確認して行う。

禁止:
- source Workspaceやsource branchを変更しない。sourceはReadOnlyな入力として扱う。
- targetを直接更新しない。最終反映はEVKの処理に委ねる。
- 未選択成果を無断で追加せず、選択成果を無断で除外しない。
- テスト削除・skip・期待値の不当変更・設定弱体化で緑にしない。
- 無関係なリファクタ、push、PR、deploy、破壊的reset/stashをしない。
- 必須検証の未実施や不明な点を、成功・確認済みとして報告しない。
- openwiki/を通常の統合担当として直接更新しない。既存Manifestと統合後のOpenWiki Syncへ委ねる。
- 合格後に別commitを作らない。変更が必要なら新しいRとしてhostの検証をやり直す。

保留:
仕様上の矛盾、必須の未選択依存、要求の重大な不明点、検証環境不足等で安全に完了できない場合、
必要な判断と理由を具体的に記録し、targetへは反映せず終了する。
通常のファイルコンフリクトだけを理由に諦めない。解決可能なものは統合用環境で解消する。
```

修正・再テストの試行には既存の実行予算・停止機構を使う。無制限の自動修復ループを作らない。試行上限やコスト上限の新設が必要なら、既存executor設定との関係をレビューで決める。

## 15. 受入テスト

### 15.1 ユーザー操作と対象選択

| ID     | シナリオ                                                     | 期待結果                                                                     |
| ------ | ------------------------------------------------------------ | ---------------------------------------------------------------------------- |
| AT-01  | 3カードを選び１回開始                                        | １Run・１統合Workspaceを作り、全対象へRunリンクを返す                        |
| AT-02  | １Cardに複数Workspace                                        | 採用成果を明示させ、最新を勝手に選ばない                                     |
| AT-03  | Workspaceに複数Session                                       | Sessionごとの重複mergeをしない                                               |
| AT-04  | sourceがdirty／継続goal・queued起動・Memory source確定処理中 | 理由付きで投入を止め、勝手にstashやcommitしない                              |
| AT-05  | In progressだが作業停止済み                                  | 条件を満たせば選択できる                                                     |
| AT-06  | 手動Doneだが未統合                                           | Doneを根拠に取り込み済みと誤判定しない                                       |
| AT-07  | 未選択Aを祖先に含むBを選択                                   | 潜在的なA取り込みを明示し、無断で範囲を広げない                              |
| AT-08  | 別repoにも変更のあるWorkspace                                | 片方だけ反映してCard全体を無条件Doneにしない                                 |
| AT-08A | Aの通常Sessionから「Bと合わせてテスト」                      | BをReadOnly sourceとしてA+Bを検証し、target/Card status/B branchを変更しない |
| AT-08B | A+B成立にBの実装修正が必要                                   | Bを直接編集せず、必要修正・根拠・該当テストを示して保留する                  |
| AT-08C | 「必要ならBも修正して」と指示                                | Delegation未実装の初版ではBへ直接書かず、制約と必要作業を報告する            |

### 15.2 統合と検証

| ID    | シナリオ                           | 期待結果                                           |
| ----- | ---------------------------------- | -------------------------------------------------- |
| AT-09 | 同一ファイルへ両立可能な2機能      | AIが統合し、両方の受入テストが通る                 |
| AT-10 | 同じAPIに相容れない仕様            | 片方を捨てず、理由付きblocked                      |
| AT-11 | A/B/CのうちCが統合不可             | A/Bだけをtargetへ反映しない                        |
| AT-12 | 未選択Dが必須                      | 勝手に追加せず、追加選択を提案して保留             |
| AT-13 | 各sourceは成功、統合後に回帰失敗   | 修正・再検証するか保留。targetは未反映             |
| AT-14 | テストをskipして成功報告           | 未実施を成功にせず、必要検証を満たした扱いにしない |
| AT-15 | 同名テストや登録競合で片方が消える | テストの範囲欠落を評価ケースで検出する             |
| AT-16 | テスト後にコードを変更             | 既存の成功記録を使い回さず新しいRで再検証          |
| AT-17 | 必須テスト環境不足                 | VALIDATION_UNAVAILABLE。全テスト成功と表示しない   |
| AT-18 | テスト用DBやポートが他開発と衝突   | 外部状態を壊さず保留／既存の分離設定で実行         |

AT-09〜AT-15の意味的判断は固定fixtureによるエージェント評価を含む。プロンプトを追加しただけで常に成立すると主張しない。

### 15.3 Git・並行操作・キャンセル

| ID    | シナリオ                                   | 期待結果                                                                             |
| ----- | ------------------------------------------ | ------------------------------------------------------------------------------------ |
| AT-19 | 同じtargetへ2Run                           | 直列実行し、後続は実行開始時のtargetを基点にする                                     |
| AT-20 | 同じGitストレージを別repo IDで登録         | 別のロックにすり抜けて同時反映しない                                                 |
| AT-21 | 統合中に既存manual merge／Wiki publication | 同じ短いGit反映guardを使う。Bが進めば古い統合候補はTARGET_CHANGED                    |
| AT-22 | 外部操作でtargetが進む                     | 古いBを前提に反映せず、再試行が必要と表示                                            |
| AT-23 | sourceが外部操作で変わる                   | 未確認の新HEADを取り込まず、元Cardを誤って完了しない                                 |
| AT-24 | queued／integratingでキャンセル            | 対象targetを変更せず、プロセス停止後に予約解除                                       |
| AT-25 | キャンセルとpublishingが同時               | サーバー側で一方だけ成立。二重の結果表示をしない                                     |
| AT-26 | cleanなtarget worktreeへ反映               | refだけでなくindex・実ファイルもRと整合                                              |
| AT-27 | dirtyなtarget worktreeへ反映               | 保留し、利用者の変更を消さない                                                       |
| AT-28 | エージェントからtarget直接更新を試みる     | 強隔離モードなら拒否。協調モードなら保証外であることを明示し、可能な範囲で変更を検出 |

### 15.4 永続化・復旧・既存機能

| ID    | シナリオ                            | 期待結果                                                                  |
| ----- | ----------------------------------- | ------------------------------------------------------------------------- |
| AT-29 | 開始ボタン二重押し・HTTP再送        | 重複Run／重複mergeにならない                                              |
| AT-30 | R反映後、Done更新前にクラッシュ     | ref/index/worktreeとintentを照合し、再mergeせず条件付きのCard後処理へ進む |
| AT-31 | 復旧時にtargetがRの先へ進んでいる   | 後続成果を巻き戻さない                                                    |
| AT-32 | 旧workerの子プロセスが残存          | 新workerとの二重処理を防ぎ、必要ならrecovery_required                     |
| AT-33 | DoneをIn progressへ戻す             | GitのUndoが発火しない                                                     |
| AT-34 | 再開して新commitを作る              | 過去の統合記録を保持したまま新しいRunに投入できる                         |
| AT-35 | remote／GitHub認証なしのlocal Board | source反映とlocal IssueのDoneが成立する。remote未対応なら開始前に区別     |
| AT-36 | 既存手動PR／mergeを使用             | 操作は維持され、Auto Mergeの必須化による回帰がない                        |
| AT-37 | cleanup実行中にRunが存在            | 必要なsource／統合Workspace／ログを削除しない                             |
| AT-38 | 後続の新規カードを開始              | 統合結果をbootstrapから発見でき、元の未統合方針と区別できる               |

### 15.5 コード照合で追加した回帰条件

| ID    | シナリオ                                                  | 期待結果                                                                                      |
| ----- | --------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| AT-39 | 検証済みRを正式反映                                       | targetはexact R。追加squash／source ref書換え／source自動archiveなし                          |
| AT-40 | 合格後にfinalizer／formatter／follow-upがsourceを変更     | 既存合格を失効し、新Rで再検証するまで反映しない                                               |
| AT-41 | targetを開くDirect-folderから「Bとテストだけ」            | 一時環境を使い、target／B／Card状態を維持。検証用変更のsource commit・統合eventを誤発行しない |
| AT-42 | 3CardのRunを表示                                          | 各Cardから同じRunへ辿れ、sourceの既存Workspaceリンクを上書きしない                            |
| AT-43 | 反映後のDone再送前にCardを再開／再リンク／要求変更        | 適用済み結果は冪等。期待条件不一致は完了保留とし、新しいユーザー操作を上書きしない            |
| AT-44 | 再起動でdispatcher leaseが消え、子Agentが残存             | 業務予約をRunから照合し、二重起動・cleanupを防ぐ                                              |
| AT-45 | 予約取得とgoal継続／queue消費／rebase／リンク変更等が競合 | 共通mutation guardで片方だけ受理し、UI外APIからも予約を回避できない                           |
| AT-46 | 自己報告だけ成功／plan欠落／別Rのログ／必須commandの置換  | 合格としない。規約上対象外のprivate remote Cargoは理由付き除外と区別する                      |
| AT-47 | A/Bを固定後、Cや新しいA eventが追加                       | 固定OIDとtargetに対応するeventだけ統合。破損を黙って落とさず、後続eventを消費しない           |
| AT-48 | 設定targetへ反映後、既存Wiki Syncが成功／失敗             | 既存自動更新が維持され、先行知識を保持。失敗はpendingで再試行し、Rを取り消さない              |
| AT-49 | R == B、選択成果が既に含まれる                            | 空commitや再mergeを作らず、必要な検証と条件付き完了は省略しない                               |
| AT-50 | 途中障害でrefだけR、worktree/indexは不整合                | recovery_required。成功・Doneと偽らず、ユーザーのdirty変更を破棄しない                        |

これらは規範となる受入条件であり、個別の検証方法・結果は[実装記録](parallel-integration-implementation.md#requirement-by-requirement-acceptance-audit)で区別する。通常Sessionの終了処理、既存manual merge、OpenWiki Sync、goal／queueの回帰を、変更する共通境界のテストにも含める。

## 16. 推奨実装順序

1. レビュー基準点と現在差分を照合し、第2.2節の対応範囲を決める。既存OpenWiki更新一本化を維持する。
2. 入口の文面・機械的一覧と、固定OIDをReadOnly sourceとして読む最小経路を接続する。通常Sessionの試験専用と成果を残す終了処理を区別する。
3. 既存Git crateでexpected-B／exact-R反映を追加し、source不変、checked-out target、競合・途中障害をAgentなしのテストで確認する。既存squash APIは置換せず残す。
4. 既存Orchestrationへ固定集合・業務予約・検証plan・publication intentを最小追加する。cardless Workspaceと１Sessionを使い、統合・検証を先に成立させる。
5. 反映・条件付きDone・再起動復旧・cleanup guardを接続する。ここまで不足した状態を自動統合完成とは扱わない。
6. Boardの明示選択、進捗・停止・保留・後処理表示を接続し、既存Manifest／OpenWiki Syncへ結果を渡す。通常開発と手動merge／PRの回帰を確認する。

段階的な「統合・検証だけ」は開発用の到達点であり、完成版UXを毎回手動承認へ変更する意味ではない。Auto Merge列、定期バッチ、自動集合選択、auto PR、Delegation/MCPは初版に入れない。

## 17. 実装担当が結論を出すべき事項

| 論点                            | 求める結論                                                                                                |
| ------------------------------- | --------------------------------------------------------------------------------------------------------- |
| 対応Board／multi-repo／保証範囲 | 第2.2節の最小案を採用するかを決め、制限とUX差分を明示する                                                 |
| cardless Workspaceの実行目的    | 作成経路は存在する。試験専用／正式Integrationと通常finalizerを区別する最小設定を決める                    |
| exact-R publication             | 現行squashは流用不可。既存Git crate内で追加するhelper、expected-B照合、checked-out targetの復旧を決める   |
| cross-workspace ReadOnly        | 固定object参照と一時worktreeを既存経路へ接続し、peer再作成や他Workspace Memory参照を避ける                |
| 将来Delegation                  | Workspace所有Agentへ修正依頼を渡す場合に、既存Session/queue/MCPのどこを再利用できるか。初版では実装しない |
| 実行記録・予約                  | Orchestration種別／payload／DB制約への最小追加と、dispatcher leaseと独立したmutation guardを決める        |
| source固定とCard完了            | Issue要求snapshotとリンク、他Session活動を条件付き照合し、Card別後処理を冪等化する                        |
| 検証結果                        | 既存Script経路を基本に、plan・exit・ログ・前後HEAD・Rの対応を検証する。自己報告と区別する                 |
| 書き込み隔離                    | A-19のどちらの保証範囲を実現するか。unrestricted実行での限界                                              |
| checked-out target              | refs・index・実ファイルの一貫性を保つ具体的経路                                                           |
| 再起動復旧                      | Git反映前後の証拠をどこへ永続化し、どう照合するか                                                         |
| OpenWiki接続                    | 固定batch event集合と統合固有のdraftを既存writerへ渡す最小拡張を決める。通常Syncは維持                    |
| 型・設定・UI                    | 生成型、DB移行、既存設定version、国際化、テストの既存流儀                                                 |

この文書の論理状態やフィールドをそのまま大量のテーブル・APIへ変換しない。一方、必要な永続情報や条件付き更新を「新DBを避ける」ために揮発状態へ落とさない。合意したUXを保ち、実在する機構と追加する契約の境界を実装記録へ残す。
