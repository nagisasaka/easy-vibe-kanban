---
title: "上流改善の選別移植 — 実行・入力・設定・操作性"
description: "LVKの既存契約を維持し、固定したEVK上流から五段階で改善を移植するための実装・検証・ブランチ運用仕様。"
---

## 1. 目的と完了の単位

LVK（このeasy-vibe-kanban fork）の製品モデルを維持したまま、上流EVKの実行・入力・設定・編集・操作性の改善を選別して取り込む。上流のコミット数、変更行数、新UIへの一致率を達成指標にしない。

第1〜第5弾を一つの実装ゴールとして進める。各段階で調査、実装、回帰テスト、レビュー、必要な修正、ローカルコミットを行い、その成果を含む次のブランチへ進む。途中のmainへの取り込みは不要。最後に全段階を含むコード状態で自動検証とChrome DevTools MCPによる実機受入を行う。

本書は実装要求であり、移植済み・検証済みの報告ではない。本書の作成だけでは実装、コミット、サーバー再起動、実モデル試験を開始しない。以下の作業権限は、本書を参照する実装依頼を受けた後に適用する。

## 2. 固定した調査基準

仕様作成日: 2026-09-22。調査時の参照点は以下。

| 対象                                  | 固定値                                     |
| ------------------------------------- | ------------------------------------------ |
| LVK作業ブランチ                       | `fix/merge-upstream-test`                  |
| LVK HEAD / ローカルmain / origin/main | `39bbba0f4238d08cdbc203defe20dae03aa90368` |
| LVK HEADのcommit日時                  | `2026-09-21T19:14:51+09:00`                |
| 上流repository                        | `toby1123yjh/easy-vibe-kanban`             |
| 上流参照SHA                           | `76a86da903a931e7cd290d1d4b26e9b4304794d9` |
| 上流commit日時                        | `2026-09-21T17:43:58+08:00`                |
| 共通祖先                              | `0b4ca341eac59e6f27e296e716bb2af8283e0d43` |
| 上流側の対象差分                      | 共通祖先から42コミット                     |
| 仕様追加前の作業ツリー                | クリーン                                   |

仕様作成時に`git ls-remote upstream refs/heads/main`で上流参照SHAとの一致を確認した。これは将来の実装開始時点の最新性を保証しない。実装時にbranch、HEAD、mainとの差分、未commit変更を再記録する。上流が進んでも対象を自動拡大せず、本書の固定SHAを用いる。

調査はコード・テスト定義・Wikiの静的確認であり、上流のtest suite、障害再現、性能測定の合格証拠ではない。コミット途中の状態ではなく、固定SHAでの最終状態を確認する。例えばフォルダー選択は途中コミットの名称と最終挙動が一致しない。

## 3. 最初に読む資料と実装の接続点

適用される`AGENTS.md`と本書を全文読む。下表のWikiは探索の入口であり、現在の動作はソース・テスト・実設定を優先する。対応する既存仕様のうち変更する契約を読む。過去の実装仕様にある「未実装」「最新attemptのみ」等を、現在も成立する事実と仮定しない。

| 領域                          | LVKの接続点                                                                                                                                                       | 対応する知識・契約                                                                                                                                                                                                                                       |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 状態配信・履歴                | `crates/server/src/routes/agent_runs.rs`、`crates/services/src/services/agent_runtime.rs`                                                                         | [Agent Runtime](../../openwiki/architecture/agent-runtime.md)、[データ経路と回復](../future/agent-runtime/data-planes-and-recovery.md)                                                                                                                   |
| Host・provider・永続化        | `crates/local-deployment/src/process_host.rs`、`agent_run_port.rs`、`crates/db/src/models/agent_runtime.rs`、`crates/executors/src/executors/provider_adapter.rs` | [SessionとAgentRun](../../openwiki/concepts/session-and-agent-run.md)                                                                                                                                                                                    |
| チャット・Scratch             | `packages/web-core/src/features/workspace-chat/`、`shared/hooks/useScratch.ts`、`crates/server/src/routes/sessions/`                                              | [ScratchとGoal](../../openwiki/concepts/session-and-agent-run.md)、[Web同期](../../openwiki/architecture/web-and-sync.md)                                                                                                                                |
| 設定・Host・CLI               | `crates/executors/src/agent_settings.rs`、`agent_tools.rs`、`command.rs`、`crates/utils/src/shell.rs`、`packages/web-core/src/shared/dialogs/settings/settings/`  | [Agent providers](../../openwiki/integrations/agent-providers.md)、[Remote接続](../../openwiki/integrations/remote-access.md)                                                                                                                            |
| Workflow編集                  | `crates/server/src/routes/workflows.rs`、`crates/workflow/src/graph.rs`、`packages/web-core/src/features/workflow/`                                               | [Workflow Runtime](../../openwiki/architecture/workflow-runtime.md)、[Workflow Attempt](../../openwiki/concepts/workflow-attempt.md)                                                                                                                     |
| 用途と所有権                  | `crates/services/src/services/workspace_usage.rs`、`crates/server/src/middleware/model_loaders.rs`、`routes/workspaces/usage.rs`                                  | [Workspace](../../openwiki/concepts/workspace.md)、[利用契約仕様](workspace-usage-contract.md)                                                                                                                                                           |
| Memory・Integration・OpenWiki | `crates/services/src/services/repository_memory.rs`、`crates/server/src/routes/integrations/`、`routes/openwiki/`、`workflow_runtime/bootstrap.rs`                | [Repository Memory](../../openwiki/concepts/repository-memory.md)、[正式Integration](../../openwiki/concepts/formal-integration.md)、[OpenWiki保守](../../openwiki/operations/openwiki-maintenance.md)、[更新一本化仕様](openwiki-update-unification.md) |
| UI・開発・配布                | local/remote-webのroute・Vite設定、`packages/ui/`、`crates/server/src/routes/frontend.rs`、`.github/workflows/publish-easy-npx.yml`                               | [Workspace inspection](../../openwiki/operations/workspace-inspection.md)、[開発・配布](../../openwiki/operations/development.md)、[サーバーコンテナー](../../openwiki/operations/server-container.md)                                                   |

