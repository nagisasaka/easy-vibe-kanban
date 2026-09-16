---
type: architecture
title: Remote サービスとホスト接続
description: Cloud のデータ・認証と、ローカル実行ホストへの pairing、署名、relay/WebRTC、モバイル直接接続を分けて説明する。
tags: [remote, authentication, relay, mobile, self-hosting]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-15T17:45:17.356Z
sources:
  - id: openwiki-source-d5ca02ab651771d5c3a01c6c
    resource: repo://crates/api-types/src/auth.rs
  - id: openwiki-source-b27269f8a0d5cbaf4935992c
    resource: repo://crates/relay-control/src/signing.rs
  - id: openwiki-source-ddffaf1883a26ff98da34ab8
    resource: repo://crates/remote/src/app.rs
  - id: openwiki-source-ab7664fd62abcc8bc828f5d5
    resource: repo://crates/remote/src/auth/jwt.rs
  - id: openwiki-source-d5e73c457d06008688054dda
    resource: repo://crates/remote/src/auth/middleware.rs
  - id: openwiki-source-8eef09b4d339788111832682
    resource: repo://crates/remote/src/auth/oauth_token_validator.rs
  - id: openwiki-source-d6feebf72416d6504b5d2b00
    resource: repo://crates/remote/src/config.rs
  - id: openwiki-source-10955693e5794ef292f6b976
    resource: repo://crates/remote/src/db/auth.rs
  - id: openwiki-source-81dada2f014b934994b8ed97
    resource: repo://crates/remote/src/routes/electric_proxy.rs
  - id: openwiki-source-1a9b02902c9bc5f78ff1da70
    resource: repo://crates/remote/src/routes/hosts.rs
  - id: openwiki-source-8836879a3c811b7b2d0ff602
    resource: repo://crates/remote/src/routes/mod.rs
  - id: openwiki-source-5369a9e10aec0bc6992eecbf
    resource: repo://crates/remote/src/routes/organizations.rs
  - id: openwiki-source-027ba5c840b03ed97ec1aaf8
    resource: repo://crates/remote/src/routes/tokens.rs
  - id: openwiki-source-c2eaddb5c69f49c5ee934a63
    resource: repo://crates/server/src/middleware/origin.rs
  - id: openwiki-source-9ad2991837fb49edb00ea427
    resource: repo://crates/server/src/middleware/relay_request_signature.rs
  - id: openwiki-source-a488138289c401dc3cd2bd9b
    resource: repo://crates/server/src/relay_pairing/server.rs
  - id: openwiki-source-44a3663cca1c90812a1a6640
    resource: repo://crates/server/src/routes/relay_auth/server.rs
  - id: openwiki-source-d721cae86e7d99c5765ef7e5
    resource: repo://crates/trusted-key-auth/src/runtime.rs
  - id: openwiki-source-502cda8b7eeb4e7c2ce3d710
    resource: repo://crates/trusted-key-auth/src/trusted_keys.rs
  - id: openwiki-source-2e73e6d6d9262c17a3148217
    resource: repo://docs/ai-mobile/design.md
  - id: openwiki-source-74a1893f4c1e77eb654658f7
    resource: repo://docs/ai-mobile/tailscale-local-access.md
  - id: openwiki-source-8213b6be875ac110cc608b1b
    resource: repo://docs/cloud/authentication.mdx
  - id: openwiki-source-25f7171a9e4f7bc1722ae461
    resource: repo://docs/self-hosting/deploy-docker.mdx
  - id: openwiki-source-3ecddc0be7b037ea6ab24381
    resource: repo://packages/local-web/vite.config.ts
  - id: openwiki-source-583dbd3b3158283fd7ebc798
    resource: repo://packages/remote-web/src/shared/lib/relayHostApi.ts
  - id: openwiki-source-3e761b9a1df3da8cd157a815
    resource: repo://packages/remote-web/src/shared/lib/webrtc/transport.ts
  - id: openwiki-source-d4726fdd33da0f7c2f1ae1ee
    resource: repo://packages/web-core/src/shared/lib/remoteApi.ts
generated: { by: "codex", at: "2026-09-15T17:45:17.356Z" }
---

# Remote サービスとホスト接続

Remote は共有データを扱うサービスと、実行ホストへ到達する経路を組み合わせる。Cloud の Project/Issue を読むことと、ホスト上の [Workspace](../concepts/workspace.md) を操作することでは認証・接続先が異なる。[Web と同期](../architecture/web-and-sync.md) は画面がこの二つの API を選ぶ仕組みを説明する。

## Cloud のデータ境界

