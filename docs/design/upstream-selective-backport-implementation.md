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
- 第3弾完了commit: `56af98b6`（BP09〜BP11）。第4弾はこのcommitから `fix/upstream-compatibility` を作成。

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

### 第4弾 — 適応方針と段階検証

- BP12: 固定上流のuser executable探索とavailabilityを既存resolverへ適応。明示pathは実行可能性まで確認し、無効な指定から別CLIへfallbackしない。native設定／認証ファイルだけではinstalledにせず、各providerのcommand overrideを起動と同じparser／resolverで検証する。npm／bun prefixとper-user配置のfallbackはglobal PATHへ追加しない。既存login-shell PATH refreshは維持する。インストールも資格情報変更も行わない。
- BP13: 固定上流のnested text、Claude API error／retry fixturesをLVK adapterへ適応。混在するtext／tool use／tool resultは同一Native Audit参照から意味的eventへ展開する。新規non-Codex mapperをv3とし、v1／v2の過去の分類・sequenceは変更しない。replay decoderへ最新分類を混ぜず、旧frameが新validationに巻き込まれないようにした。CodexのLive／Canonical／Goal／親子分類を変更しない。retryは非terminal状態表示、errorは構造化された理由を保持する。
- BP14: production frontend routerのAPI namespace guardを移植。未知APIは404、非GETも非成功、SPA deep linkとstatic assetは従来どおり。API middleware／signed／Host relay経路は変えず、上流の開発redirectは取り込まない。
- BP15: 適用。両Vite設定で既存TanStackのautoCodeSplittingが無効で、初期entryに全routeが含まれていた。既存pluginの分割を有効化し、共有のloading／error表示は既存CrashScreenとi18nを利用。chat／Wiki表示のmount契約は変更しない。

ビルド比較（同じdevelopment mode、manifestのentryから静的importsを再帰集計、source mapを除外）:

| app    | 変更前 JS / gzip            | route分割後 JS / gzip     |
| ------ | --------------------------- | ------------------------- |
| local  | 6,488,384 / 1,954,826 bytes | 1,517,832 / 473,957 bytes |
| remote | 5,983,797 / 1,806,869 bytes | 3,133,957 / 967,670 bytes |

これはroute分割の中間測定（error表示追加前）で、全route閲覧時の総転送量や実機速度の保証ではない。成果物は `/tmp/lvk-backport-bundles.i2KwAc/` のbefore／split各directory。既存のchunk size／Browserslist／Tailwind警告は残る。release packageは作らない。最終frontend buildとMCP deep link／Viewer操作を後続で確認する。

実行済み: provider adapter 33 tests、CLI resolver 5 tests、production frontend HTTP fallback 1 test、Vitest 72 files / 402 tests、Playwright 27 tests（lazy deep link／reload／failure／keyboardを2件追加）。全executorテストは287 passed / 6既存ignored。追加のavailability経路testを含めて最終再実行する。初回checkでremoteの共有module alias誤りを検出し、既存の`@/`へ修正。修正前のcheck失敗を合格として数えない。

第4弾最終再実行: executor 288 passed / 6既存ignored、resolver 5 passed、frontend HTTP 1 passed。`pnpm run check`と`pnpm run lint`成功。local／remote development buildも成功し、error表示を含むentryはそれぞれ1,518,982 / 474,355 bytes、3,136,498 / 968,469 bytes（JS / gzip）。fixtureはprovider資格情報・有料呼出しを使わない。最終MCP受入はまだ未実施。

`pnpm run generate-types:check`も成功し、生成型／schemaに追加差分なし。stage差分レビューで旧mapperのdecoder互換性とtool展開後のreplay cursorを確認・修正し、再実行済み。

第4弾commit: `2ce8cf132f9886fb29e2d84e466e255dd4123992`、branch `fix/upstream-compatibility`。

### 第5弾 — 実装方針（進行中）