上流を読む起点は、固定SHAでの上記対応ファイル、および上流に追加された以下とする。

- `crates/executors/src/profile/runtime_identity.rs`
- `crates/server/src/routes/sessions/executor_config.rs`
- `crates/local-deployment/src/process_host/journal.rs`
- `packages/web-core/src/features/agent-workbench/model/sessionDraft.ts`
- `packages/web-core/src/features/agent-center/model/agentCenterState.ts`
- `packages/web-core/src/features/workflow/model/workflowAuthoring.ts`
- それぞれのunit testsと`tests/agent-stream/`、`tests/agent-workbench/`等のfixture tests。

上流`afa92b76`はGit Import以外にRuntime等の修正を含む。`9963e043`、`7789f20b`、Agent Center関連コミットも複数責務を含む。コミット名だけで採用範囲を決めない。

## 4. 採否と実装記録

### 4.1 対象の区分

- **必須**: 要求された挙動を実装・検証する。既に同等実装がある場合は、それを確認するテストと実コードの根拠で充足でき、不要な書換えはしない。
- **条件付き**: 第5節に記載した適用条件を調べる。条件を満たすなら実装は必須。満たさない場合だけ、根拠付きで非適用にする。
- **除外**: 本ゴールでは実装しない。別途製品判断を行う候補を含む。

難しさ、作業時間、残りcontext、テスト失敗は非適用の理由にしない。必須要求と既存契約が両立しない場合は、最小の適応を検討し、それでも重大な判断が必要なら未解決として報告する。要求を黙って条件付きへ格下げしない。

### 4.2 記録

実装時に`docs/design/upstream-selective-backport-implementation.md`を作り、要求IDごとに次を記録する。新しい課題管理基盤は不要。

- 参照した上流SHA・シンボル・テストとLVKの対応箇所。
- 採用、既存実装で充足、条件不成立、未解決の区別と根拠。
- 変更内容、上流からの適応、互換性・性能・権限への影響。
- 修正前の再現、修正後の検証、実行コマンド、branch・commit、MCP証拠。
- 未実施、既知の無関係な失敗、残る制限。

## 5. 五段階の移植対象

| ID   | 段階  | 区分     | 要求                                                                          |
| ---- | ----- | -------- | ----------------------------------------------------------------------------- |
| BP01 | 第1弾 | 必須     | Canonicalイベントが空でも変化したStateを訂正し、履歴を取り残さず配信する      |
| BP02 | 第1弾 | 必須     | 送信・Queue・Goal steerの完了で、新しい下書きや別Sessionの入力を消さない      |
| BP03 | 第1弾 | 必須     | 省略DEFAULTと明示DEFAULTのruntime profile identityを同一に扱う                |
| BP04 | 第1弾 | 必須     | Sessionの実行設定を不変なlaunch情報から正しく復元する                         |
| BP05 | 第1弾 | 必須     | 差分取得失敗・未取得を変更ゼロと区別する                                      |
| BP06 | 第2弾 | 必須     | 設定応答と変更処理で機密値を保護し、保持・置換・削除を区別する                |
| BP07 | 第2弾 | 必須     | 明示Host、遅延応答、取得失敗を安全に扱い、別Hostへの誤変更を防ぐ              |
| BP08 | 第2弾 | 必須     | Workflow保存のrevision競合を検出し、付随するSession生成を原子的に扱う         |
| BP09 | 第3弾 | 必須     | ProcessHostの応答・購読・再接続を件数とbytesで有界化する                      |
| BP10 | 第3弾 | 必須     | Hostイベント購読と修復確認を組み合わせ、再接続で欠落・重複適用を起こさない    |
| BP11 | 第3弾 | 必須     | 確認済みHost終了後の未反映イベント回収と、安全な状態収束を実装する            |
| BP12 | 第4弾 | 必須     | CLI検出を改善し、設定した実行ファイルと実際に起動するものを一致させる         |
| BP13 | 第4弾 | 必須     | 上流のprovider本文・tool結果・エラー／retry解析改善をLVKの分類境界へ適応する  |
| BP14 | 第4弾 | 必須     | 未知のAPI要求をSPAのHTML成功応答へfallbackさせない                            |
| BP15 | 第4弾 | 条件付き | 初期routeから不要な重い機能が静的に読み込まれる場合、既存routerで遅延読込する |
| BP16 | 第5弾 | 必須     | Workflow編集で未保存変更・送信中の追加編集・別対象への切替を保護する          |
| BP17 | 第5弾 | 必須     | 現行Workflow editorへローカルUndo/Redoを追加または既存の同等挙動を検証する    |
| BP18 | 第5弾 | 必須     | 本移植で触れる画面のkeyboard・focusとloading/error/empty/degraded表示を整える |
| BP19 | 第5弾 | 条件付き | 現行npm発行入口がstable/betaを拒否する場合、版番号validationのみを拡張する    |