remote-server は PostgreSQL へ接続し migration を実行した後、Electric の role/password と publication を準備する。認証は GitHub / Google OAuth と self-host 用 local auth を登録し、いずれもなければ起動を失敗させる。[起動順](../../crates/remote/src/app.rs#L33-L78)

保護された /v1 router に session middleware を掛け、組織取得はさらに membership を検証する。host 一覧も user にアクセス可能な host を問い合わせる。ログイン済みというだけで任意の組織・host ID を読める契約ではない。[router](../../crates/remote/src/routes/mod.rs#L112-L140)・[組織の権限](../../crates/remote/src/routes/organizations.rs#L95-L109)・[host 一覧](../../crates/remote/src/routes/hosts.rs#L16-L29)

Electric の shape proxy は table と WHERE をサーバー側で設定し、client が渡す同期パラメーターを allowlist で絞る。この制約は [同期の仕組み](../architecture/web-and-sync.md) を変える際にも維持する。[shape の境界](../../crates/remote/src/routes/electric_proxy.rs#L41-L79)

## Cloud AuthSession と token の寿命

**AuthSession** は PostgreSQL の `auth_sessions` に保存する、user に属した Cloud ログインの状態である。coding-agent の [Session](../concepts/session-and-agent-run.md) や、次節のホスト署名用 session とは別物である。保護 API は Bearer access token を復号・検証した後、DB の session の存在、revoked_at、未使用期間、user の存在を検査し、token の user ID とも照合する。DB の読取障害は 500、無効な認証は 401 になる。[保存モデル](../../crates/remote/src/db/auth.rs#L23-L84)、[認証の検査順](../../crates/remote/src/auth/middleware.rs#L68-L155)

| 対象 | 現行の寿命・判定 |
| --- | --- |
| Access token | 発行時から 120 秒。署名が有効でも DB session の失効を回避できない |
| Refresh token | その token の発行時刻から 365 日。更新時に ID を入れ替える |
| AuthSession の未使用期限 | last_used_at、なければ created_at から **365 日超**。保護 API の middleware が失効処理を試みて 401 を返す |
| 最終使用日時 | 認証成功時に日単位へ丸めて更新する。更新エラーはログに留まり、認証自体は成功する |

[token の発行](../../crates/remote/src/auth/jwt.rs#L22-L23)、[期限の組立て](../../crates/remote/src/auth/jwt.rs#L117-L148)、[未使用期間](../../crates/api-types/src/auth.rs#L6-L26)、[session 定数と touch](../../crates/remote/src/db/auth.rs#L23-L23)、[日単位更新](../../crates/remote/src/db/auth.rs#L87-L102)、[失効と更新エラー](../../crates/remote/src/auth/middleware.rs#L118-L169)

refresh は現在の token ID を条件に DB transaction で入れ替え、直前の ID と猶予期限を保存し、旧 token を rotation として失効記録へ追加する。`REFRESH_TOKEN_OVERLAP_SECS` の既定値は **60 秒**、有効範囲は 0–300 秒。まだ失効していない session の直前 token が猶予内に再送された場合は、再度 rotate せず現在の refresh ID・発行時刻から応答を作る。同時更新に負けた経路も最新 session を読み直して同じ猶予を検査する。[atomic な更新](../../crates/remote/src/db/auth.rs#L105-L155)、[設定範囲](../../crates/remote/src/config.rs#L228-L232)、[直前 token と競合の処理](../../crates/remote/src/routes/tokens.rs#L197-L258)

猶予外の旧 token 再使用は対象 AuthSession を失効させる。通常の rotation が残す失効記録だけで session 全体の失効とは判断せず、`session.revoked_at` を先に確認する。local provider 以外は更新時に OAuth token も検証し、provider 無効などのエラーは全 user sessions の失効を試みる一方、一時的な ValidationUnavailable ではその処理をしない。[再使用境界](../../crates/remote/src/routes/tokens.rs#L110-L155)、[provider 検証](../../crates/remote/src/routes/tokens.rs#L183-L192)、[失効範囲](../../crates/remote/src/auth/oauth_token_validator.rs#L50-L83)。ブラウザーの Cloud API は 401 後に refresh を試み、token を得たら元の要求を一度再送する。[client 側の再送](../../packages/web-core/src/shared/lib/remoteApi.ts#L98-L135)

廃止表示のない [Authentication 文書](../../docs/cloud/authentication.mdx#troubleshooting) は OAuth の操作手順と「再起動で全 session が失効」「7 日未使用で失効」という説明を含む。後二者はこの checkout の永続 DB session と 365 日定数に一致しない。JWT secret は起動ごとの生成ではなく環境設定から読み、**同じ DB と secret を維持する再起動そのものを全失効の契機とは扱わない**。secret 変更や DB 消失などは別条件である。[文書の記載](../../docs/cloud/authentication.mdx)、[secret の読込](../../crates/remote/src/config.rs#L378-L383)。これは静的な照合であり、稼働環境の再起動・競合試験の結果ではない。

Cloud 認証後に何を操作できるかは [Organization・Member・Invitation の権限](../concepts/organization-and-membership.md) で決まる。[Issue の添付・購読・通知](../concepts/issue-collaboration.md) はこの共有データ側に属し、組織の [GitHub App と Cloud Review](github-review.md) にはさらに installation と repository の設定が必要になる。

## Paired host の意味と寿命

Paired host は、Cloud の host ID に加えて、client がローカルホストとの鍵の関係を持つ状態である。通常の操作手順は [Remote Access](../../docs/remote-access.mdx) に集約されている。実装では pairing code を relay 経由で取得することを拒否し、SPAKE2 enrollment の finish で client proof を検証した後、trusted client の公開鍵を保存して signing session を生成する。[code の取得制限](../../crates/server/src/routes/relay_auth/server.rs#L52-L68)・[enrollment 完了](../../crates/server/src/relay_pairing/server.rs#L164-L209)

trusted client は trusted_ed25519_public_keys.json に保存される。一方 signing session はメモリー上にあり、作成時刻と最終使用時刻による期限を持つ。永続的な pairing と一時的な session ID を同じものとして保存・失効処理を設計しない。[鍵の保存](../../crates/trusted-key-auth/src/trusted_keys.rs#L11-L50)・[session 保存と期限](../../crates/relay-control/src/signing.rs#L134-L139)・[期限検証](../../crates/relay-control/src/signing.rs#L291-L303)

refresh は保存済み client、timestamp、未使用 nonce、署名を検証して新 session を作る。nonce 再使用と空文字の拒否には focused test がある。paired client の削除は trusted client の保存レコードを除去する処理であり、既存 signing session の扱いまで同一操作と仮定しない。[refresh](../../crates/server/src/relay_pairing/server.rs#L235-L268)・[test](../../crates/trusted-key-auth/src/runtime.rs#L137-L170)・[削除](../../crates/trusted-key-auth/src/trusted_keys.rs#L77-L92)

## Relay の要求と応答

relay 要求は signing session、timestamp、nonce と、method/path/query/body に対応する署名を検証する。nonce の再送を拒否し、署名が有効になってから nonce を記録する。署名検証失敗は Unauthorized となる。[検証](../../crates/relay-control/src/signing.rs#L228-L261)・[middleware](../../crates/server/src/middleware/relay_request_signature.rs#L30-L68)

応答署名には status、path/query、元 request の nonce、新 response nonce、本文が含まれる。署名付き HTTP の request/response はともにメモリーへ読み込み、50 MiB を上限とする。大きい添付や streaming API の変更では、このバッファリング境界を確認する。[応答と容量上限](../../crates/server/src/middleware/relay_request_signature.rs#L25-L28)・[応答署名](../../crates/server/src/middleware/relay_request_signature.rs#L71-L116)

Origin guard は別の機構である。relay 要求を別認証へ委ね、Origin のない要求を許可し、Origin がある場合は same-origin または allowlist を確認する。Origin の一致は user 認証ではない。[Origin 検証](../../crates/server/src/middleware/origin.rs#L40-L81)

## Browser transport と失敗時の挙動

remote-web の local API 要求は host context を必要とし、host が特定できなければ失敗する。relay の認証失敗時には remote session cache を無効化し、signing session の refresh が成功した場合に一度再送する。[host 選択](../../packages/remote-web/src/shared/lib/relayHostApi.ts#L38-L85)・[再認証](../../packages/remote-web/src/shared/lib/relayHostApi.ts#L88-L132)

WebRTC HTTP transport は接続がない場合、未対応 body の場合、送信例外の場合に relay へ fallback する。WebSocket は接続がないときに relay を選び、接続があると data channel を返す。HTTP と WebSocket に同一の回復処理があると仮定しない。副作用を持つ API を追加する際は、HTTP transport の再送後も整合するかを個別に確認する。[HTTP fallback](../../packages/remote-web/src/shared/lib/webrtc/transport.ts#L40-L100)・[WebSocket の選択](../../packages/remote-web/src/shared/lib/webrtc/transport.ts#L103-L123)

## Self-hosting と直接接続

[Remote Service README](../../crates/remote/README.md) は開発用 Compose と relay / attachments profile の入口、[Deploy with Docker Compose](../../docs/self-hosting/deploy-docker.mdx) は独自ドメインで Cloud を配置する手順である。後者の local auth は単一の共有 credential pair を使う bootstrap 手段と明記されている。複数利用者の identity 管理と同一視しない。[記録された制約](../../docs/self-hosting/deploy-docker.mdx)

手元の local-web を私有 tailnet から開く場合の設計は [Local Tailscale Access](../../docs/ai-mobile/tailscale-local-access.md) にある。文書自体は draft だが、記載された .ts.net の Vite allowlist、Host を保持する /api proxy、WebSocket 転送は現行設定に存在する。tailnet の外部設定や通信成立まで repository の検査だけでは保証できない。[現在の proxy 設定](../../packages/local-web/vite.config.ts#L135-L150)

[AI Mobile Design](../../docs/ai-mobile/design.md) は v0.1 draft であり、既存の web/relay 経路を再利用してモバイル操作面を整える意図を記録する。QR pairing は明示的な提案、PWA の拡充や native packaging は計画・条件付き検討である。現行の pairing-code 実装を根拠に、それら全体が提供済みとは扱わない。[文書の状態と目的](../../docs/ai-mobile/design.md)・[QR 提案](../../docs/ai-mobile/design.md)
