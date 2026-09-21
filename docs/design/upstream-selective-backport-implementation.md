---
title: "上流改善の選別移植 — 実装・検証記録"
description: "固定した上流からの五段階移植の対応表、設計判断、段階コミット、自動検証とMCP受入の証拠。"
---

## 状況

実装中。AC01〜AC12は未達成。上流のコードを読んだこと、自動テストの成功、実機受入の成功を区別して記録する。

正規仕様は[選別移植仕様](upstream-selective-backport.md)。canonical Wikiは編集しない。

## 開始時点 — 2026-09-22

- branch: `fix/merge-upstream-test`
- HEAD、main、origin/main: `39bbba0f4238d08cdbc203defe20dae03aa90368`
- HEAD commit日時: `2026-09-21T19:14:51+09:00`
- 固定上流: `76a86da903a931e7cd290d1d4b26e9b4304794d9`
- mainとの差分なし。未commit変更は `docs/design/upstream-selective-backport.md` の追加のみ。同梱は依頼で許可済み。
- root、docs、local-webのAGENTSを全文確認。private remote backend Cargoは通常検証の対象外。
- Chrome DevTools MCPの`list_pages`成功。Chromeは9222、既存frontendは0.0.0.0:4020、backendは0.0.0.0:4021。
- 既存backendはこのcheckoutの`target/debug/server`。cargo-watchは動いていない。まだ再起動・ユーザーrun停止をしていない。
- Codex CLI 0.154.0、`codex login status`はChatGPT認証済み。資格情報本文は表示・変更しない。
- OpenWiki CLIは0.5.1のhelpを返す。ただし非TTYのhelp末尾にInk raw-mode errorが出る。host-driven MCPの実実行成功とは扱わず、EVK既存preflightと小規模UI受入で別途確認する。
- `openwiki` Skillを全文参照した。標準のqueue／Claims／finish契約を尊重し、今回のWiki試験はEVK UI起動に限定する。

## 対応表と実装順序

以下は開始時の調査対応表。段階ごとの実装・検証結果は後述する。各段階のコード・テスト・自己レビュー完了後にローカルcommitし、そのcommitから次段階branchを作る。mainを更新せず、最後のbranchに全変更を累積する。

