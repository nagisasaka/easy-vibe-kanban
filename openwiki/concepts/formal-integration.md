---
type: concept
title: 正式 Integration の選択・検証・公開
description: local Board で採用した Card と Workspace の変更を固定し、host 検証済み commit だけを公開する契約。予約、取消、条件付き Done、Wiki と回復の境界。
tags: [integration, git, validation, lifecycle, recovery]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-21T08:16:36.701Z
sources:
  - id: openwiki-source-cc96689b2a7b877e58513f38
    resource: repo://crates/db/migrations/20260918000000_formal_integration.sql
  - id: openwiki-source-c01fa644be961cb7ee12d2b5
    resource: repo://crates/db/src/models/integration.rs
  - id: openwiki-source-d40a52c39b8604505b97c3e9
    resource: repo://crates/git/src/publication.rs
  - id: openwiki-source-595d5d0308d670e38ef27fbf
    resource: repo://crates/server/src/routes/integrations/admission.rs
  - id: openwiki-source-ecfa47de2e3842c785820c9b
    resource: repo://crates/server/src/routes/integrations/mod.rs
  - id: openwiki-source-ec7bbc48470f4ddafbe92723
    resource: repo://crates/server/src/routes/integrations/runtime_tests.rs
  - id: openwiki-source-b393e75029374eaf9cba5f7d
    resource: repo://crates/server/src/routes/integrations/runtime.rs
  - id: openwiki-source-e0c2de9e01617a8949b78360
    resource: repo://packages/web-core/src/features/kanban/ui/KanbanContainer.tsx
generated: { by: "codex", at: "2026-09-21T08:16:36.701Z" }
---

# 正式 Integration の選択・検証・公開

