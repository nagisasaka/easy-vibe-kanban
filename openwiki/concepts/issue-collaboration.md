---
type: concept
title: Issue の添付・購読・通知
description: Cloud の Issue・Comment に付随するファイルの確定と保存寿命、Assignee・Follower の通知対象、同期と部分失敗の契約。
tags: [issue, collaboration, attachments, notifications, lifecycle]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-15T18:24:07.449Z
sources:
  - id: openwiki-source-63b741b02f122d47e34fd409
    resource: repo://crates/api-types/src/attachment.rs
  - id: openwiki-source-5c0e06d4a816ba8592d7e625
    resource: repo://crates/remote/docker-compose.yml
  - id: openwiki-source-e729d022cb0491fed02a1692
    resource: repo://crates/remote/README.md
  - id: openwiki-source-ddffaf1883a26ff98da34ab8
    resource: repo://crates/remote/src/app.rs
  - id: openwiki-source-222defea42d9bcd621254151
    resource: repo://crates/remote/src/attachments/cleanup.rs
  - id: openwiki-source-838658819b62d906d6d9184d
    resource: repo://crates/remote/src/db/attachments.rs
  - id: openwiki-source-163210c0a2c86b39d845470f
    resource: repo://crates/remote/src/db/blobs.rs
  - id: openwiki-source-ac9e9bdf1e0d01f8259a74db
    resource: repo://crates/remote/src/notifications.rs
  - id: openwiki-source-393b5e3a8041c755c711162c
    resource: repo://crates/remote/src/routes/attachments.rs
  - id: openwiki-source-95a895baa747a1a77dc12a32
    resource: repo://crates/remote/src/routes/issue_comments.rs
  - id: openwiki-source-7b11311180f556a001c02e54
    resource: repo://crates/remote/src/routes/issues.rs
  - id: openwiki-source-abcea000192f51e92efef423
    resource: repo://crates/remote/src/routes/notifications.rs
  - id: openwiki-source-1ffb01d1ba3953937e0771bf
    resource: repo://crates/server/src/routes/local_remote.rs
  - id: openwiki-source-bdb4dae5722fe76ec5e44001
    resource: repo://crates/server/src/routes/workspaces/create.rs
  - id: openwiki-source-379c8cd3e1e9932f9adda9ee
    resource: repo://crates/services/src/services/container.rs
  - id: openwiki-source-4251300987fd63068dbcfd5d
    resource: repo://crates/services/src/services/notification.rs
  - id: openwiki-source-64c9f3fab4fe77c2cfdd50f6
    resource: repo://docs/settings/general.mdx
  - id: openwiki-source-1a337d16d294c4dc710f105d
    resource: repo://packages/web-core/src/pages/kanban/KanbanIssuePanelContainer.tsx
  - id: openwiki-source-84ce6eab6a49372dce37fcb2
    resource: repo://packages/web-core/src/shared/hooks/useAzureAttachments.ts
  - id: openwiki-source-2bc4d71db2cf73c3210b9268
    resource: repo://packages/web-core/src/shared/hooks/useNotifications.ts
  - id: openwiki-source-12989c758da3bf15977fb603
    resource: repo://packages/web-core/src/shared/lib/notifications.ts
generated: { by: "codex", at: "2026-09-15T18:24:07.449Z" }
---

# Issue の添付・購読・通知

[Project・Issue](project-and-issue.md)の仕事には、本文・Comment、参照ファイル、担当者・購読者が関連する。Cloud でのアクセスは [Organization の所属](organization-and-membership.md)を基盤とするが、ファイル保存・関連付け・通知は別の処理であり、一つの成功応答から全ての完了を推測しない。

## Attachment と Blob