| ID   | 接続点／参照上流                                                                                                         | 最小適応と検証予定                                                                        | 状態             |
| ---- | ------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- | ---------------- |
| BP01 | `routes/agent_runs.rs`、`AgentRuntimeReadService::history_page`、上流の空page State repair                               | LVKのLive先行購読とusage再送を残し、State変化検出・page追従・有限send・DB wake-upを追加   | 調査済み、実装前 |
| BP02 | `SessionChatBoxContainer`、`useSessionMessageEditor`、`useSessionSend`、Scratch API／localStorage、上流`sessionDraft.ts` | identityと送信snapshotでdraft消去を限定。サーバー側の無条件Scratch削除も確認              | 調査中           |
| BP03 | `profile.rs`、`provider_adapter.rs`、`sessions/agent_run.rs`、DB/runtime比較、上流`runtime_identity.rs`                  | exact DEFAULTだけを同一視する共通比較。既存保存値とnon-defaultを保持                      | 調査済み、実装前 |
| BP04 | `sessions` API／Sessionのconfig選択、上流`executor_config.rs`                                                            | latest immutable requestから全LVK fieldsを復元。破損時fail-closed、native adoption対応    | 調査済み、実装前 |
| BP05 | 差分hook・表示、上流`dfa`系のdiff error修正                                                                              | loading／empty／errorを分離し対象切替を検証                                               | 接続点精査待ち   |
| BP06 | `agent_settings.rs`、`agent_tools.rs`、既存settings consumer                                                             | safe DTO、明示的詳細取得、keep／replace／clearと外部編集保護。role MCP overrideを維持     | 接続点精査待ち   |
| BP07 | `SettingsHostContext`、settings queries／mutations、上流Agent Centerのidentity guard                                     | 明示Hostを維持、stale response隔離、cached readとwrite可否分離                            | 接続点精査待ち   |
| BP08 | `routes/workflows.rs`、Workflow DB model、上流revision/CAS transaction                                                   | 互換migration、期待revision、Session生成を保存と原子的に処理                              | 接続点精査待ち   |
| BP09 | `process_host.rs`、上流`process_host/journal.rs`                                                                         | bounded count/bytesと保存方式を実装前に確定。raw二重永続化・毎delta fsyncを機械移植しない | 接続点精査待ち   |
| BP10 | Host client／`agent_run_port.rs`                                                                                         | subscribe＋repair、cursor再送、bounded reconnect、command identity                        | 接続点精査待ち   |
| BP11 | Host registry／recoveryとNative Audit                                                                                    | 死亡確認と未反映回収、破損・不明のfail-closed、owner cleanup非回帰                        | 接続点精査待ち   |
| BP12 | `utils::shell`、executor `command`／availability                                                                         | discoveryと実起動を一致、explicit path優先。installerは除外                               | 接続点精査待ち   |
| BP13 | `provider_adapter.rs`、既存native fixtures                                                                               | non-Codexの本文／tool／retry解釈を分類境界へ適応しCodex四経路を維持                       | 接続点精査待ち   |
| BP14 | `routes/frontend.rs`とrouter                                                                                             | 未知APIを非成功にしSPA deep linkとhost transportを維持                                    | 接続点精査待ち   |
| BP15 | local/remote-web route・Vite entry                                                                                       | 依存解析で適用条件判定、該当時だけ既存routerでsplitし前後測定                             | 条件判定待ち     |
| BP16 | Workflow editorのload/save/navigation                                                                                    | draft revision・保存ACK・離脱保護。BP08 CASと接続                                         | 接続点精査待ち   |
| BP17 | Workflow editor、上流`workflowAuthoring.ts`                                                                              | bounded local Undo/Redo、LVK graph fieldsのround-trip                                     | 接続点精査待ち   |
| BP18 | 今回触れるchat/settings/workflow/diff UI                                                                                 | keyboard／focus、loading/error/empty/degraded、Stop例外                                   | 各段階で確認     |
| BP19 | `.github/workflows/publish-easy-npx.yml`                                                                                 | stable/betaの現在のvalidationを判定、該当時は実validatorをテスト。発行しない              | 条件判定待ち     |

## 再利用・非回帰の境界

- Runtime Wikiと`docs/future/agent-runtime/data-planes-and-recovery.md`を入口に、実コードの`history_page`とWebSocketを確認。Stateとeventsは別queryであり、現状のlive pollはeventがないとStateを送らない。上流修正の全置換はLVKのLive／Snapshotを消すため採用しない。
- Canonical batch、host cursor、Native Audit、projection failuresの既存永続化を再利用する。新たなevent store／schedulerは作らない。
- Session WikiのScratch説明は現行send/queueの無条件clearを調べる入口とする。入力保全は本文だけでなくselected Skills、attachments、review comments、executor configも対象。
- Workflow、Workspace usage、Memory、Integration、OpenWikiの既存契約を各変更前に実コードで再確認する。上流Task migration、DB reset、新shell、MCP共通化は採用しない。
- Session設定復元では実行時の未指定overrideとproviderで解決した実効値を区別する。既存Goal modes／max・ultra対応を狭めない。

## 設計判断・不具合・検証

### 第1弾 — 実装と初回検証