作業branch `fix/upstream-workflow-editing` は第4弾commitから作成。BP16〜18は上流のauthoring framework／Canvasを移植せず、既存Workflow editorへimmutable save snapshot、元revisionを保持するtab-local draft、離脱確認、bounded Undo/Redoを追加する。workflow APIは既存のlocal hostScope:noneであり、relay HostのAPIを仮定しない。履歴はgraph全fieldを保持し、保存時にserverが割り当てたSession IDだけを新しい編集と履歴へ反映する。右panelの入力もdraftに含め、system template／取得エラー時cached表示を編集可能にしない。

BP19は既存publish workflowにeasy限定validatorがあるため適用。stable／beta／easyの入力validationと説明のみ変更し、実validatorのローカルテストを追加する。Actions／発行／package version変更は行わない。

関連する既存不具合: 新規Attemptの説明入力がcreate requestに存在せず固定説明へ置換された。optional descriptionを既存requestへ追加し、未指定の互換動作を維持する。型はRustから生成する。まだ本段階の検証完了とは扱わない。

第5弾の段階実装・検証:

- BP16: 元revisionと編集version付きimmutable snapshotを保存し、遅いACKは送信後の編集を消さない。dirty時のrefetchは元revisionを保持し、409は自動上書きせず最新baselineを再取得する。Sessionのserver割当IDのみ新しい編集と履歴へ合成する。tab-local sessionStorageへのACKを同期記録し、保存直後の離脱でも古いdraftを復活させない。storage失敗を表示し、保存／破棄／継続を選べる。target変更時はeditorをremountして遅い応答を隔離する。
- BP17: graph全fieldのsnapshotによるUndo/Redoを既存Canvasへ接続。最大64状態、各stackはUTF-16 2Mi code units以内で古い履歴だけを落とし、編集中graphにはこの上限を課さない。Agent／Router panelの入力もgraph draftへ移し、設定選択は親draftを正とする。input／textarea／contenteditableのnative Undoを奪わない。実行やGitを巻き戻さない。
- BP18: dirty／conflict／cached read-only／保存失敗をEN/JAで表示。離脱dialog内でも保存失敗を読めるようにし、編集継続で元focusへ戻す。system templateと取得失敗時のcached graphは変更・実行できない。旧来のAgent panelの「取消」は入力を捨てない「閉じる」と区別した。
- BP19: `scripts/validate-npm-version.cjs`を実発行workflowの入力validatorにし、同じcodeをunit／CLI testsで検証（3 passed）。stable／beta／easyを受け付け、余白／leading zero／任意suffix／shell文字列を拒否。発行は実行していない。
- 新規純粋reducer tests 5 passed。最終Vitest 73 files / 407 passed。Playwright全34 passed（production draft hook＋実workflow HTTP client／query更新＋Canvas＋router blockerを使う7件を追加）。遅延ACK、409、失敗後離脱、reload、Session割当と全field、executor Undo、native文字Undo、Canvas移動、scope切替、system／cached read-onlyを含む。実backendのCASとSession原子性はRust tests、全editor／実Agentは後続MCPで確認する。
- `pnpm run check`、`pnpm run lint`、`pnpm run generate-types:check`成功。`pnpm run format`実施。新規production helper／共有hookの追加ESLintも成功（testは既存local-web tsconfigに含まれず、この追加lintの対象外。Vitestで実行）。
- Workflow route 40 passed。root全Rust gate初回で予定実行fixtureの4件が`workflows.revision`欠落により失敗し、実migrationをfixtureへ適用して修正。修正後のserver lib全152 passed。
- 同じ初回gateでOpenWiki全attempt proofの1件も失敗したが、当初assertionが具体的errorを表示していなかった。assertionを弱めずdiagnosticを追加し、単独1件／server全152件で成功を確認。原因は未確定であり「修正済み」または環境原因とは断定しない。最終root gateで再確認する。
- 新規browserテスト作成時のnative Undo grouping／node toolbar上のdrag座標の誤った前提を修正。1回はformatとVite HMRが重なったため、その実行を採用せず変更を止めて全34件を再実行した。製品テストの省略・assertion弱体化はしていない。
- `pnpm run server:check`: 12 passed / 1既存skip（nginx未導入）。静的設定・実entrypoint故障fixtureは実行したが、HTTPS ingress実機は未検証。server/hostの配布構造は変更していない。