**Blob** は Project 内の保存実体と hash・名前・MIME 等を持つ。**Attachment** は Blob と Issue または Comment を結ぶレコードで、独自 ID と任意の期限を持つ。同じ Blob を複数 Attachment が参照できる。ローカル [Session 添付の File](../operations/workspace-inspection.md#session-の添付と-cloud-attachment)とは識別子・保存先が異なる。[型](../../crates/api-types/src/attachment.rs#L6-L35)・[Blob の Project 内検索](../../crates/remote/src/db/blobs.rs#L47-L81)

### アップロードから関連付けまで

1. UI はファイル hash を計算し Project を指定して初期化する。server は Project access と20 MiB上限を検査する。同一 Project / hash の Blob があれば `skip_upload`、なければ Azure の upload URL と pending upload を作る。
2. UI は必要な場合だけ Azure へ転送し、`confirm` を呼ぶ。server は既存 Blob を再利用するか、転送先を検査し、必要な thumbnail と Blob を作成して Attachment を返す。
3. confirm 時に Issue / Comment ID があれば期限なしで関連付ける。未関連なら `expires_at = now + 24時間` の staged Attachment になる。転送が100%でも confirm・関連付けの成功は別である。
4. Issue / Comment 作成後に commit API が未関連 Attachment の参照先を設定し、期限を消す。SQL は既に関連のある Attachment を付け替えない。Issue 作成 UI は保存された Issue を待ち、本文でまだ参照中の upload ID だけ commit し、使わなくなった添付の削除を試みる。

[UI の転送・confirm](../../packages/web-core/src/shared/hooks/useAzureAttachments.ts#L228-L310)・[初期化](../../crates/remote/src/routes/attachments.rs#L171-L228)・[confirm と期限](../../crates/remote/src/routes/attachments.rs#L230-L328)・[commit](../../crates/remote/src/db/attachments.rs#L224-L303)・[Issue 保存後の選別](../../packages/web-core/src/pages/kanban/KanbanIssuePanelContainer.tsx#L882-L908)

取得・削除は関連 Issue、Comment、または staged Blob の Project から access を検査する。URL の取得、Attachment ID、Blob ID を同一の権限証明として使わない。[access 解決](../../crates/remote/src/routes/attachments.rs#L485-L507)

### 保存寿命と清掃

Azure が設定された Remote は清掃 task を開始する。期限を過ぎた未関連 Attachment と未完了 pending upload を別々に処理し、既定1時間周期で sweep する（`ATTACHMENT_CLEANUP_INTERVAL_SECS` で変更可能）。24時間は清掃対象となる期限であり、その瞬間の物理削除保証ではない。[起動](../../crates/remote/src/app.rs#L181-L187)・[周期と対象](../../crates/remote/src/attachments/cleanup.rs#L15-L61)

Attachment 削除後に Blob の残る参照数を確認し、0 の場合だけ Blob レコード・Azure 本体・thumbnail を削除する。明示削除も同様の参照確認を行う。DB 更新と Azure 削除は一括 transaction ではなく、Azure 削除失敗は警告になるため、DB 上の削除だけで物理データの消去完了とは判定できない。[期限清掃](../../crates/remote/src/attachments/cleanup.rs#L64-L103)・[明示削除](../../crates/remote/src/routes/attachments.rs#L453-L482)

[Remote README の attachments profile](../../crates/remote/README.md#local-setup)は廃止表示のない導入手順で、現行 Compose の Azurite profile と対応する。production の Azure 設定や実サービスの稼働は別途必要であり、この source 確認では接続していない。[Compose profile](../../crates/remote/docker-compose.yml#L22-L40)

### Workspace への取り込み

linked Issue から Workspace を作る経路は、remote client を取得できた場合に Cloud 添付をローカル File として取り込み、本文の `attachment://<ID>` を `.vibe-attachments/...` に書き換える。取り込み・関連付けの失敗は警告して進むため、Workspace 作成成功は添付の完備を保証しない。[取り込みの順序](../../crates/server/src/routes/workspaces/create.rs#L377-L416)・[ローカル参照の構築](../../crates/server/src/routes/workspaces/create.rs#L181-L201)

実行時の基準ディレクトリとパス検証は [Workspace の添付](../operations/workspace-inspection.md)、仕事から実行へ渡す順序は [Issue から統合まで](../workflows/task-to-integration.md)で確認する。Cloud 添付と作業ディレクトリのコピーを一つの保存寿命として扱わない。

## 購読と通知

**Assignee** は仕事の担当、**Follower** は変更を追う利用者の関連である。subscriber 通知の受信候補は両者の和集合から操作本人を除き、各 user の組織所属を再確認する。所属確認が失敗した user も送信先に含めない。担当者への直接通知 `notify_user` は別入口なので、この除外規則を全通知へ一般化しない。[subscriber の受信者](../../crates/remote/src/notifications.rs#L157-L184)・[直接通知](../../crates/remote/src/notifications.rs#L121-L155)

Issue の status / title / description / priority 変更、Comment 追加などが通知生成の入口になる。title 等は最近の通知を upsert する処理もある。Comment は先に保存され、その後に購読者へ通知を作る。受信者収集や各通知レコードの作成失敗は警告して戻るので、**Comment や Issue の更新成功は通知生成・閲覧の保証ではない。** [Issue の変更検出](../../crates/remote/src/routes/issues.rs#L55-L171)・[Comment 保存後](../../crates/remote/src/routes/issue_comments.rs#L99-L155)・[通知失敗](../../crates/remote/src/notifications.rs#L12-L79)

Cloud UI は user_id の notifications shape を購読し、REST mutation で `seen` を更新する。API の一覧はログイン user に限定され、取得・更新で他 user の通知は NotFound とする。[受信者の権限](../../crates/remote/src/routes/notifications.rs#L68-L168)・[同期 hook](../../packages/web-core/src/shared/hooks/useNotifications.ts#L10-L42)

表示は通知種別・操作本人・Issue 等で group 化し、group の未読 ID がなくなると既読とする。未読バッジは生レコード数ではなく未読 group 数なので、DB 件数と直接比較しない。[group と既読](../../packages/web-core/src/shared/lib/notifications.ts#L119-L177)・[件数](../../packages/web-core/src/shared/hooks/useNotifications.ts#L27-L34)

### Local の実行通知との違い

local 互換 API の通知一覧は空を返す。一方 `NotificationService` は設定に従って音と OS 向け push を呼び、Tauri は notifier を差し替えられる。呼出しの一例は既存 ExecutionProcess の終了処理である。Cloud Follower の受信箱、Web の通知 group、全 AgentRun の終了通知を同じ機構とみなさない。[local 一覧](../../crates/server/src/routes/local_remote.rs#L1406-L1408)・[notifier の構成と設定](../../crates/services/src/services/notification.rs#L10-L93)・[終了処理の呼出し](../../crates/services/src/services/container.rs#L257-L289)

[General settings の Notifications](../../docs/settings/general.mdx#notifications)は実行の完了・注意喚起を受け取る操作意図を説明する、廃止表示のない文書である。Cloud の subscriber 選択や配送保証の仕様としては適用しない。通知が見えない場合は、利用中の local/Cloud 経路、受信者・所属、通知作成のログ、shape / fallback、group の既読状態を順に切り分ける。[同期方式の境界](../architecture/web-and-sync.md)
