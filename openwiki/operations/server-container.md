---
type: guide
title: 単一サーバーコンテナーの運用境界
description: local LVK をサーバー上で動かす配布構成。HTTPS、認証、preview origin、永続化、停止と回復の契約。
tags: [server, docker, deployment, preview, persistence]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-21T08:16:36.701Z
sources:
  - id: openwiki-source-df6b7819295f53fc2381f23d
    resource: repo://deploy/server/compose.yaml
  - id: openwiki-source-eb00c8fb4075578ba5a7c691
    resource: repo://deploy/server/healthcheck.mjs
  - id: openwiki-source-078e9842bd26cfd05bb35acc
    resource: repo://deploy/server/nginx.conf.template
  - id: openwiki-source-8b0f6773cd6f7a2a989ad880
    resource: repo://deploy/server/nginx.test.mjs
  - id: openwiki-source-89d0fb22262f0a0ac1df7d1e
    resource: repo://deploy/server/server.mjs
  - id: openwiki-source-229e22cb29e176ae1fe594e1
    resource: repo://deploy/server/server.test.mjs
  - id: openwiki-source-bb1ebe868e35e9e500714501
    resource: repo://Dockerfile
  - id: openwiki-source-2e1d91f21691cd271afffb6f
    resource: repo://docs/self-hosting/server-container.mdx
  - id: openwiki-source-e4b6c8db09a6d8887192b13e
    resource: repo://packages/web-core/src/shared/lib/previewProxyUrl.ts
  - id: openwiki-source-a9fbf89a6c5f546ccbb68e11
    resource: repo://deploy/browser/package.json
  - id: openwiki-source-71033560192b23e2aaead6d8
    resource: repo://deploy/developer/updater.py
  - id: openwiki-source-af6429108aa5f0af03e4cea9
    resource: repo://deploy/developer/client.py
  - id: openwiki-source-cd7d4081ac64d24918e97292
    resource: repo://deploy/developer/README.md
  - id: openwiki-source-5eead7e39aee9506055e7dab
    resource: repo://tests/server/playwright.config.ts
  - id: openwiki-source-1533c9d33bc09a99bdf46a2a
    resource: repo://crates/server/src/routes/sessions/queue.rs
generated: { by: "codex", at: "2026-09-21T08:16:36.701Z" }
---

# 単一サーバーコンテナーの運用境界