- BP01: DB commit後のbounded broadcastをwake-upのみに使用。250ms cursor repairは維持。State変化は空pageでも配信し、複数pageの末尾まで終端Stateを先行させない。sendは10秒でtimeout、Live channel終了でもdurable repairを継続する。LVKの先行Live購読、usage snapshot、semantic compactionは変更しない。
- BP02: UIのHost/Workspace/Session/revisionと本文・Skills・attachments・review・config snapshotを照合してACK後の消去を限定。新規Sessionの選択もACK対象が現在のcomposerである場合に限定。Scratchのsave/deleteをidentity別に直列化し、DB/localStorage両方でexpected payloadに一致する送信済みdraftだけを削除する。backend launch完了・queue消費による無条件削除を廃止。切替直前のdebounceは旧identityへflushし、遅延uploadや新Sessionの二段階APIも起動時Hostを保持する。
- BP03: 正確な`DEFAULT`省略だけを共通関数で正規化して比較。DBの原保存キー、native adoption fingerprintの元表記、scope/Skills/override制約は維持。修正前に`CODEX:DEFAULT`のfollow-up fixtureが失敗することを確認した。
- BP04: Session固有の最新immutable attempt requestを読むGETを追加。破損した最新値はエラー、native adoptionはbindingから復元。兄弟Sessionやscript processは参照しない。未指定reasoning/実行modeを後日変更されたpresetで上書きしないことも回帰テスト化。
- BP05: diffのloading/empty/error/cached degradedを区別。共通JSON-patch hookではendpoint切替時の旧snapshotを遮断し、再接続完了まで同じendpointのcacheだけを保持。Session/VSCode/Arenaのstatsとdiff本文にも状態を伝える。日本語の追加表示を用意。

初回検証（段階commit前。後続変更後の最終結果とは分ける）:

- `cargo test -p executors --lib`: 270 passed、6 ignored（既存の実OpenWikiインストール前提テスト）。四planeと1,000delta fixtureを含む。
- `cargo test -p db --lib`: 59 passed。batch/cursor原子性、SQLite lock、Workspace利用契約、Integration/Done等を含む。
- `cargo test -p server --lib routes::sessions`: 38 passed。設定復元、native scope、DEFAULT alias等。
- `cargo test -p server --lib routes::agent_runs`: 3 passed（State repairと型export）。
- `pnpm --filter @vibe/web-core exec vitest run --maxWorkers=4`: 70 files / 398 passed。
- production hooksを使うPlaywright `runtime-input.spec.ts`: 4 passed。遅い成功/失敗、追加入力、scope変更、Skill変更、新Session選択、永続draft再読込、stream error/cacheを確認。
- 既存fixture全体の初回は21 passed / 1 failed。`card-context.spec.ts`は旧Wikiが既定選択されるという廃止済み契約を期待していた。現行`defaultCardContext`と既存unit testに合わせ、既定はOFFと検査した後、明示選択して既存の独立toggle検証を続けるよう修正した。製品の旧Wiki経路は復活させない。
- frontend TypeScript、標準local-web/UI ESLintは成功。追加のweb-core直指定lintは既存のfeature間import規則違反と既存memo依存warningを検出。今回追加のmissing dependencyは修正した。標準gatesの成功とこの既存lint制約は区別する。
- 自己レビューで発見した新規のDB broadcast依存宣言漏れ、test import漏れ、空bodyの旧Scratch DELETE互換性、unmount後の遅延ACK選択、テストmoduleの配置を修正。
- 第1弾BP01〜BP05の実装・関連unit/fixture検証を完了。共通stream hookの全consumer（diff、Scratch、Workspace、execution process、approval、executor discovery）のサーバーがReadyを送ることも確認した。
- `cargo test -p local-deployment --lib container::tests`: 6 passed。QueueとMemory/OpenWiki/Integrationのfinalizer policyを確認。
- `cargo test -p server --lib routes::scratch::tests`: 1 passed。旧空body DELETEとconditional ACKを区別。
- 修正後のPlaywright全体: 22 passed。廃止済み既定Wiki選択への期待だけを修正し、その他の既存テストを維持。
- `pnpm run check`: frontend 4 packagesとroot Rust workspace成功。`pnpm run format`と追加fixtureのPrettierも成功。
- `pnpm run lint`: 標準frontend ESLint、root workspaceのclippy（all-targets / qa-mode）、unused-i18n-key確認が成功。