小規模実機資産: `/workspace/lvk-backport-acceptance.tGwIkS`、target `test/upstream-backport-acceptance`、開始source `147ceb2d45e2f61d9dbdebd2c6a69688d7282aa3`。Node built-insのみのLabel Kit、初期4 tests成功、既存Wiki／ignoreなし。test repo内だけfixture Git identityを設定し、global identity／資格情報は変更していない。UIからの開始・publicationはまだ未実施。

第5弾commit: `34987ca0fd7a2fd17b6bf907c0d1fde44bc7d85d`、branch `fix/upstream-workflow-editing`。main／origin/mainは開始時の`39bbba0f`のまま。

### 最終コードのquality gatesと実機準備

`34987ca0`のcodeに対し、format、check、lint、Vitest 407、Playwright全34、版番号3 testsが成功。`RUST_TEST_THREADS=8 cargo test --workspace`もexit 0。多数のDB／fsync fixturesと実機準備を並行するためtest並列数を8に限定した（testの省略はしていない）。既存ignoredは8件。先のOpenWiki testの原因は引き続き未確定であり、diagnosticを残した。private remote backendのCargo検証は規定どおり実行していない。

server／agent-process-hostを同一codeからdevelopment buildし、2026-09-21T23:30:21Zに旧serverをSIGTERMして再起動。直前のactive AgentRun、active Workflow、enabled scheduleはいずれも0。frontendは0.0.0.0:4020、APIは4021。migration両件は既存DBでsuccess=1。起動時の履歴workspace再分類は0件。開始時から存在するterminal runのpending Cancelに対するrecovery警告が1件あり、今回の試験runとは別で、手動で履歴を修正していない。

- server SHA256: `8b758be004a1bc9746908b93cb926af232b3ec364f8db459714066b0dc9bc868`
- process-host SHA256: `3284a810c7c298fae259071faebfa16b73d6380195942954eab200faa980e1ed`
- 新server PID: `3783455`。試験時の追加差分は本記録のみ。
- MCP専用browser context `lvk-upstream-backport-acceptance`、page 4。既存userのpages 2／3は操作しない。
- MCPで作成したproject `94f8ef0e-6453-40ae-ad4d-8bc6e601dacb`、名称 `Upstream Backport Acceptance 20260922`。まだ各実行の受入完了ではない。

最終frontend development buildも両app成功（`pnpm exec vite build --mode development --manifest --outDir ...`）。出力は`/tmp/lvk-backport-final-web.4D6pOu/{local,remote}`。generate-types checkをcommit後にも再実行し成功。既存Browserslist／chunk-size等の警告は残る。

実機受入開始（最終code `34987ca0`、差分は本記録のみ）:

- 2026-09-21T23:36:25Z、MCP page 4でproject設定のInitialize Wikiをクリック。repo `8dc5c831-f4ad-49cd-9fa3-be445e2b763e`、targetは明示保存した`test/upstream-backport-acceptance`、language en。
- Bootstrap Workflow `2881ebef-21c6-4b61-8f2f-ad5efddf3f96`、execution-only workspace `be2f5025-07c9-459b-a152-1dedb540e668`、worktree `/var/tmp/vibe-kanban-dev/worktrees/be2f-openwiki-lvk-bac/lvk-backport-acceptance.tGwIkS`。
- Generate Session `c8fceee9-5c44-46b2-befb-a6757b88a92d`、AgentRun `3f891684-c06b-574f-48db-125b546cc16e`。UIのView Workflowから進捗・実MCP beginを確認。system graph編集／自由chatはdisabled。
- 原資料索引は候補3、Markdown解析2、instruction-only 1、問題0、chunk 1、digest `sha256:c91c9e99e860b0cf926b7df859dc68af43e86d73ac43420c25e0279a41fa308c`。生成結果ではなく開始時source `147ceb2d45e2f61d9dbdebd2c6a69688d7282aa3`に固定。
- MCPのfilePath指定保存はMCP側workspace roots制限で拒否されたため、UI操作／snapshotのtool記録を証拠とする。これをブラウザ操作不能とは扱わず、権限設定も変更していない。