BP15の非適用は、既に必要な分割がある、または現在のentryに対象の静的依存がないことを依存解析で示した場合に限る。効果を示せない微調整は追加しない。build不能は非適用の証拠ではない。

BP19は該当発行入口と制約が残る場合に適用する。既存の`X.Y.Z-easy.N`を維持し、`X.Y.Z`と`X.Y.Z-beta.N`を受け入れ、空白・不正な数字表記等を拒否する。版番号の更新、npm tag方針の変更、発行実行は含めない。

### 5.1 第1弾: 実行・入力の正確性

**BP01**: `history_page`はStateとイベントを別タイミングで読むため、commitを挟んだ組合せを扱う。新イベントの有無だけでState送信を決めない。未配信pageがある場合はboundedなpage単位で追い付き、履歴の末尾より先に終端表示を確定しない。送信timeoutと接続終了を扱う。DB通知はwake-upの補助とし、通知欠落時も永続cursorから修復する。大きなbacklogでもStopや切断処理を飢餓状態にしない。

LVKのLive先行購読、completed全文による収束、usage snapshot再送を残す。Canonical cursorをLiveで進めない。上流のWebSocket実装をファイルごと置換しない。

**BP02**: Host／Workspace／Session／draft revisionと送信snapshotを対応させ、応答がそのsnapshotに対応するときだけ消去する。新規Sessionへの正当な選択遷移も扱う。本文だけでなく、添付、selected Skills、レビューコメント、executor設定の同時変更を検査する。新たな入力を消さず、送信済みだけを適切に解除する。送信失敗時の保持、遅いScratch保存・削除、local APIとremote localStorageの差も検証する。

通常送信、Queue、Goal steer、承認・質問の返答を混同しない。承認欄の操作で通常下書きを消さない。すべての入力操作を長時間禁止することを解決策にしない。

**BP03**: providerと非DEFAULT variantの識別を維持し、正確な`DEFAULT`の省略表記だけを同一視する。DB保存・runtime・adapterの比較箇所へ同じ規則を適用する。profileの綴りを破壊的に書き換えず、別provider・別variant・別scopeへのresumeを許可しない。

**BP04**: 対象Sessionの最新のlaunch request等、既存の不変情報を正本にする。script processや変更されたpreset、兄弟Sessionの設定を借りない。初回未起動・native session adoptionも区別する。壊れた最新設定を黙って古い設定やdefaultへ置換しない。ExecutionMode、PermissionPolicy、model、reasoning、Goal関連項目等、LVKの型にある全項目を維持する。

launch時の指定値とproviderが解決した実効値を区別する。未指定reasoningを低いeffortと表示したり、継承値をUI復元で上書きしたりしない。対応済みの`max`／`ultra`等を上流の固定選択肢で拒否する回帰を起こさない。

**BP05**: loading、成功した変更ゼロ、取得失敗、cached表示の劣化を区別する。Host／Workspace切替で前対象の差分やerrorを残さない。表示改善のためにGit比較基準や正式Integrationの検証証拠を変更しない。

### 5.2 第2弾: 設定・編集の安全性

**BP06**: 現行Agent設定・tools・profileの公開応答とconsumerを調べ、通常のdiscovery/list/diffに設定全文、credentials、機密を含み得る未知値を無条件に返さない。UI上で隠すだけでは不足。詳細取得が必要な既存機能は明示的な操作へ分離し、意味を説明する。既存の高度編集等を説明なく廃止しない。

保持・置換・削除を明示し、マスク値の保存、未知設定の脱落、外部編集の上書きを防ぐ。既存のrevision/hash、変更前後検証、atomicなfile write等を再利用する。profile適用、MCP設定、Skill管理の本番経路も対象にする。新しい秘密管理サービスや認証方式、Agent Center全画面は作らない。

共通登録とSessionごとの実行権限は別契約である。通常codingでOpenWiki writerを無効化し、Reviewerにはwriter MCP／Skillを与えず、maintenanceでは必要な登録を検証する現在のoverrideを残す。ユーザーのglobal設定をrole切替のために書き換えない。

**BP07**: 明示Hostが不明・offline・取得失敗の場合、そのidentityを保持して診断する。別Hostやlocalへfallbackして編集しない。応答・mutation後処理をHost／provider／project scopeに結び付け、古い応答で新画面を上書きしない。cached情報が読めることと変更可能であることを分ける。optionalなremote discoveryの失敗だけで無関係なlocal設定を使用不能にしない。

このfail-closed方針で緊急Cancelまで一律に無効化しない。read-only閲覧、owner Stop、設定変更は別のaction policyとする。

**BP08**: Workflowの更新に期待revision相当を導入し、競合時は409等の既存API規約に沿う明確な応答を返す。保存競合時に未保存draftを消さない。graph保存と、その保存に必要なSession生成を同じDB transactionへ収め、失敗時に孤立Sessionを残さない。

既存template・attempt・repository-scoped system runを区別する。system templateの不変性、実行開始時のgraph snapshot、LVK固有のgraph fieldsを維持する。必要な新規migrationと型生成は行うが、上流Task migrationを取り込む前提にしない。