MCP実機受入はまだ未実施。接続確認を受入成功へ読み替えない。

## ブランチとコミット

- 第1弾: `fix/merge-upstream-test` / `d4e305c3`。BP01〜BP05、関連unit/fixtureとformat/check/lint成功。MCPは五段階の最終コードで実施予定。
- 第2弾: 第1弾commitから `fix/upstream-settings-safety` を作成。main/origin/mainは動かしていない。
- 第2弾完了commit: `fb509b17`（BP06〜BP08）。第3弾はこのcommitから `fix/upstream-host-recovery` を作成。

### 第2弾 — 設定・編集の安全性

- BP08: 現行updateはgraph writeより前にSessionをINSERTする。production関数を通すSQLite trigger故障注入で、保存失敗後にSessionが0→1件となることを修正前に再現した。上流のrevision CASを移植し、LVKのSession working directory解決と利用契約guardを再利用して同一transactionへ収める。Task model変更は採用しない。
- BP06: 現行settings discovery/diff、tool inventory、profile list/copy-previewがraw値を返す。safeな公開表示と明示的な高度編集を分離する必要がある。上流の高度編集廃止はそのまま採用しない。session-specific MCP overrideの適用経路は設定管理と別に維持する。
- BP07: 現行SettingsHostContextは未知の明示Hostをlocalへfallbackし得る。選択identityを保持し、Host/provider/projectをまたぐ遅延応答を隔離する。任意remote discovery失敗でlocalを停止させない。

実装:

- Settings／Tools／profileの通常公開DTOを安全なsummaryへ分離。native全文・MCP定義・Skill本文は同意付きの明示取得とrevision照合を経由する。解析エラーに原文を含めない。高度編集は残し、日本語でも機密値取得の意味を説明する。
- 非表示値をplaceholderとして保存しない。MCPは`preserve`／`replace`／`clear`、Skill契約編集は未読assetを保持する。設定profileはserver側capture・reference付きcopy/apply/rename/deleteへ変更し、秘密のenvや未知設定をブラウザ経由で再構成しない。
- 既存`/api/info`のlaunch profile、`/profiles`、高度MCP編集も調査して保護。最近利用したmodelの更新は専用の狭いpatchとし、sanitized profile全体を保存してnative設定を消さない。
- 同一service内のnative書込は共通lock、参照hashと書込直前の比較、atomic file replacementを使用。外部編集を観測した場合は拒否し、rollbackも外部変更を上書きしない。任意の外部editorに対するOSレベルの完全CAS／秘密管理サービスではない。
- Host identityは未知でも維持。offline／discovery失敗は書込不可、localは任意remote discoveryの失敗と分離。scope変更で古いload結果を捨て、machine clientもdispatch直前に書込可否を照合する。Cancel policyは変更しない。
- Workflowに非破壊の`revision` migration。graph更新CAS、必要Session INSERT、draft→readyを一つのtransactionで処理。失敗／409時は孤立Sessionを残さない。system owner専用Sessionの生成経路は変更しない。
- 関連する既存不具合として、Codex設定descriptorのmax／ultra欠落、TOMLの通常table形式のenv mapを適用できない問題を修正。copy時はtarget providerで非対応の値を移植しない。

検証:

- `cargo test -p server --test workflow_routes`: 40 passed（CAS、保存失敗／Session INSERT失敗の原子性、system templateを含む）。migrationの旧schema fixture: 1 passed。
- `cargo test -p executors --lib`: 280 passed、6既存ignored。safe serialization、明示読取、preserve/clear、revision競合、profile capture/copy/apply、未知値保持、rollback外部変更保護、MCP atomic writeを含む。
- `cargo test -p server --lib settings_safety_tests`: 2 passed。launch profileのsafe summaryと既存Goal/ultra/parallel設定保持、recent-model patchを確認。
- Vitest: 72 files / 402 passed。production hooks／panelsを使うPlaywright全体: 25 passed。追加3件はunknown Host非fallback、optional discovery failure下のlocal利用、遅延provider応答隔離、明示全文取得とrevision付き保存の競合。
- 初回fixtureのprovider指定prop誤りと、変更前API名を呼ぶ追加unit testを修正して再実行した。fixture成功はMCP実機受入とは区別する。
- 型はRustから生成。ts-rsのflatten出力で生じる行末空白はgeneratorで正規化し、生成物を手編集しない。
- `pnpm run format`、`pnpm run check`、`pnpm run lint`、`pnpm run generate-types:check`成功。追加fixtureはweb-coreのPrettier設定でも整形。最終差分レビューでMCPのdiscovery値とhashの別読取を同一snapshotへまとめ、writerにもexpected revisionとcreate collision拒否を追加して回帰テスト済み。

互換性: 設定公開APIはsafe DTOと明示write intentへ更新。旧clientの全文再送は黙って受理せず、更新版UIを使用する。DB内容と実際のユーザー設定はmigrationで書き換えない。Workflow editorの保存ACK／draft／Undo改善は第5弾でこのCASへ接続する。

## 最終受入の計画

### 第3弾 — 実装前の配信・回復契約

現行HostはrawをNative Auditへsync後に分類し、Projected（durable/live/native ref）を無制限Vecへ保持。Attachはcursor以降全件をcloneし、observerは1秒poll。DBは意味的event／usage snapshotとHost cursorをbatch transactionで保存する。commandは既存durable identityを持ち、Host制御を再送してよい根拠とはしない。

- 上流固定SHAのjournal／Subscribe／dead-host recoveryを参照し、LVKのProjectedと四planeへ適応。raw全文は従来Native Auditのみ。Host journalは意味的event・usage snapshot・制御開始/終端・監査参照を保持し、Message/Thinking/Tool delta本文は保存しない。
- journalはattempt／Host instanceを明記したchecksum付きbatch。最大128件または256KiBを目安にまとめ、50msの有限flushと開始/終端flush。sync成功前のsequenceを配信せず、DB cursorも進めない。Native Audit自身の既存sync契約は弱めない。未flush窓の途中出力はAuditに残るが、journal未完了から成功を復元しない。
- Live deltaは件数とbytesを制限したringに限定。遅いreaderが失ったdeltaは再表示せず、durable completedで収束。usage snapshotはdeltaと異なりjournalに残す。
- Attach／Subscribeは件数・bytesでpage制限。単一巨大eventは明記したhard limit、上限超過は明確なエラーとし黙って切らない。subscriberは独立connectionで、有限send/read timeout・heartbeat・定期repair、失敗時はDB cursorから再接続。
- seek indexはbatch数に比例し、定数メモリとは呼ばない。readerは一つのbounded batch/pageだけを保持する。raw二重耐久化やper-delta追加fsyncを避け、実fixtureで書込量・flush回数・サイズ・追従を計測する。
- 新Host protocolは明示versionを持つ。旧HostはAttach互換で再接続し、新journalと推測しない。Host identity mismatch／生存不明は終了扱いしない。確認されたHost終了後だけjournalを検査・回収し、途中破損・sequence gap・別attempt・不完全末尾を区別する。
- Host死亡、provider死亡、canonical終端、Audit終端、owner cleanup／OpenWiki proofを別に扱う。回収したtextだけで成功にせず、未完了はcrash/audit failureとして診断。生きたproviderを勝手にkillせず、既存の監査付きCancel経路を維持する。

### 第3弾 — 実装と検証

BP09〜BP11を固定上流の`HostJournal`、`Subscribe`、recoveryから適応した。上流のevent単位syncをそのまま移植せず、上記のgroup journalとLVKのsemantic/live/usage分類を使用した。protocol 2とLinux boot/start identityをnullable migrationで記録し、既存行のPID・cursor・stateを維持する。旧HostはAttachを使用し、未知versionを拒否する。

