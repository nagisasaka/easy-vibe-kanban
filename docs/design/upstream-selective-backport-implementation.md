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

第1弾は開始branchを使用する。後続branchは前段commitから作る。まだ実装commitはない。

## 最終受入の計画

最終コードの自動gatesと両binary開発用コンパイル後に、仕様第11節の8シナリオを実施する。隔離した小repo／test branchのみをGit反映先とする。実Codex/OpenWikiは小規模試験に限定する。

MCPのUI操作記録、run／session／workflow／integration ID、source OID、publication commit、監査参照を記録する。秘密や全文会話をこの文書へ転載しない。executionに影響する修正後は影響する実機runを新規実行する。