### 5.3 第3弾: ProcessHostの配信・回復

BP09〜BP11の実装前に、現在のHostEvent、raw Audit、分類後のdurable/live/snapshot、command identity、host cursorの対応を記録する。採用する保存・flush・acknowledgement・再送方式と、server／hostのversion互換性を短い設計として確定する。

- backlogのpayloadを無制限にメモリー保持・全件複製しない。件数とbytesでpageを制限し、単一巨大eventも明示的に扱う。offset索引等が件数に比例する場合は「定数メモリー」と主張せず、残る上限と測定結果を示す。
- 購読、heartbeat、有限timeout、切断、再接続、通知lag、定期修復を扱う。cursor以降の再送とidempotencyを維持し、接続障害だけでproviderを終了済みとみなさない。
- Hostの実終了が確認できた場合だけ、durableな証拠から未反映分を回収する。PIDだけで別processを同一視しない。unreachable／生存不明は終了確定と区別する。
- journal等の最終未完了append、途中破損、sequence gap、別attemptの記録を区別する。途中破損を黙って飛ばさず、欠落した証拠で成功を捏造しない。
- 再接続でproviderを重複起動せず、ACK不明の非冪等commandを無条件に再送しない。既存commandの永続identityと監査付きCancelを維持する。
- AgentRunのterminal投影、実process終了、owner cleanup、OpenWiki completion proofは別条件である。回収した表示イベントだけでWiki成功やowner解放を確定しない。
- 既存のprotocol／registry／Auditを持つ実行について、互換な再接続・従来経路または明確な診断を提供する。古い記録を新形式だと推測しない。稼働中ユーザーrunをmigration試験に使わない。

上流`HostJournal`の丸写しは要求しない。上流はeventごとに`sync_data()`を呼ぶため、LVKでのI/O負荷と耐久性を検討する。Native Auditと同じraw payloadを第二の完全ログへ複製することや、再表示不要なLive deltaの完全耐久化は目的ではない。

保存が必要な順序・制御結果・投影情報を失わず、必要ならboundedなgroup flush等を既存Host境界で実装する。flush前に耐久化済みとしてcursorを進めることは認めない。方式・失敗窓・batch境界をテストで示す。独立schedulerや汎用event storeは追加しない。

SQLiteはDELETE journal modeと有限busy timeoutを維持する。delta単位INSERTへの逆戻り、Canonical保存とHost cursorの非原子的な更新、lock一回で永久degradedにする挙動を導入しない。Native Auditから製品DB全体を再構築する管理機能は今回の対象外。

### 5.4 第4弾: 互換性・読み込み・API

**BP12**: 現行CLI resolverへ、上流の明確なユーザー導入先探索・availability判定を適応する。明示path／commandを優先し、壊れた明示指定を別CLIへ黙ってfallbackしない。検出用pathと実起動pathを一致させる。ブラウザーを動かすWindowsと、agentを実行するcontainer／Hostを混同しない。インストール、認証、global設定変更は行わない。

**BP13**: 上流のClaude等に対するネストした本文、tool結果、API error／retryの解釈を既存provider adapterへ取り込む。上流fixtureをLVKの分類契約へ適応し、temporary retryとterminal errorを区別する。CodexのdeltaをCanonical Messageへ戻したり、intentional ignoreをProvider Extensionへfallbackさせたりしない。UIにprovider frameの独自解釈を追加しない。実資格情報を必要としないfixture検証を必須とし、未契約providerの有料実機試験は要求しない。

**BP14**: 未知の`/api`と配下routeはAPIの適切な非成功応答にし、HTMLの200やViteへのredirect loopにしない。通常SPA deep link、既知API、signed/host-scoped経路、静的assetの動作を維持する。上流の開発用frontend redirectまで採用する必要はない。

**BP15**: 現在のlocal／remote entryとrouterの依存を測定し、不要なWorkflow、Arena、editor、terminal等の読み込みを既存のcode splittingで分離する。新shell、新theme、Task APIを導入しない。Wiki／chat切替でcomposerをunmountしない現在の契約、lazy routeのloading/error、deep linkとreloadを維持する。

採用時は同じ条件で変更前後のentry依存・転送量等を記録する。開発用frontend bundleの検査は許可するが、release package／Docker release buildは行わない。改善量を未測定のまま断定しない。

### 5.5 第5弾: 選別したUI・開発支援

**BP16**: Workflowの未保存draftを、遅延fetch、保存ACK、route／Host切替で消さない。保存・破棄・編集継続を選べるようにし、失敗時は内容を保持する。revision競合を自動上書きで解消しない。system template／実行専用閲覧を編集可能にしない。

**BP17**: Undo/Redoはeditor内のgraph編集を対象にする。ノード・接続・位置・設定の編集を扱い、selected Skills、include_workflow_context、LVK固有の分岐・検証fieldsをround-tripで保持する。履歴を対象workflow／Hostごとに分離し、メモリー量を制限する。Git、AgentRun、公開済みWorkflow実行を巻き戻す機能にしない。上流の新Canvasや全authoring frameworkは必須ではなく、既存editorへの最小適応でよい。

**BP18**: 対象は今回変更するchat、設定、Workflow、差分等のsurfaceと、それらが使う共有部品。keyboard操作、focus復帰、適切なdisabled理由、読取可能なcached状態と変更不可状態を検証する。未解決状態を成功した空データへ変換しない。日本語を含む既存i18nとdesign tokensを維持し、英語を日本語訳として一括コピーしない。全画面のテーマ再設計や包括的アクセシビリティ刷新へ拡張しない。