- 新規`process_host/journal.rs`は128件/4MiB page、8MiB単一semantic event、256件/4MiB Live ring。producer queueも64件/8MiBでbackpressure。indexはbatch数に比例する。50ms/group sync以前のcursorを返さない。回復時は完全recordをsyncしてからDBへ反映する。
- Subscribeの10秒heartbeat/送信deadline、30秒読取deadline、切断後のDB cursor replay。slow subscriberの送信中はjournal lockを保持せず、別のControl接続を使用できる。
- Native Auditのidentity/reference checksumをbounded readerで検証。途中破損・gap・foreign attempt・未知protocolはcursorを進めずfail-closed。最後の未完了appendは未commitとして区別し、成功根拠にしない。
- Host生存/不明/元PID再利用、providerおよびprocess groupの生存を区別する。子が生きている場合はStartedだけを回収して正規Cancel可能にし、terminalを適用しない。無断kill・replacement launch・非冪等commandの再送は追加しない。
- 関連する既存不具合を修正: providerのterminal通知がHostのAudit閉鎖より先にcanonical成功を確定し得た。terminal化はHost Terminalに限定。さらにcanonical commit後のregistry更新失敗でも、同一page再送でexit後処理を完了できるようにした。重複Startedで終了済みprocessを再登録しない。
- Journal破損の診断は既存`ProjectionDegraded` eventへ保存し、Host cursorを動かさずactive runの監査付きCancelを残す。OpenWikiの全attempt proofやowner lifecycleの判定自体は変更しない。

検証（段階コード）:

- `cargo test -p local-deployment --lib`: 71 passed。slow consumer/再接続/finite control、Journal flush失敗、Host生存中の拒否、子生存中の非terminal化、全Audit照合、restart重複、未完了末尾、破損、gap/別attempt、DB保存/commit後後処理の故障注入を含む。
- `cargo test -p executors runtime::native_audit --lib`: 5 passed。prefixを完了と扱わず、closed manifestと末尾までchecksum照合。
- `cargo test -p db agent_runtime --lib`: 既存21 passed（1,000 delta compaction、cursor atomicity、実SQLite lock解除後retry、usage upsertを含む）。追加の`host_replay_migration`も1 passedで、旧PID/cursorを保持し、新fieldがNULLから明示登録されることを確認。
- `cargo clippy -p local-deployment -p executors -p db --all-targets -- -D warnings`: 最終追加テスト後も成功。`pnpm run format`、`pnpm run backend:check`も成功。
- 性能fixture: 1,000個の5,000字deltaについて、旧Vec/全Attachに相当するserialized payloadは5,764,676 bytes。新ringは約1,470,864 bytes、Journalは約300,154 bytes、group fsync 8回、offset 256 bytes、fixture全体約427ms。これはpayload量の比較でありRSSや任意環境の速度保証ではない。旧Journal書込は0、新Journalのdisk増分は約300KiB。Native Auditの既存raw/frame毎syncは維持する。
- production adapter→journal→Audit検証→port→DBの別fixtureは1,000delta+completed+input（raw 1,002）からcanonical 4件（user/running/completed/terminal）、8 page transaction。semantic compactionは移植前から4件であり、新たな削減と誤記しない。server再生成と同一page再適用後も4件、回収2周約280ms。SQLite commit数はpage境界で有界化され、control/registry/Audit metadataの別書込はこれと区別する。

未実施: 実Codex/Goal/OpenWikiを通す最終MCP受入、最終全workspace gates。現時点の旧serverを本変更の成功証拠にはしていない。

最終コードの自動gatesと両binary開発用コンパイル後に、仕様第11節の8シナリオを実施する。隔離した小repo／test branchのみをGit反映先とする。実Codex/OpenWikiは小規模試験に限定する。

MCPのUI操作記録、run／session／workflow／integration ID、source OID、publication commit、監査参照を記録する。秘密や全文会話をこの文書へ転載しない。executionに影響する修正後は影響する実機runを新規実行する。
