---
type: integration
title: Cloud Review と GitHub App
description: 組織の GitHub App 設定から webhook、自動レビュー、外部 worker、結果通知までの境界と失敗条件。
tags: [github, review, organization, webhook, cloud]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-15T17:45:17.356Z
sources:
  - id: openwiki-source-d6feebf72416d6504b5d2b00
    resource: repo://crates/remote/src/config.rs
  - id: openwiki-source-bb59e74539bbfeee865028d3
    resource: repo://crates/remote/src/db/github_app.rs
  - id: openwiki-source-454bc09c273f78c9b9193b0d
    resource: repo://crates/remote/src/db/reviews.rs
  - id: openwiki-source-219b8c8774f9360a647d00af
    resource: repo://crates/remote/src/github_app/pr_review.rs
  - id: openwiki-source-992fd80dd32889d5f5a6bd76
    resource: repo://crates/remote/src/github_app/webhook.rs
  - id: openwiki-source-7943a218d28bcc247061fabd
    resource: repo://crates/remote/src/routes/github_app.rs
  - id: openwiki-source-8836879a3c811b7b2d0ff602
    resource: repo://crates/remote/src/routes/mod.rs
  - id: openwiki-source-502510e01f3135fb1c5219e4
    resource: repo://crates/remote/src/routes/review.rs
  - id: openwiki-source-27c2433bab843f01a51418b1
    resource: repo://crates/server/src/routes/workspaces/pr.rs
  - id: openwiki-source-1414db6270aaae415e25f73a
    resource: repo://docs/cloud/troubleshooting.mdx
  - id: openwiki-source-4815b99ed5b9f7632484ef04
    resource: repo://docs/integrations/github-integration.mdx
generated: { by: "codex", at: "2026-09-15T17:45:17.356Z" }
---

# Cloud Review と GitHub App