**BP19**: 条件成立時は既存発行workflowのvalidationと説明だけを最小変更し、その実際のvalidatorを呼ぶローカルテストを付ける。テスト側へ正規表現を複製して、別の実装だけを検証しない。GitHub Actions実行、publish、package version変更はしない。

## 6. 取り込まない変更

次は本ゴールの明示的除外。実装を容易にするために依存として引き込むことも禁止する。

- canonical Taskモデルの新設・上流の一括migration、`local_workspace_links`の廃止、SessionのProject帰属固定、default Project導入。
- 新サイドバー、Dashboard、横断検索、Discuss、Agent Center全画面、Arena比較画面の全面移植。
- Git Import、SSH資格情報保管、Session単位の削除、管理対象フォルダーの新しい作成・削除機能。
- Commands管理、CLI自動インストーラー、Desktop updaterの新規導入。
- 起動時の開発DBリセット、実データのfixture置換、ユーザー履歴・Wiki・worktreeの削除。
- Workflowの`selected_skills`廃止、Goal UIを旧permission選択へ戻す変更、session-specific MCP overrideの廃止。
- OpenWikiのfork／vendor／patch、旧`.llm-wiki`経路の復活、Wiki生成promptの品質再設計、大規模Wiki再生成・A/B品質比較。
- 新しい意味的依存エンジン、共有会話DB、独立scheduler、DAG、汎用artifact store、認証・秘密管理基盤。
- 無関係なrefactor、依存の一括更新、翻訳やCSSの全面刷新、remote backendの新機能。

上流の新UIが良く見えることや同じファイルに変更があることだけでは、採用理由にならない。条件付き項目からもこの除外境界を越えない。

## 7. LVKの非回帰契約

次は全段階に適用する。Wikiの説明だけで充足とせず、実コードと関連testsを確認する。

1. **Runtime**: Native Auditのlossless性・manifest/checksum、semanticなCanonical、Live表示の収束、最新値Snapshot、batch transactionとcursorの原子性、一時DB競合の再試行を維持する。
2. **停止**: active degraded Runの監査付きCancelを維持する。state不明・terminal・cancellingを適切に区別し、無監査killへ置換しない。
3. **Session／Goal**: fresh Session、follow-up、native resume、4実行モード、Goal lifecycle、継承reasoning、selected Skills、親子Agentの識別を維持する。子の完了を親の成功にしない。
4. **Workspace**: 用途、表示、物理所有権、実行owner、一時予約を分離する。内部実行を通常一覧へ戻さず、閲覧でSession／worktree／scriptを作らない。APIとdispatchのguardを残す。
5. **並列開発／Integration**: peerのReadOnly参照、採用source固定、業務予約、expected-B／exact-R、検証証拠、Git／Done／Wikiの別結果、復旧idempotencyを維持する。
6. **Repository Memory**: 通常codingのcanonical Wiki書込禁止、incremental Memory、Manifestのsource帰属、finalizer後のQueue起動、未統合event除外、単一writerを維持する。
7. **OpenWiki**: 同一worktree・fresh phase Sessions、ReviewerのReadOnlyとcontext遮断、全attemptのoperation sequence証明、索引・reportのidentity/digest、setup復元、最後のpublicationを維持する。
8. **既存利用形態**: 通常Workflow、LLM Condition、Arena、direct-folder、複数Repo、local Board、既存手動merge／PR、host-scoped transportを維持する。
9. **Viewer／chat**: Wiki本文のメイン表示、表示元identity、内部遷移、reload、非表示chatのdraftとtimeline保持を維持する。

上流のある機能が似た目的を持っていても、これらの代替であると仮定しない。特にDB変更通知やHost journalの追加だけで、既存の全projection障害が修復できたと主張しない。

## 8. Git・ブランチ・コミットの運用

### 8.1 許可する操作

実装依頼後は、次をエージェントの通常作業として許可する。

- 作業ブランチの作成・切替と、今回の変更だけを対象にしたローカルコミット。
- 段階ごとの検証済み成果を引き継ぐ、積み重ね型のブランチ運用。
- 衝突しない隔離テストbranch／repositoryの作成と、その中の試験source commit、正式Integration、Wiki publication。

第1弾は現在のブランチを使用できる。例として第2〜第5弾には`fix/upstream-settings-safety`、`fix/upstream-host-recovery`、`fix/upstream-compatibility`、`fix/upstream-editor-safety`等を使えるが、既存名と衝突したら新しい名前を選び、既存refを移動・削除しない。

前段の完了commitから次のブランチを切り、最後のブランチが全変更を含むようにする。全段階を元のmainから独立に分岐させてはいけない。単にbranchを作るだけでは未commit変更が保存されないことに注意する。

### 8.2 保護するもの

- main／origin/mainの更新、push、PR作成、upstream全体のmerge、取り込んでいない変更をmerge済みに見せる操作を行わない。
- 開始時の未commit変更を記録し、ユーザーの変更をcommit／stash／reset／上書きしない。本仕様書そのものは実装依頼の入力として第1弾へ同梱してよいが、他の既存変更を便乗させない。
- ステージングは対象path／hunkを明示し、コミット前にstaged diffをレビューする。秘密、実行ログ、試験DB、巨大build成果物、試験Wikiを実装commitに混ぜない。
- 一部がユーザー変更と分離不能なら、その境界だけを報告する。履歴操作で黙って解決しない。共有中の作業ツリーを別作業の都合で切り替えない。
- 段階ごとのcommitとbranchを保存し、後から過去のcommitを書き換えない。発見した回帰の修正は現在の先端へ追加する。