### MCP受入で確認した関連不具合と再検証

- 既存不具合（BP18）: Stage枠を選択すると本番CSSのselected z-indexがAgentより前面になり、内側のnodeや編集ボタンのpointer eventsを奪った。本番CSSとcontrolled selectionをfixtureにも読み込み、修正前はclick timeout／Stage interceptionで失敗、selected時も背面を維持すると成功。全Playwright 35件成功。MCPでもStage選択後にAgent編集を開き保存できた。CSSのみの変更でruntime／publication証拠は無効にしない。
- 既存不具合（通常Workflow受入）: MCPで作成・保存したAttemptの最初の実行 `c48f6b37-b23a-447c-b1df-9c77d829d7d5` が `Workflow workspace has no local path` で失敗。Workspace `ca60b7f3-5aee-4688-aa4a-01526f2d98ae` はDB上だけ作成済みで、AgentRunへ移行したdispatcherが旧container経路の遅延worktree準備を呼ばなかった。閲覧により偶然worktreeが作られることに依存せず、Agent dispatchで既存ContainerServiceを呼ぶ。Sessionの所属検証後・新規Session作成前とし、Integration予約は準備前に拒否。execution-onlyはownerが用意した既存環境のみ読取り、setup／復元を再実行しない。通常／内部共通のowner・launch validationは維持。実際のDB migrationを用いた新規2 testsは旧動作で失敗、修正後成功。実runへの再検証は後続。
- 説明欄の入力が保持されない疑いは、MCPのfill操作とmodal focusの誤りを切り分けた。実input focusを確認してkeyboard入力・保存した結果、revision 3のDB descriptionに正確に保存された。未確認の製品不具合としてコードを追加変更しない。
- 二つの実backend editorでrevision 1→2の保存競合を再現。409後も別タブのdraftを保持し、reload後も元revisionのまま保存を拒否。離脱時の編集継続とfocus、明示確認後の破棄を確認。Note追加→Undo（0件）→Redo（1件）→Ctrl+Z（0件）をMCPで確認。API-onlyではなく本番UIで実施。

初回Bootstrapは2026-09-21T23:36:29.969Z〜23:43:42.695Z（約7分13秒）で成功。Generate／fresh Review(pass)／Refine skipped／Publishの経路。Review Session `019eecce-c898-4e65-9bef-4820962f2e49`、AgentRun `7335a53b-a13b-8b2d-ca5f-9ac919b7ebb7`。publication `48ffe453311374d6d79895a0029b7fb445d39289` は隔離targetのみ。2本文ページと索引があり、MCP ViewerでCurrent workspace／branch表示、index→quickstartを確認。今回のdispatcher修正は内部phaseにも通るため、この成功を最終runtimeの合格へ流用せず、`test/upstream-backport-final`（同じ初期source `147ceb2d`、既存Wikiを削除せず新規ref）で改めてUIから開始する。

上記修正後はserver lib 154／Workflow routes 40／Playwright 35 tests成功、format・check・lintも成功。serverとprocess-hostを同じcodeからdevelopment build（hostのsource変更はなくbinary hashも同じ）。AgentRun／Workflow／Integration active=0を確認して2026-09-21T23:55:48Zに再起動、server PID `3938671`、SHA256 `f4ca60fa8f9cd59f4addbffb95e0b84d94fc1ebfe25683590d8de71c8a825a2d`。ユーザー実行は停止していない。

通常WorkflowはMCPから新規run `25b09c1a-cf26-4b40-98ea-9b9da2b53617` を開始し、23:56:22〜23:57:53Zで成功。AgentRun `6386e9d1-10e8-e758-7c59-75353e27b9a9`。修正前に失敗した同じlazy workspaceでCodexのLive出力と完了を確認。修正版BootstrapはMCPから `a7b5311f-1faa-4156-bca0-3f340ad2a6d5`、Workspace `1a7220eb-4ce4-49c2-ac88-1ee02a08476a` として23:56:44Z開始。Generate Session `923467c2-019b-414d-9256-ded10c72453b`、AgentRun `118f4868-3d55-8c0e-4676-4475fc7bfdd5`。完了は別途確認する。