Cloud Review は、PR のコードを別の review worker に渡して結果を公開する Remote の機能である。組織に接続した **GitHub App** は、installation とアクセス可能な Repository を管理し、GitHub イベントからこの機能を起動する。EVK の Workspace 内で coding agent を動かす [Agent review](../workflows/task-to-integration.md#3-会話を続け成果物を確認する) や、[Workflow の graph 実行](../concepts/workflow-attempt.md) とは実行主体・保存モデルが異なる。[Remote の公開・保護 router](../../crates/remote/src/routes/mod.rs#L103-L140)、[App の入口](../../crates/remote/src/routes/github_app.rs#L26-L56)

## 組織への接続と設定の寿命

GitHub App のサーバー設定には app ID、秘密鍵、webhook secret、slug が必要で、app ID がなければ連携は無効になる。レビューにはさらに R2 と `REVIEW_WORKER_BASE_URL` が必要で、`REVIEW_DISABLED` はレビュー起動を止める。[実設定](../../crates/remote/src/config.rs#L156-L202)、[review 設定](../../crates/remote/src/config.rs#L249-L258)

接続開始と repository ごとの review 有効化・無効化、接続削除は [Organization の Admin](../concepts/organization-and-membership.md) を要求する。個人用 organization へのインストールは拒否する。開始時に 10 分有効の state と pending installation を保存し、callback は state の形式・期限・保存済み pending を確認して GitHub から installation 情報を取得し、組織との対応を保存する。[開始条件](../../crates/remote/src/routes/github_app.rs#L124-L184)、[callback 検証](../../crates/remote/src/routes/github_app.rs#L494-L566)、[review 設定変更](../../crates/remote/src/routes/github_app.rs#L283-L340)

組織設定の接続削除は **EVK 側の installation record の削除**であり、GitHub 側の App uninstall を行わない。GitHub の deleted / suspend / unsuspend イベントは保存状態を更新し、repository 追加・除外イベントはアクセス対象を同期する。これらの後処理にはログを残して 200 を返す失敗経路もある。[削除の範囲](../../crates/remote/src/routes/github_app.rs#L252-L280)、[installation 更新](../../crates/remote/src/routes/github_app.rs#L667-L710)、[repository 同期](../../crates/remote/src/routes/github_app.rs#L713-L799)

## 起動イベントとスキップ条件

webhook は本文と `X-Hub-Signature-256` の HMAC-SHA256 を検証する。署名不正は 401、JSON 不正は 400、App 未設定は 501。未対応イベントは 200 で無視する。[webhook 入口](../../crates/remote/src/routes/github_app.rs#L609-L662)、[検証関数とテスト](../../crates/remote/src/github_app/webhook.rs)

| イベント | 起動条件 |
| --- | --- |
| `pull_request` | action が `opened`。この経路は pending review の事前確認を行わない |
| `issue_comment` | action が `created`、対象が PR、前後空白を除いた本文が厳密に `!reviewfast`、投稿者が Bot 以外。既存 pending review があればスキップ |

[pull_request 分岐](../../crates/remote/src/routes/github_app.rs#L939-L990)、[comment 分岐](../../crates/remote/src/routes/github_app.rs#L993-L1047)

共通処理は review の全体無効化、installation 不在・停止、repository の review 無効化、R2/worker 未設定などを調べる。ただし repository の行がない場合は「all repos」向けに有効を既定値とし、呼出側はその DB 問合せエラーでも有効へ fallback する。pending 問合せのエラーも「pending なし」として続ける。常に安全側へ停止する設定検査とは説明できない。[起動条件と fallback](../../crates/remote/src/routes/github_app.rs#L821-L872)、[行がない場合の記録された理由](../../crates/remote/src/db/github_app.rs#L434-L452)

## コード取得から結果通知まで

起動条件を通過すると、handler は `tokio::spawn` に処理を渡す。background 処理は installation token で PR head を clone し、base branch との merge-base を計算して archive を作り、R2 へ upload、review DB record 作成、worker の `/review/start` 呼出しの順に進む。worker へ渡す callback URL は Remote の review URL である。[background 起動](../../crates/remote/src/routes/github_app.rs#L896-L936)、[installation token と clone](../../crates/remote/src/github_app/service.rs#L274-L390)、[実行順](../../crates/remote/src/github_app/pr_review.rs#L97-L189)

この受付は [Workflow の永続 outbox](../architecture/workflow-runtime.md) を使っていない。DB record を作る前にも clone/upload があり、途中で失敗すれば後続へ進まず呼出側がログに残す。worker 呼出し失敗時は既に R2 と DB record が作られている。pending の確認と record 作成も一つの atomic claim ではないため、一回だけ実行される保証は読み取れない。**webhook の 200 は、レビュー開始・完了・再起動後の再開を証明しない。** これはソースの順序に基づく制限で、外部サービスを含む障害試験の結果ではない。[部分失敗](../../crates/remote/src/github_app/pr_review.rs#L125-L184)、[handler の応答](../../crates/remote/src/routes/github_app.rs#L986-L990)、[pending の照会](../../crates/remote/src/db/reviews.rs#L253-L278)

worker の success/failed callback は DB の状態を先に更新し、webhook review なら GitHub PR に結果リンクまたは失敗コメントを投稿する。通常 upload 由来なら登録 email に通知する。PR コメント失敗はログに留めて 200 を返すので、DB の完了状態と通知の配達は分けて調べる。[成功・失敗 callback](../../crates/remote/src/routes/review.rs#L395-L502)

未解決の配備前提として、PR review の `r2_public_url()` は worker base URL をコード配信用 proxy と仮定して返す。コード自身が将来は別設定にすべきと記している。この checkout だけで R2 の公開読取やその proxy が実際に稼働するとは確定できない。[記録された仮定](../../crates/remote/src/github_app/pr_review.rs#L150-L199)

## 公開 upload API と外部 worker の境界

GitHub App 以外にも `/review/init` で review ID・R2 upload URL・DB record を作る公開経路がある。IP ごとの上限は 1 分 2 件、1 時間 20 件で、init は IP、review 有効状態、R2 設定を検査する。`/review/start`、status、結果、file、diff は別設定の worker への proxy であり、worker 未設定は明示エラーとなる。upload 初期化の成功を worker の準備完了とみなさない。[公開 router](../../crates/remote/src/routes/review.rs#L21-L32)、[初期化](../../crates/remote/src/routes/review.rs#L179-L249)、[worker への転送](../../crates/remote/src/routes/review.rs#L252-L392)

ここで扱うのは Remote の契約であり、外部 worker の内部実装・出力品質・稼働状態ではない。[OpenWiki の独立 Review](../operations/openwiki-maintenance.md) も、この公開 review record とは別の lifecycle である。

## 文書と通常の PR 操作を使い分ける

廃止表示のない [Cloud troubleshooting の GitHub 節](../../docs/cloud/troubleshooting.mdx#github-integration-issues) は、組織設定で App の接続・repository access・Code Review を確認する運用意図を記録しており、上記の設定経路に対応する。一方、同節の Issue 自動リンク手順まで、このレビュー実装だけで証明したとは扱わない。

[GitHub Integration](../../docs/integrations/github-integration.mdx) の「GitHub CLI を認証すればよく、Settings の設定は不要」という説明は、手動 PR 作成の文脈である。実際の Workspace PR 作成も GitHostService へ渡る。[手動 PR の実装](../../crates/server/src/routes/workspaces/pr.rs#L305-L340)。組織 App の設定を不要にする説明ではなく、[依頼から統合までの PR 操作](../workflows/task-to-integration.md) とこのページを使い分ける。
