---
type: concept
title: Organization・Member・Invitation
description: Cloud の組織が所有するデータ、所属と管理権限、personal organization、招待の受理と最後の Admin の保護。
tags: [organization, membership, authorization, invitations, cloud]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-15T17:45:17.356Z
sources:
  - id: openwiki-source-ddffaf1883a26ff98da34ab8
    resource: repo://crates/remote/src/app.rs
  - id: openwiki-source-06595c5fce6c6294d5f1a358
    resource: repo://crates/remote/src/db/invitations.rs
  - id: openwiki-source-34ba69f4ccb4bf0744529416
    resource: repo://crates/remote/src/db/organizations.rs
  - id: openwiki-source-ade0a6fc0fb5d49065adb77e
    resource: repo://crates/remote/src/routes/organization_members.rs
  - id: openwiki-source-d0e56a8d1b6b799035132afd
    resource: repo://crates/remote/src/routes/projects.rs
  - id: openwiki-source-1ffb01d1ba3953937e0771bf
    resource: repo://crates/server/src/routes/local_remote.rs
  - id: openwiki-source-b1ac63cc9ffe8bae27ce96af
    resource: repo://docs/cloud/organizations.mdx
  - id: openwiki-source-5e3da7822fa02a87c001ef16
    resource: repo://docs/cloud/team-members.mdx
generated: { by: "codex", at: "2026-09-15T17:45:17.356Z" }
---

# Organization・Member・Invitation

**Organization** は Cloud の Project と利用者の所属をまとめる単位である。**Member** は user と organization の関係で、role は Admin / Member。**Invitation** はまだ所属していない人へ参加と role を渡す期限付き token であり、発行しただけでは membership を作らない。仕事そのものは [Project・Issue](project-and-issue.md)、ログインの寿命は [Cloud AuthSession](../integrations/remote-access.md#cloud-authsession-と-token-の寿命)が受け持つ。[所属モデル](../../crates/remote/src/db/organization_members.rs#L7-L76)・[招待の作成と受理](../../crates/remote/src/db/invitations.rs#L38-L95)

## 所属と管理権限

| 操作 | 現行の判定 |
| --- | --- |
| Project の取得・作成・変更・削除、Issue へのアクセス | 対象 Project / Issue の organization に所属すること |
| Organization の名前変更・削除 | Admin。personal organization の削除は拒否 |
| 招待の発行・一覧・取消、Member の削除・role 変更 | Admin。personal organization や自己変更に追加制約 |

Project / Issue の access helper は組織まで関係をたどって membership を検査し、通常の仕事へのアクセスに Admin を要求しない。Admin は組織管理上の権限であり、特定 Issue の Assignee とは別である。[Project の各入口](../../crates/remote/src/routes/projects.rs#L46-L96)・[変更と削除](../../crates/remote/src/routes/projects.rs#L137-L205)・[Issue の認可](../../crates/remote/src/routes/organization_members.rs#L604-L691)・[組織の管理](../../crates/remote/src/db/organizations.rs#L207-L276)

[Organizations](../../docs/cloud/organizations.mdx#what-is-an-organisation)と [Team Members の role 説明](../../docs/cloud/team-members.mdx#member-roles-explained)は廃止表示のない操作文書で、この所属と管理権限の分離に対応する。文書上の「full access」を、別ホストの作業ファイルや provider の権限まで含むものと読まない。[実行ホストの pairing](../integrations/remote-access.md#paired-host-の意味と寿命)は独立した関係である。

## 作成と personal organization

通常 organization の作成は組織・初期 Project（既定 Status / Tag）・作成者の Admin 所属を同じ transaction に収める。personal 作成経路では user ID 由来の slug で既存組織を探し、なければ初期 Project とともに作成してから Admin 所属を確保する。[通常作成](../../crates/remote/src/db/organizations.rs#L113-L164)・[personal の確保](../../crates/remote/src/db/organizations.rs#L60-L94)

personal organization は追加メンバーの招待・招待受理、メンバー変更、組織削除を拒否する。[personal の操作案内](../../docs/cloud/organizations.mdx#personal-organisation)がいう個人用の境界は、通常 team organization の名前を変えたものではない。[招待制約](../../crates/remote/src/db/invitations.rs#L47-L56)・[受理制約](../../crates/remote/src/db/invitations.rs#L227-L234)・[メンバー変更](../../crates/remote/src/routes/organization_members.rs#L376-L390)・[削除](../../crates/remote/src/db/organizations.rs#L240-L253)

local 互換 API は固定 UUID の Local user / organization と Admin を返す。これは Cloud の personal organization・実在の招待・複数 user の認可を実装した代替ではない。[固定 ID](../../crates/server/src/routes/local_remote.rs#L265-L271)・[Local の応答](../../crates/server/src/routes/local_remote.rs#L1204-L1234)

## 招待の通常フローと失敗

1. Admin が email と role を指定して発行する。server は UUID token と7日期限を作り、pending 招待を保存してから email を送る。同じ email の pending 招待との衝突はエラーになる。
2. token の参照は公開入口、accept はログイン済みの入口である。受理は `token` と現在の `user.id` を DB 処理へ渡す。
3. DB は pending token を `FOR UPDATE` でロックする。不存在・使用済み、personal、期限超過、既に所属済みなら拒否する。期限超過では expired 更新を commit する。
4. 有効なら membership 追加と accepted 更新を同じ transaction に収める。Admin の取消は対象 organization の招待レコードを削除する。

[発行と送信順](../../crates/remote/src/routes/organization_members.rs#L93-L149)・[公開/保護入口](../../crates/remote/src/routes/organization_members.rs#L34-L49)・[受理 user](../../crates/remote/src/routes/organization_members.rs#L274-L293)・[DB の受理](../../crates/remote/src/db/invitations.rs#L191-L285)・[取消](../../crates/remote/src/db/invitations.rs#L167-L190)

**現行 accept は招待 email とログイン user の email の一致を検査しない。** [Team Members](../../docs/cloud/team-members.mdx#inviting-team-members)はサインイン用 email へ送る操作を案内するが、それを server の email-binding 保証として扱わない。招待に記録された role は有効 token を受理したログイン user に付く。この意図の理由は記録から確定できない。[受理の全判定](../../crates/remote/src/db/invitations.rs#L191-L285)

mailer は Remote 起動設定で Noop にもなる。招待レコードの作成成功とメール到達は別に診断し、未着なら pending 状態・期限・mailer 設定を確認する。[mailer の選択](../../crates/remote/src/app.rs#L93-L112)

## 最後の Admin と変更時の不変条件

メンバー削除・降格は対象行と Admin 群を transaction 内でロックし、最後の Admin の除去・降格を Conflict で拒否する。さらに自己削除、Member への自己降格を BadRequest にする。最後の Admin 保護だけを残して自己変更の制約を落とさない。[削除 guard](../../crates/remote/src/routes/organization_members.rs#L363-L430)・[降格 guard](../../crates/remote/src/routes/organization_members.rs#L465-L540)

[役割移譲の操作意図](../../docs/cloud/team-members.mdx#transferring-ownership)は別の人へ Admin を付与してから所属を整理することを説明する。ここで確認したのは現在の SQL・handler の契約であり、同時変更の負荷試験やメール配送を実行した記録ではない。membership 変更は [Issue 購読通知の受信候補](issue-collaboration.md#購読と通知)にも影響するため、画面の role 表示だけを変更対象にしない。