一つの論理修正と対応テストを追える粒度でcommitする。部分移植のcommit本文には参照上流SHAと適応内容を記載し、完全なcherry-pickだと誤記しない。既存著作権・ライセンス表示を維持する。

第1〜第5弾の順序はcheckpointであり、同じ関数を複数回直す必要がある場合の小さな前倒しは許容する。対応要求IDを記録し、未完了段階を完了扱いしない。

## 9. 型・DB・依存・配布

- API型はRustを正として生成する。generated TypeScript／schema／SQLx出力を手編集しない。
- migrationは既存ファイルを変更せず、新しい互換migrationを追加する。実DBへ上流migrationを試しに適用しない。代表的な既存データの合成fixtureへ全migrationを適用し、保存・帰属・foreign keyを確認する。
- BP08等に必要な最小fieldの追加は許可する。Task migrationを一部コピーして無関係なtable再構成を持ち込まない。
- 上流lockfileやmanifestを丸ごと置換しない。必要な依存だけを既存workspaceへ追加し、対応lockfileを正規コマンドで更新する。
- serverと独立`agent-process-host`の両方を開発用にコンパイルし、配布時に両binaryが必要な契約を保持する。旧binaryで新serverを試験して成功扱いしない。
- 公開forkのvalidation範囲を維持する。private billing依存取得、資格情報追加、source manifest改変でremote Cargo gatesを無理に通さない。
- 開発DBは永続データとして扱う。上流debug serverを既存asset dirへ向けて起動しない。migrationや障害注入は明示的な隔離DB・資産で行う。

## 10. 自動テストと性能確認

原則として修正前に失敗する回帰テストを用意し、修正後に成功させる。既存実装で充足する場合は確認テストにする。上流テストを読んだだけでは実行済みとしない。mockの文字列一致だけでなく、本番のreader／route／dispatcher／serializer／UI containerを通す境界テストを優先する。

| 対象          | 必須の検証                                                                                                                                                                             |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| BP01          | State読取とevent読取の間のcommit、空pageでの訂正、複数page、切断／送信timeout、通知欠落、Live deltaとcompletedの非重複、usage再接続                                                    |
| BP02          | 遅いsend成功中の追加入力、失敗、Session／Host切替、初回Session生成、Queue、Goal steer、承認返答、添付／Skill／設定変更、Scratch保存・削除の順序、localStorage                          |
| BP03–04       | DEFAULT明示／省略、非DEFAULTとprovider不一致、native adoption、未起動Session、latest attemptの順序と破損、兄弟Session隔離、全ExecutorConfig fieldsと継承値の保持                       |
| BP05–07       | 変更ゼロと未取得／errorの区別、cached degraded、secret非露出、保持／置換／削除、外部設定変更、未知値保持、誤Host fallback拒否、遅延応答隔離、optional discovery failure                |
| BP08          | 代表旧DBへのmigration、二つのeditorの競合、一方のdraft保持、transaction失敗時のSession非生成、system template拒否、run graph snapshot維持                                              |
| BP09–11       | page件数／bytes境界、巨大event、遅いsubscriber、reconnect、重複batch、Host生存／終了／不明、journal途中破損／末尾未完了、誤attempt、command再送、server再起動、旧protocol／registry    |
| Runtime非回帰 | 1,000 delta＋completedのsemantic compaction、全raw Auditのchecksum／replay、SQLite lock解除後の再試行、cursor atomicity、degraded時Cancel、terminal process確認、owner cleanup         |
| BP12–14       | explicit CLI path優先と失敗、ユーザー導入先、環境を壊さない探索、provider別本文／tool／retry／error、Codex分類維持、未知API／既知API／SPA／host-scoped経路                             |
| BP15          | entry依存の前後比較、採用時の分割とlazy load failure、deep link、reload、chat／Wiki切替、local／remote frontend型確認                                                                  |
| BP16–18       | 未保存離脱・保存失敗・競合、保存中の追加編集、Undo/Redoと保存境界、全graph fields保持、対象切替、system read-only、keyboardとfocus、状態表示とStop例外                                 |
| BP19          | 実validatorのstable／beta／easy受入、不正値拒否、既存入力互換。発行は実行しない                                                                                                        |
| 製品横断      | Goal親子分離、selected Skills、OpenWiki Reviewer権限、PASS／REFINE、Refine変更不要、全attempt完了証明、Memory finalizerとQueue、Integration予約／publication／Done、execution-only閲覧 |

第3弾では実モデルを使わない大量frame fixtureと遅いconsumerを使い、変更前後でcanonical event件数、transaction数、Host応答pageサイズ、メモリー増加、journal／Audit書込量・flush回数、追い付き時間を測る。必要な意味的eventを性能目標のために捨てない。新方式の耐久性とI/Oの代償を記録し、重大な退行を未修正で完了しない。環境に依存する絶対速度を一般的保証にしない。

各段階で関連検証を行い、最終コードで以下を実行する。

