---
type: concept
title: Arena による比較と選択
description: 同じ Issue に対する複数の Workspace を比較する Arena の意味、Design と Implementation の契約、選択・再試行・終了の寿命。
tags: [arena, workspace, comparison, lifecycle]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-15T18:24:07.449Z
sources:
  - id: openwiki-source-2a51fb8409723c3914db4954
    resource: repo://crates/db/src/models/arena_group.rs
  - id: openwiki-source-343f68138e6e2d3312d2774c
    resource: repo://crates/local-deployment/src/container.rs
  - id: openwiki-source-1ffb01d1ba3953937e0771bf
    resource: repo://crates/server/src/routes/local_remote.rs
  - id: openwiki-source-13bc408495036974093d65eb
    resource: repo://crates/server/src/workflow_runtime/arena.rs
generated: { by: "codex", at: "2026-09-15T18:24:07.449Z" }
---

# Arena による比較と選択

Arena は、一つのローカル [Issue](project-and-issue.md)について複数の実施候補を保持する group である。各 candidate は独立した [Workspace](workspace.md)と会話を持つ。group の選択状態と agent の稼働状態は別であり、差分が空でも会話としての成果があり得る。[モデル](../../crates/db/src/models/arena_group.rs#L25-L86)、[candidate 作成](../../crates/server/src/routes/local_remote.rs#L2337-L2386)

## Design と Implementation

| mode | 主な意味 | 実装上の区別 |
| --- | --- | --- |
| Design（既定） | 方案を読み、追問・比較して方向を決める | Open の間は EVK の既定 commit 方針を抑制 |
| Implementation | コードの実施候補を比較する | 従来の diff・promote 経路を利用 |

[Arena v2 仕様](../../docs/future/ai-arena/spec-v2.md)は Draft 表記だが、この mode 分離は現行モデルに実装されている。仕様には「diff 統計だけでは設計判断に足りない」という記録された動機がある。そこから全提案 UI が完成済みだとは推論しない。

Open Design の既定 commit 抑制は provider の任意操作を禁止する sandbox ではない。ImplementationStarted では通常方針へ戻る。[policy とテスト](../../crates/local-deployment/src/container.rs#L2082-L2109)。Design を promote する API は、その前に ImplementationStarted であることを要求する。[promote guard](../../crates/server/src/routes/local_remote.rs#L3093-L3099)

## 三種類の状態を混同しない

- group の lifecycle: `open`、`closed`、`adopted`、`implementation_started`。
- Workspace の Arena 内の立場: `active`、`promoted`、`archived`。
- Workspace の通常 `archived` flag、および最新 AgentRun status。

Arena の Archived は負けた候補・置き換えられた候補を示し、通常の soft archive とは直交する。閉じた group は promote 済みでなくても active group の検索から外れる。[状態の定義](../../crates/db/src/models/arena_group.rs#L8-L53)、[closed group のテスト](../../crates/db/src/models/arena_group.rs#L424-L453)

## 作成・追問・選択

通常の Arena 作成 API は 2 候補以上、project ごとの上限（2〜6 に制限）、空でない prompt、少なくとも一つの Repository、Issue と Project の整合性を検証する。同じ Issue に active group があれば拒否する。候補は順番に作成・起動するため、全候補の構築が単一 transaction で成功するわけではない。[検証と作成](../../crates/server/src/routes/local_remote.rs#L2480-L2574)

message API の操作として AskAll、特定 Workspace、Challenge、Synthesize を記録できる。Start implementation は既存候補 Workspace を group の実装対象として記録し、非空の follow-up がある場合に実行を開始する。「実装開始」を新規 worktree 作成や merge と解釈しない。[操作型](../../crates/server/src/routes/local_remote.rs#L1955-L1976)、[実装開始](../../crates/server/src/routes/local_remote.rs#L2656-L2696)

通常 Arena の Promote は winner を記録し、他候補の Arena status と通常 archive flag を更新する。処理内で同期的に worktree を削除せず、Git merge も行わない。選ばれた実装はその後の [レビュー・PR・統合](../workflows/task-to-integration.md)へ進む。[Promote](../../crates/server/src/routes/local_remote.rs#L3082-L3139)

## Retry・Close・Dissolve と部分失敗

Retry は旧候補を Arena 内で Archived にし、同じ Repository 群を使う新候補を作る。旧候補の通常 archive flag は変更せず、履歴を残す。これは同じ AgentRun の RunAttempt retry とは別操作である。[Retry](../../crates/server/src/routes/local_remote.rs#L3142-L3212)

Close は Open group を Closed にする。Dissolve は未 promote group の全 Workspace を archive し、group を削除する。これらの handler は「プロセス停止完了」を意味しない。[Close](../../crates/server/src/routes/local_remote.rs#L2634-L2653)、[Dissolve](../../crates/server/src/routes/local_remote.rs#L3224-L3248)

候補作成が途中で失敗すると、cleanup は既存候補を archive して group を削除しようとする。cleanup 自体の失敗は warning に記録し、元の作成エラーを返す。残存 Workspace と実行状態を確認すべき境界である。[失敗処理](../../crates/server/src/routes/local_remote.rs#L2560-L2572)、[cleanup](../../crates/server/src/routes/local_remote.rs#L2598-L2609)

## Workflow 内の Arena

[Workflow](workflow-attempt.md) の Arena node は Implementation group を作る。winner 選択では candidate と主 Workspace の Repository を対応させ、candidate の base からの patch を主作業木へ適用する。通常 Arena の Promote と異なりコード反映を伴う。複数 Repository は順に適用し、途中の失敗時にはそれ以前の反映を一括 rollback する transaction はこのループにない。[winner 反映](../../crates/server/src/workflow_runtime/arena.rs#L712-L793)