正式 Integration は、local Board で明示的に選んだ Card 群を一つの Repo・local target branch へ統合する製品実行である。各 Card に一つの採用 Workspace と観測済み commit を指定し、専用の [execution-only Workspace](workspace.md#操作用途と実行所有者) と一つの Session で整合させる。通常 Workflow、手動 squash merge、[detached preview](../workflows/task-to-integration.md#並列変更を試してから正式統合する)とは lifecycle が異なる。[選択](../../crates/server/src/routes/integrations/admission.rs#L191-L224)、[環境と実行](../../crates/server/src/routes/integrations/runtime.rs#L385-L508)。Board の入口は local API が有効で remote host を選択していない場合に表示される。[UI の境界](../../packages/web-core/src/features/kanban/ui/KanbanContainer.tsx#L1010-L1024)

## 選択を凍結し、他の writer を止める

採用元は現在その local Card / Project に関連している必要がある。Card に関連する全 Workspace を調べ、AgentRun・未確認 process・script finalization・送信待ち queue・active Goal がなく、tracked / untracked 変更や未完了 Git 操作もないことを要求する。他 Repo の未統合結果がある Card は拒否する。source の意味記録がある場合は成功 coding run の Manifest 完了も確認する。[idle 判定](../../crates/server/src/routes/integrations/admission.rs#L26-L54)、[全関連環境の検査](../../crates/server/src/routes/integrations/admission.rs#L79-L155)、[clean の意味](../../crates/git/src/publication.rs#L175-L201)

source OID、branch、Card の title / description / status / requirements revision、関連 Workspace 群、帰属を証明できる event ID を保存する。HEAD が利用者の `expected_commit` と異なれば `SOURCE_CHANGED` で明示的な再選択を要求する。同じ request ID は同じ選択・設定に対して既存 run を返し、違う入力で再利用できない。[凍結](../../crates/server/src/routes/integrations/admission.rs#L155-L188)、[冪等性](../../crates/server/src/routes/integrations/admission.rs#L225-L254)

予約は Card と全関連 Workspace に対する永続 DB 記録で、dispatcher lease の期限では解放しない。受付の writer transaction と受付後の再照合で競合を検査する。target の順番は **canonical Git common directory** をキーに repository 全体で直列化し、別 Repo ID の登録や別 target branch でも同じ storage の run が追い越さない。[予約と再検証](../../crates/server/src/routes/integrations/admission.rs#L285-L355)、[target queue](../../crates/server/src/routes/integrations/admission.rs#L367-L378)。DB trigger も Agent 起動・Card/link・Workspace mutation を保護する。[永続 guard](../../crates/db/migrations/20260918000000_formal_integration.sql#L55-L129)

予約された通常 Workspace は用途まで execution-only に変わるわけではない。専用実行環境の恒久的な操作制限と、採用元の一時予約は [Workspace の契約](workspace.md#操作用途と実行所有者)で区別する。worktree の分離は協調的な規則であり、外部 Git 操作や agent の OS 権限を完全に隔離するものではない。[実行指示の境界](../../crates/server/src/routes/integrations/runtime.rs#L173-L186)

## 固定した B から検証対象 R を作る

```mermaid
flowchart LR
  Q[選択と予約] --> B[順番が来た target B]
  B --> A[採用 source OID を統合]
  A --> R[clean な候補 R を固定]
  R --> V[host が検証 plan を実行]
  V --> P[公開 intent を保存]
  P --> G[target を正確に R へ更新]
  G --> D[条件付き Card Done]
  D --> M[semantic outbox と後続 Wiki]
```

B は受付時の表示値ではなく、queue 先頭になって preparation を開始した時点の target commit である。agent には固定 OID の履歴を保持した merge、全選択要件の維持、source refs・target・Card・canonical Wiki を変更しないことを要求する。host は clean な R が B と全採用 source の子孫であることを確認する。既に充足している `R=B` も許し、空 commit を作る必要はない。[B の固定](../../crates/server/src/routes/integrations/runtime.rs#L385-L425)、[要求された統合方針](../../crates/server/src/routes/integrations/runtime.rs#L173-L186)、[候補検査](../../crates/server/src/routes/integrations/runtime.rs#L280-L323)

agent の最終結果は選択集合と一致する JSON proposal で、根拠・環境要件・相対 cwd を伴う検証 plan を必要とする。少なくとも一つの required command が必要で、空 command や `true` / `exit 0` / `:` は拒否する。通常 Code mode を要求し、独立 Goal / Plan 継続に公開を委ねない。[proposal 検査](../../crates/server/src/routes/integrations/runtime.rs#L94-L159)、[mode 制限](../../crates/server/src/routes/integrations/admission.rs#L203-L208)

## Agent の自己申告と host の検証を分ける

host は固定 R 上で plan の各 command を Bash の `IntegrationValidation` ExecutionProcess として逐次実行する。cwd の canonical path が Repo 外へ出ないことを検査し、各 command の前後に clean な同一 R を要求する。一 command の上限は1時間で、終了投影後も after-HEAD 記録と script cleanup の収束を待つ。required の失敗、timeout、未収束では公開しない。[実行と収束](../../crates/server/src/routes/integrations/runtime.rs#L538-L641)

公開直前には process の Session・reason・非 dropped・終了 code、実際の command / cwd / context と保存済み plan の一致、before/after commit が共に R であることを再検証する。ログ内の「成功」や agent のテスト報告はこの証拠を代替しない。[照合](../../crates/server/src/routes/integrations/runtime.rs#L807-L857)、[異なる commit / cwd / command を拒否するテスト](../../crates/server/src/routes/integrations/runtime_tests.rs#L127-L220)

これらは **提案された plan が R 上で実行された**ことの証拠である。plan がすべての必要テストを意味的に網羅したことや、agent が要件を正しく解釈したことの機械的な証明ではない。prompt は検証の根拠と正当な除外理由を求めるが、その内容もレビュー対象になる。[plan の品質要求](../../crates/server/src/routes/integrations/runtime.rs#L180-L186)

## 公開と取消の境界

source と target writer を再確認し、target がまだ B である場合にだけ公開へ進む。DB の writer transaction で cancellation と `publication_intent` のどちらか一方を確定してから Git を変更する。この境界を越えた Cancel は Git undo にならない。[公開受付](../../crates/server/src/routes/integrations/runtime.rs#L861-L904)、[競合の確定](../../crates/db/src/models/integration.rs#L147-L169)

Git publication は新たな merge commit を生成せず、ref lock 下で B と比較して **検証済み R そのもの**へ target を進める。target checkout があれば許可された managed path・clean 状態・一つだけの checkout を要求し、ignored なローカルファイルも R と衝突すれば保護する。index と実ファイルを更新してから ref を commit するため、この二つと DB を一 transaction とみなさない。[公開機構](../../crates/git/src/publication.rs#L226-L319)、[衝突保護](../../crates/git/src/publication.rs#L21-L71)

## Git・Done・Wiki は独立した結果

Git publication を確認した後だけ Card 完了を試みる。source / link が変わった、別 run が予約した、title・description・status・revision が凍結値と違う、可視 Done 列がない、という場合は Card を変えず理由を保存する。Card の変更と `done_result` は同一 DB transaction で確定するため、後で人が再開した Card を古い回復処理が再び Done にしない。変更して元に戻した要件も revision が異なるので対象外になる。[source の再確認](../../crates/server/src/routes/integrations/runtime.rs#L1015-L1075)、[条件付き更新](../../crates/db/src/models/integration.rs#L184-L235)、[revision と rollback のテスト](../../crates/db/src/models/integration.rs#L717-L781)

Memory 有効時は凍結した source event IDs と統合固有 Manifest を既存 outbox へ結び付ける。Memory 無効時は新しい memory 記録を作らず、別 target 設定ならその旨を残す。semantic outbox 失敗や後続 Wiki 失敗は成功済み Git / Done を取り消さない。表示上の Wiki acknowledgement は各 receipt を読んで判定し、Integration succeeded だけで Wiki 完了としない。[後処理](../../crates/server/src/routes/integrations/runtime.rs#L990-L1011)、[event 帰属](../../crates/server/src/routes/integrations/runtime.rs#L1093-L1159)、[Wiki 結果の投影](../../crates/server/src/routes/integrations/mod.rs#L113-L168)。記憶の詳しい寿命は [Repository Memory](repository-memory.md)を参照する。

## 失敗と回復

公開前の取消は子 agent と検証 script を止め、実際の writer が idle になってから予約を解放する。preparation の中断は起動時に fence し、validation の中断は副作用のある command を再実行せず新しい Integration を要求する。[取消](../../crates/server/src/routes/integrations/runtime.rs#L644-L742)、[startup fence](../../crates/db/src/models/integration.rs#L138-L145)、[validation 中断](../../crates/server/src/routes/integrations/runtime.rs#L365-L371)

公開 intent 後の障害は `recovery_required` となる。Recover は後処理の回復だけに限定され、再 merge や再検証を同じ run で始めない。Git の証拠が R の適用を示せば receipt と後処理を続け、ref / index / files が B のままなら未適用として blocked にする。B の ref と R の files が混在する途中状態や不明な ref は推測で reset しない。確定済み publication receipt があるときだけ、その後の正当な target 前進・利用者編集を許容できる。[回復入口](../../crates/server/src/routes/integrations/mod.rs#L182-L195)、[照合と後処理](../../crates/server/src/routes/integrations/runtime.rs#L934-L988)、[Git 証拠](../../crates/git/src/publication.rs#L82-L132)、[部分更新を保持するテスト](../../crates/git/src/publication.rs#L465-L501)

standalone と embedded の monitor 配線の差は [起動順序](../architecture/system.md#起動時は内側の所有者から回復する)に記録する。実行専用 Workspace の結果・ログは成功後も保持され、通常開発用に転用されない。