- `pnpm run format`
- 関連Rust unit/integration tests、およびroot workspaceの`cargo test --workspace`
- 関連Vitest／既存Playwright fixture tests。`test:wiki`だけで全frontendを検証したとしない
- Rust/API型変更時の`pnpm run generate-types`と`pnpm run generate-types:check`
- `pnpm run check`、`pnpm run lint`
- 必要な開発用コンパイル、`agent-process-host`とserverの整合性確認
- server配布境界へ影響した場合の`pnpm run server:check`

private remote backendのCargo checks/tests/lint/type generationは通常gatesから除外し、remote-web型確認とremote Rust formatは維持する。変更がprivate backend契約に影響する場合だけ、その未検証範囲を明示する。release build、Docker release build、GitHub Actions、publishは実行しない。

既存の無関係な失敗は変更前後の証拠で分離する。今回の回帰を既存失敗と呼ばず、testの削除・assertion弱体化・広いskipで合格にしない。

## 11. Chrome DevTools MCPによる最終受入

### 11.1 環境と資産の保護

開始時にChrome DevTools MCP、Chrome、開発用EVK、既存Codex認証、既存OpenWikiのhost-driven経路を確認する。過去の成功を現在の接続証拠にしない。Dockerの利用形態を確認し、動いている既存CLIを理由なくDocker-in-Dockerへ置換しない。

最終コードのbackend／frontendと対応するprocess-hostを、サンドボックス外から`localhost:4020`で確認できるようにする。実行revision・差分・binary build元を記録する。既存ユーザーrunを無断停止せず、必要なら終了を待つか安全な別環境で準備する。最終的な4020での確認を別portの成功へ読み替えない。

専用の小さなテストrepository、または衝突しない`test/upstream-backport-*` branchを使用する。Wiki publicationと正式Integrationのtargetも隔離対象にする。main／開発branchへ試験変更を反映しない。実ユーザーの設定・資格情報を書き換えず、設定の保存・機密値試験には隔離した設定fixtureと架空の秘密値を用いる。

実装依頼後は、この小規模受入に必要な既存認証済みCodex／OpenWikiの実モデル呼出しを許可する。別API-key経路、権限・認証の拡張、大規模なEVK Wiki再生成、品質比較は行わない。テスト資産と証拠は確認用に残す。

### 11.2 必須シナリオ

1. **通常Workspace**: MCPで通常UIからSessionを開始し、実CodexのLive出力、完成表示、follow-up、fresh Session、再読込、実行設定復元を確認する。別Sessionのログ・draftを混ぜない。
2. **入力保全**: 通常send、Queueまたは対応する待機操作で入力保持を確認する。遅延・競合を確実に作るケースは自動UI testsで補完する。API応答が正しいだけで入力UIを検証済みにしない。
3. **Goalと停止**: 小さな隔離タスクをGoalで実行し、継続・終端表示を確認する。別の試験runを正規Stopで停止し、実process・監査・UI状態の収束を確認する。試験を停止することと、既存ユーザーrunを停止することを混同しない。
4. **Workflow編集**: 試験用template／attemptを二つの画面で開き、保存競合、draft保持、未保存離脱、Undo/Redo、再読込を確認する。一般Workflowの実行が進むこと、system executionが編集可能になっていないことも確認する。
5. **設定と表示**: local設定画面の表示、対象identity、機密値の通常非表示、明示操作、失敗表示、差分error／zeroの区別、keyboard／focusを確認する。fake Host・遅延応答・secret書換えは隔離した本番部品のfixtureで検証し、実remote Host接続と称しない。新しいremote資格情報を必須にしない。
6. **OpenWiki**: 小さな隔離repoで通常UIからBootstrapを開始し、fresh Generate／Review、必要時Refine、completion proof、publicationまで確認する。ReviewerがPASSならRefineを強制しない。実機で通らなかった分岐は自動テストで確認したと区別する。
7. **Memoryと統合**: 上記repoの通常Workspaceで小さなsource変更を実Codexに依頼し、Manifest生成と、選択したWorkspaceの正式Integration、条件付きDone、後続OpenWiki Sync／publicationを通常UIから確認する。通常Workspaceがcanonical Wikiを直接更新しないことを確認する。既存の複数Workspace・競合・予約回帰は自動testsで維持し、大規模並列試験を繰り返す必要はない。
8. **閲覧**: Executions／Historyから上記内部実行を開き、owner・状態・ログ・成果物を確認する。自由chat／新規Sessionが解禁されていないことを確認し、Wiki Viewerの本文、代表的な内部リンク、reload、通常Workspaceへの復帰とdraft保持を確認する。

MCPで開始した実行の詳細確認にAudit、DBのread-only照会、server logを使ってよい。直接API／CLIからの開始だけでUI受入を代替しない。Host死亡・DB lock・破損等の故障注入は隔離自動テストで行い、実ユーザー環境を壊して再現しない。

### 11.3 証拠と再実行

- branch、EVK実行commitと差分、試験repo source OID、Workspace／Session／AgentRun／Workflow／IntegrationのID、時刻、Audit参照、publication commit、MCP操作記録または画面証拠を残す。秘密・全文会話を実装記録へ転載しない。
- Goal、publication、Doneはそれぞれ正本の状態と対応証拠で判定する。Agentの「完了」という発言だけで成功としない。
- 実行・入力伝達・永続化・完了証明・公開へ影響する修正後は、新しい試験runで影響するend-to-end経路を再確認する。古いserverや前段branchの成功を最終コードの成功として混ぜない。
- 表示だけの修正は、影響しない理由を記録した上で既存成果物を使い、MCPで元の操作を再確認できる。
- 無診断の再実行で偶然の成功を採用しない。失敗runを保全し、修正・環境回復等の再実行理由を残す。