server 配布は local LVK と coding agent を常時稼働サーバーへ置き、利用者のブラウザーから操作するための構成である。[Cloud / Remote](../integrations/remote-access.md)の PostgreSQL・Electric・relay を必要とする構成とは別で、同じ local の DB・Workspace・AgentRun を使う。ブラウザーを閉じてもサーバーの実行は続くが、コンテナー停止やホスト再起動はプロセスを止める。[製品の用途](../../docs/self-hosting/server-container.mdx#L6-L16)

単一の信頼された利用者の開発環境であり、Basic 認証が tenant ごとの隔離を作るわけではない。LVK・nginx・agent は同じ非 root user `appuser`（UID/GID 10001）で動く。read-only secret mount は上書きを防ぐだけで、同じユーザーの agent からの読取を防がない。[信頼境界](../../docs/self-hosting/server-container.mdx#L10-L27)、[runtime user](../../Dockerfile#L103-L115)

## 配布 binary と開発 checkout

root Dockerfile の最終 `server` target は prebuilt server・隣接 `agent-process-host`・MCP CLI と開発 toolchain を含む。稼働 binary は `/usr/local/bin` に置かれ、`/repos` の LVK checkout を編集・build しても、それを管理しているアプリを置換しない。小さい `runtime` target は別の起動入口を持ち、server target の認証付き ingress と同じではない。[target と起動入口](../../Dockerfile#L108-L171)

Node 22、固定版 Codex/pnpm、repository の Rust toolchain などを image に置き、user の認証・cache は永続領域へ分ける。追加 SDK は派生 image の `/opt` や `/usr/local` に置くと、既存 home volume に隠されにくい。公開版・digest と対応する配布 kit を使い、文書の例の tag を公開済み成果物と推定しない。[image の構成](../../Dockerfile#L130-L170)、[派生 image の契約](../../docs/self-hosting/server-container.mdx#L185-L210)

## HTTPS と起動前検証

Compose は host の `443` だけを nginx の `8443` へ公開する。Rust backend は `127.0.0.1:3000`、preview proxy は `3001` を使い、supervisor は継承された HOST/PORT を上書きする。許可 origin は設定したアプリの HTTPS origin である。[port と mount](../../deploy/server/compose.yaml#L1-L21)、[backend 環境](../../deploy/server/server.mjs#L87-L98)

`LVK_HOST` は IP アドレスまたは scheme/port/path を含まない小文字 DNS 名を受け付ける。任意の `LVK_PREVIEW_DOMAIN` を使う場合、アプリ名を preview wildcard と同じかその配下に置けない。起動前に TLS 証明書の期間、アプリ名と preview の代表 host 名への対応、秘密鍵との一致、非空 bcrypt htpasswd を検証する。volume 書込権と `nginx -t` も通ってからサービスを開始する。[設定](../../deploy/server/server.mjs#L7-L85)、[起動順](../../deploy/server/server.mjs#L182-L215)

TLS 発行・更新はコンテナー外の責任であり、起動検査はブラウザーによる trust chain 検証の代わりではない。証明書・鍵・htpasswd は `/run/lvk-secrets` へ read-only mount し、欠けた host path を Compose が自動作成しない。更新・認証情報変更の具体手順は [運用文書](../../docs/self-hosting/server-container.mdx#renew-certificates-and-rotate-credentials)に置く。[mount](../../deploy/server/compose.yaml#L15-L20)、[証明書の責任](../../docs/self-hosting/server-container.mdx#L50-L81)

nginx はアプリ、API、WebSocket、stream、preview に Basic 認証を適用する。転送時は Authorization / Proxy-Authorization と relay 署名ヘッダーを消すため、開発アプリが独自の同じ認証ヘッダーを要求する場合はこの入口と両立しない。WebSocket upgrade を通し、proxy buffering/cache は無効にする。access log は query string・認証情報・agent 本文を含めず method/URI/status を記録する。[ingress 契約](../../deploy/server/nginx.conf.template#L10-L48)

## Preview は別 origin

開発サーバー `5173` の公開 URL は `https://5173.<preview-domain>` になる。許す port は 1024–65535 から LVK/nginx の 3000・3001・8443 を除いたもの。main origin の `/api/preview` と `/api/host/<id>/preview` は 403 にし、project-controlled HTML を LVK の origin へ持ち込まない。[nginx の分離](../../deploy/server/nginx.conf.template#L50-L65)、[preview host の生成](../../deploy/server/server.mjs#L33-L46)

UI も公開 preview domain がある場合は HTTPS wildcard URL を使い、relay host を組み合わせない。preview 無効なら wildcard server は作らない。iframe 内で認証 prompt が出ない場合は新しいタブで当該 preview に認証してから戻る運用となる。HTML/JavaScript に埋め込まれた絶対 localhost URL を全て書き換える保証はない。[URL 構築](../../packages/web-core/src/shared/lib/previewProxyUrl.ts)、[preview の運用制限](../../docs/self-hosting/server-container.mdx#L117-L126)。ローカル表示との関係は [Workspace preview](workspace-inspection.md#preview-と開発サーバー)を参照する。

## 永続化と実行の寿命

| named volume | mount | 維持するもの |
| --- | --- | --- |
| home | `/home/appuser` | SQLite・設定・認証・agent Session・user cache |
| repos | `/repos` | 元の Git repository と未追跡ファイル |
| work | `/var/tmp` | worktree・共有資源・run 関連ファイル |

Compose project 名の変更は別 volume の選択になる。同じ named volume の再利用はデータを維持するが、実行中プロセスは復元しない。custom worktree root を mount 外へ置けばこの永続化の対象外になる。[保存先](../../deploy/server/compose.yaml#L1-L30)、[永続データの契約](../../docs/self-hosting/server-container.mdx#L128-L140)

`.lvk-shared` の意味は [Workspace の共有領域](../concepts/workspace.md#共有ディレクトリの意味)と同じで、Cargo target 等を自動設定するものではない。停止した Goal や保守 run は保存状態と所有者の規則に従って確認する。コンテナー再起動だけで任意の処理を自動継続する保証はない。[再起動後の確認](../../docs/self-hosting/server-container.mdx#L174-L183)

supervisor は server と nginx を process group として起動する。一方の終了・spawn 失敗ではもう一方へ SIGTERM を送り、既定 120 秒後に残存 group へ SIGKILL を送る。外部 SIGTERM/SIGINT も同じ停止手順を使い、Compose の停止猶予は150秒である。これは個々の AgentRun の [監査付き取消](../architecture/agent-runtime.md#取消と実プロセスの終了)とは別のコンテナー停止境界である。[supervision](../../deploy/server/server.mjs#L101-L179)、[猶予](../../deploy/server/compose.yaml#L21)

healthcheck は backend `/health` の200と、認証なし HTTPS の401を内部で確認する。後者の TLS trust 検証は無効であり、外部 DNS・証明書信頼・agent 認証の検査ではない。運用文書は unhealthy のみでは再起動しないことを明示する。サービス終了時の restart policy と health status を分けて診断する。[probe](../../deploy/server/healthcheck.mjs#L19-L35)、[再起動契約](../../docs/self-hosting/server-container.mdx#L83-L92)

## 更新・回復と検証限界

### サーバー内ブラウザー E2E と開発者向け更新サービス

標準 server image は `/opt/lvk-browsers` に固定版 Playwright の Chromium と
OS 依存ライブラリーを含む。home volume に隠されない場所に配置し、Compose の
共有メモリーは256MBとする。`pnpm run server:e2e` は指定した実アプリのURLへ接続して
画面描画・reload・実API・匿名HTTPS拒否を確認する。接続先は `LVK_E2E_BASE_URL`、
Basic認証は `LVK_E2E_CREDENTIALS_FILE` のJSONから読み、TLS検証は無効化しない。
既存 `workflow:e2e` は4175のUI fixtureであり、8443の実アプリE2Eとは区別する。
[ブラウザー構成](../../deploy/browser/package.json)、[実アプリE2E](../../tests/server/playwright.config.ts)

任意導入の `deploy/developer` は Linux/systemd ホスト向けで、標準imageには含めない。
ホストへ設置したroot所有のコードがDockerを操作する。agentには要求ディレクトリーへの
書込みと状態の読取りだけを提供し、Docker socket・rootコード・ホスト用GitHub tokenを
渡さない。公開forkは `client.py release --repository OWNER/REPO --tag VERSION` で
CI結果とdigestを取得し更新を要求できる。要求元agentはそのrunを終了する必要がある。
[導入と運用手順](../../deploy/developer/README.md)、[非特権client](../../deploy/developer/client.py)

ホストは許可repository・source SHA・digest・CI run label・公開workflow成功を照合し、
使い捨てvolumeで候補imageのブラウザー/バックエンドを検査する。通常の編集はsource reload、
採用する変更のリリース時だけCI image buildを行う。8443は単一checkoutを参照し、カードの
worktreeを自動選択しない。共有previewの排他利用か別portの環境を明示的に用意する。

更新は両アプリのDBと実プロセスがidleになるまで待つ。Goal active、未完了AgentRun/
attempt、共有資源所有、未完了workflow、未知process、有効な定期実行は更新を妨げる。
HTTPのreadiness barrierで既存handlerの終了を待ち、新しい変更リクエスト・WebSocket
upgradeを503で拒否する。[HTTP barrier](../../crates/server/src/middleware/maintenance.rs)
その後、停止で失われるin-memory follow-upを共通admission mutex付きのqueue GETで
検査する。旧版にこの能力がなければ、まずSSHから対応版へbootstrapする。
debug版DBはcheckoutの`dev_assets`、release版DBは永続homeにあり、両者を検査する。
idle検査から停止までの競合を閉じるため、両コンテナーをfreezeして再検査し、仕事が
増えていれば再開して待つ。idleを確認できた場合だけfreezeしたまま終了する。
これはgraceful shutdownではなくSQLite WAL回復を使う停止であり、active agentを
強制終了する手段にはしない。停止後、全永続volumeと設定を保存し、再生成可能な
development Cargo targetだけを除外する。[状態機械と検査](../../deploy/developer/updater.py)

新imageの起動・ブラウザー検査中は任意のmaintenance mountによってHTTPSを503にする。
検査失敗時はchecksum検証済みのimage/データの組へ戻す。ホスト処理自体が中断された場合は
`recovery_required` を永続記録して後続更新を止め、SSHから確認・回復する。再起動は
実行中processの自動復元を意味しない。更新サービス自身のコード変更は別途ホストで反映する。
[競合・WAL・失敗時のテスト](../../deploy/developer/test_updater.py)

Codex のログイン済み表示と container の healthy は、カードからの実行成功を保証しない。
配置後は使い捨てのカードから通常の Code 実行を開始し、応答まで確認する。
特に `invalid transport in mcp_servers.openwiki` は認証エラーではなく設定エラーである。
Codex は無効な MCP サーバーにも transport を要求するため、通常 agent が OpenWiki を
無効化するときは effective config に存在する場合だけ override する。未登録のサーバーへ
`enabled = false` だけを追加してはならない。既存の stdio / HTTP transport は保持し、
保守 agent の明示的な MCP 起動設定とは区別する。
[設定の生成と回帰テスト](../../crates/executors/src/executors/codex.rs)がこの境界を扱う。

canonical 運用手順は agent を停止・完了させ、LVK を止めて三つの volume と設定・secrets を整合した組としてバックアップする。rollback は古い image だけを新 DB に向けず、対応する全 volume のバックアップを同じ内部パスの別 recovery deployment へ戻して検証する。schema migration と Git worktree metadata を分離して復旧しない。[バックアップと rollback](../../docs/self-hosting/server-container.mdx#L153-L183)

回帰検証は設定・証明書・停止を Node tests、TLS/認証/preview 分離を native nginx fixture、公開 URL を Rust/TypeScript tests で扱う。これらは実サーバー配置成功や公開 registry の存在を証明しない。native nginx が無い場合の skip と必須化は [開発時の検証](development.md#サーバー配布-kit-の検証)に置く。[認証・分離の test](../../deploy/server/nginx.test.mjs#L189-L235)、[停止の test](../../deploy/server/server.test.mjs#L197-L224)、[記録された未検証範囲](../../docs/self-hosting/server-container.mdx#L212-L231)