## 12. 関連不具合の扱い

次は修正範囲に含める。

- 今回の移植が起こした回帰。原因箇所が別moduleでも対象。
- 採用要求の実行経路、既存契約の維持、必須受入を直接損なう、確認済みの既存不具合。

症状、再現／コード上の確定的根拠、要求IDとの因果関係、根本原因、修正、回帰test、MCP再確認を記録する。同じmodule内にあるだけの不具合、任意改善、未確定の疑いは記録に留める。無関係な領域へ探索を広げない。

関連する確認済み不具合を見つけたことは停止理由ではない。既存abstractionと安全性の範囲で修正を続ける。未修正の関連不具合を「制約」と記載するだけで達成扱いしない。一方、未発見の問題がゼロであることは要求しない。

## 13. 受入条件

| ID   | 達成条件                                                                                                                       |
| ---- | ------------------------------------------------------------------------------------------------------------------------------ |
| AC01 | 固定上流SHAと実装開始状態を記録し、BP01〜BP19すべての採否・変更・検証に対応がある                                              |
| AC02 | すべての必須要求を実装または既存実装の証拠で充足し、条件付き要求の採否を定義済み基準で説明できる                               |
| AC03 | 第1弾の状態・draft・Session／profile・差分の回帰testsが成功する                                                                |
| AC04 | 機密設定・Host isolation・Workflow保存競合とmigrationのtestsが成功する                                                         |
| AC05 | 有界配信・Host回復・protocol互換性・故障注入を検証し、Audit／cursor／Cancelを維持し、性能上の重大な退行がない                  |
| AC06 | CLI／provider／API／適用された遅延読込・版番号validation・Workflow編集操作のtestsが成功する                                    |
| AC07 | 第7節のLVK固有契約を対応testsで維持し、Task移行・DBリセット・除外機能を持ち込んでいない                                        |
| AC08 | 最終コードで必須quality gatesを実行し、今回の関連失敗・confirmed findingsを修正・再検証している                                |
| AC09 | Chrome MCPから最終EVKで通常Session、Goal、Stop、Workflow編集・実行、設定／表示の受入を確認している                             |
| AC10 | 小規模な実Codex／OpenWikiでBootstrapとManifest→正式Integration→Syncの公開経路が成功し、Viewerと実行専用閲覧をMCPで確認している |
| AC11 | 段階branch／commitと最終結果を追跡でき、main・ユーザー変更・実データ・global設定を保護している                                 |
| AC12 | 採否、適応、変更点、運用・互換性、試験結果、未検証範囲、残した試験資産を文書化している                                         |

条件付き非適用は実装成功ではなく、根拠付きの範囲判定として報告する。無関係な既存gate失敗が残る場合はbaselineと非影響を示し、当該gateを合格と表現しない。原因が不明、または今回の安全性に関わる失敗なら受入未完了とする。

## 14. 自律進行と停止条件

checkpointごとに、完了項目、branch／commit、検証、次の段階、残課題を短く報告する。通常の実装判断、既存機構に合わせた小さな適応、許可済みのbranch／local commitで確認待ちを挟まない。

修正可能な失敗では診断・修正・再検証を続ける。成功はAC01〜AC12を満たした場合だけとする。途中の三段階だけを完了としない。

安全な調査・代替手段を尽くしてもMCP接続、認証、利用枠、既存OpenWiki実行環境、追加権限などの外部要件で必須実機受入ができない場合は、可能な実装・ローカル検証を終え、「ブロック／受入未完了」と根拠と最小再開手順を報告する。手順書、mock、API-only試験をMCP成功へ読み替えない。

破壊的migration、権限拡張、製品モデル変更、安全性の撤廃が不可欠と判明した場合は、目的を維持する最小案と判断点を報告する。勝手に除外範囲を広げたり、未修正の実装不具合を環境制約と分類したりしない。

達成後は無関係な改善を続けない。mainへの取り込み、push、PR、releaseはユーザーの別操作として残す。

## 15. 最終報告

- 第1〜第5弾の構成、主な変更ファイル、BP01〜BP19の対応表。
- 採用元の固定SHA、LVK向けの適応、条件付き非適用と明示的除外。
- 段階branch／commitと、全変更を含む最終branch／commit、残る未commit変更。
- 最終コードで有効な自動テスト・quality gates・性能確認の結果。skipやbaseline失敗は区別する。
- Chrome MCP試験の対象・ID・実行revision・操作証拠、実モデルで通った経路とfixtureでのみ確認した経路。
- Bootstrap／Integration／Syncの結果、試験target、publication commit、Viewer確認。
- 発見した関連不具合の症状・原因・修正・再確認、無関係な未修正事項の理由。
- migration／旧protocol／設定／配布の互換性、残る制限、試験資産と安全な後片付け方法。
- mainとユーザーデータを変更していないこと。達成、または受入未完了の根拠と最小の再開手順。

OpenWiki本文は本仕様の移植作業中に手編集しない。実装・運用文書を更新し、canonical Wikiの更新は既存の正規経路で行う。試験WikiをLVK開発branchへコピーしない。
