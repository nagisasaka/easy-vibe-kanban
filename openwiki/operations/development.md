---
type: guide
title: 開発・検証・運用文書の使い分け
description: 起動形態、検証の適用範囲、型と SQLx の更新、DB 回復の制約、リリース文書の位置づけを整理する。
tags: [development, testing, database, releases]
sources:
  - id: openwiki-source-eaf96aacdc40d6db8dae4f8e
    resource: repo://deploy/server/acme.py
  - id: openwiki-source-df6b7819295f53fc2381f23d
    resource: repo://deploy/server/compose.yaml
  - id: openwiki-source-8fd7f4fe2d73e778cc63c256
    resource: repo://crates/local-deployment/src/process_host.rs
  - id: openwiki-source-42facea377bb55dbd5d67a80
    resource: repo://deploy/server/compose.test.mjs
  - id: openwiki-source-ddf0f7c06298de76526a4f7c
    resource: repo://packages/web-core/src/features/pipeline/model/cardContext.ts
  - id: openwiki-source-da8c4a13aea0bac33b3f0a35
    resource: repo://crates/services/src/services/openwiki.rs
  - id: openwiki-source-6067be8d6f85caa8ac03638f
    resource: repo://crates/services/src/services/repository_memory.rs
  - id: openwiki-source-d40a52c39b8604505b97c3e9
    resource: repo://crates/git/src/publication.rs
  - id: openwiki-source-19419edea1d312cc9f07ef2d
    resource: repo://crates/utils/src/path.rs
  - id: openwiki-source-7d5590729f3e0f42d994b0e8
    resource: repo://crates/workspace-manager/src/shared_resources.rs
  - id: openwiki-source-b50585f67dac89cd8eecc75d
    resource: repo://crates/executors/default_mcp.json
  - id: openwiki-source-a3956271cbf62ae0786e7fc7
    resource: repo://crates/executors/src/env.rs
  - id: openwiki-source-b28e4ff9d0efb53d7fe91df4
    resource: repo://.github/workflows/publish-server.yml
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-b41a0c4c19395b75cde58592
    resource: repo://CONTRIBUTORS.md
  - id: openwiki-source-5c2565a649f8be059d306209
    resource: repo://crates/db/src/lib.rs
  - id: openwiki-source-788fa2f6f1a3d449a1a9efe4
    resource: repo://crates/local-deployment/src/agent_run_port.rs
  - id: openwiki-source-1dab759dec78d0ac59fce9bf
    resource: repo://crates/remote/Cargo.toml
  - id: openwiki-source-b45d5e8f529e20ec3c79b7c3
    resource: repo://crates/remote/Dockerfile
  - id: openwiki-source-002506e7c7027ce202508759
    resource: repo://crates/remote/src/bin/generate_types.rs
  - id: openwiki-source-32657d5732db65a52d550bc6
    resource: repo://crates/server/src/bin/generate_types.rs
  - id: openwiki-source-88fc73cd214d9f8c3b051c4f
    resource: repo://crates/tauri-app/src/main.rs
  - id: openwiki-source-57eebdf400ca14f6e9abea48
    resource: repo://crates/tauri-app/tauri.conf.json
  - id: openwiki-source-8b0f6773cd6f7a2a989ad880
    resource: repo://deploy/server/nginx.test.mjs
  - id: openwiki-source-42e4658efb0c9810a0f8245c
    resource: repo://docs/design/openwiki-implementation-checkpoints.md
  - id: openwiki-source-16e2e0955d13393936bad36a
    resource: repo://docs/easy-npx-npm-publish.md
  - id: openwiki-source-2e1d91f21691cd271afffb6f
    resource: repo://docs/self-hosting/server-container.mdx
  - id: openwiki-source-e6cb0fba58fb726a75c5078f
    resource: repo://mobile-testing.md
  - id: openwiki-source-cbce6a0c74f278a7e8b4a543
    resource: repo://npx-cli/package.json
  - id: openwiki-source-4492126e225ccf5847191271
    resource: repo://npx-cli/README.md
  - id: openwiki-source-1bc1c02bea1e9f22f813abc8
    resource: repo://npx-cli/src/cli.ts
  - id: openwiki-source-26312f1dedcae510b462cb43
    resource: repo://npx-cli/src/download.ts
  - id: openwiki-source-5b54a58d1b51cd490b0e7162
    resource: repo://package.json
  - id: openwiki-source-96cd4c94fb57fb4764608e3d
    resource: repo://packages/web-core/package.json
  - id: openwiki-source-23775c3de52f3ab95a13cb8b
    resource: repo://README.md
  - id: openwiki-source-b7793decf9d7c9ba48e57e0f
    resource: repo://rust-toolchain.toml
  - id: openwiki-source-d90097af5777425ac57c25b3
    resource: repo://scripts/prepare-db.js
  - id: openwiki-source-30e56c8d7adebcb9dacd7f0c
    resource: repo://scripts/validate-npm-version.cjs
  - id: openwiki-source-7d85b73c82559c83c6efb7f9
    resource: repo://tests/workflow/fixture/src/main.tsx
  - id: openwiki-source-966297deb875824dc95ee8d5
    resource: repo://tests/workflow/playwright.config.ts
  - id: openwiki-source-1d08facbf90704cd4c4c54d8
    resource: repo://tests/workflow/specs/card-context.spec.ts
generated: { by: "codex", at: "2026-09-22T01:26:32.834Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-22T01:26:32.834Z
---

# 開発・検証・運用文書の使い分け

変更箇所から [システムの境界](../architecture/system.md) を選び、その境界の検証を行う。README は開発の入口だが、コマンドの実際の対象は package.json と各 Cargo manifest で確認する。

## 開発環境と起動形態

ルート package.json は Node >=20、pnpm >=8 を要求し、packageManager を pnpm@10.13.1 に固定する。Rust toolchain は nightly-2025-12-04。npm 配布用 npx-cli の Node >=18.18.0 という条件は、ソース開発全体の条件とは異なる。[root manifest](../../package.json#L74-L78)・[Rust toolchain](../../rust-toolchain.toml)・[配布 package](../../npx-cli/package.json#L19-L21)

| 作業 | 入口 | 重要な境界 |
| --- | --- | --- |
| ローカル Web と backend | pnpm run dev | frontend/backend/preview の port を解決し並行起動する |
| Backend の再起動開発 | pnpm run backend:dev | agent-process-host を build してから server を実行する |
| Tauri desktop の開発 | pnpm run tauri:dev | 外部 Vite / backend に接続する debug 起動。production の embedded backend 起動とは異なる |
| Cloud | pnpm run remote:dev | 別 Cargo workspace と Compose の PostgreSQL / Electric |
| Cloud + relay + 添付 | pnpm run remote:dev:full | relay / attachments の追加 profile |

[各 script](../../package.json#L17-L60)

Tauri の `beforeDevCommand` は frontend と backend watch を並行起動する。debug build の window はその外部 frontend に接続し、production build だけが `server::startup::start()` で backend を内蔵起動してから window を開く。したがって desktop の開発試験を、production の embedded startup・completion watcher 配線を検証した証拠にしない。[開発前処理](../../crates/tauri-app/tauri.conf.json#L6-L12)、[debug / production 分岐](../../crates/tauri-app/src/main.rs#L176-L239)

backend の watch 起動は DISABLE_WORKTREE_CLEANUP=1 を設定する。[Workspace の期限切れ cleanup](../concepts/workspace.md) の挙動を検証するときは、この開発時の差を考慮する。Agent runtime の変更では server だけでなく独立 process host の binary も変更対象になる。[watch 定義](../../package.json#L35-L38)

**remote:dev / remote:dev:full は up が終了した後に down -v を実行する。** ローカル検証用データの volume を保持したい運用で、この script を常駐サービスの起動と同一視しない。remote:dev:clean も volume 削除を含む。個別 Compose の実行は [Remote Service README](../../crates/remote/README.md)、公開配置は [Remote と self-hosting](../integrations/remote-access.md) を参照する。[終了処理](../../package.json#L53-L55)

## 検証の対象を取り違えない

| 検証 | 実際の対象・限界 |
| --- | --- |
| pnpm run check | local-web / remote-web / web-core / ui の型、legacy path guard、root の cargo check |
| pnpm run lint | local-web / ui の lint、root の Clippy（qa-mode、all-targets）、未使用 i18n key 検査 |
| cargo test --workspace | root workspace の Rust tests |
| pnpm run remote:check / remote:lint / remote:test | 独立した Remote Rust workspace |
| pnpm run workflow:e2e | 専用 Vite fixture に対する Chromium UI tests |
| web-core の test:wiki | Pipeline model と Wiki feature の Vitest。全 frontend tests の包括実行ではない |
| pnpm run server:check | server 配布 kit の Node tests。native nginx test の実行には nginx が必要 |

[コマンド定義](../../package.json#L13-L45)・[Wiki test script](../../packages/web-core/package.json#L100-L104)

root Cargo workspace は remote と relay-tunnel を明示的に exclude する。README の「all Rust workspaces」という check の説明は実際の範囲より広い。relay-tunnel 自体を変更する場合も、その manifest に対する検証が必要になる。[Cargo 境界](../../Cargo.toml#L1-L36)・[README の記載](../../README.md)

Workflow の Playwright は1 workerで専用 fixture を立ち上げる。fixture は製品の Workflow component/model を import するが、実 provider と DB を含む本番実行全体の test ではない。たとえば Card context test は折り畳み、read-only preview、保存 description との独立性を確認する。[test 設定](../../tests/workflow/playwright.config.ts#L6-L43)・[fixture](../../tests/workflow/fixture/src/main.tsx#L1-L24)・[Card context test](../../tests/workflow/specs/card-context.spec.ts#L7-L42)

実行制御の変更は [監査・投影の回復 tests](../architecture/agent-runtime.md)、Graph の変更は [Workflow の validation/planner tests](../concepts/workflow-attempt.md)、memory checkpoint は [Repository memory の冪等性 tests](../concepts/repository-memory.md) へ進む。画面上の成功と永続状態の成功を分けて検証する。

### Remote を直接検証するときの private 依存

Remote の manifest は `default = []` でも、SSH Git URL の private `billing` dependency を optional として保持し、`vk-billing` feature から参照する。**feature を使わないことと Cargo が private source を解決しなくてよいことは別**であり、直接の remote:check / remote:lint / remote:test は依存解決段階で止まる場合がある。[manifest](../../crates/remote/Cargo.toml#L11-L17)、[直接実行の script](../../package.json#L41-L43)

この条件を記録した [implementation checkpoints の private dependency 節](../../docs/design/openwiki-implementation-checkpoints.md#private-remote-dependency-and-aggregate-quality-gates) は、当時の失敗がコンパイル前のアクセス失敗であったこと、SSH と許可された repository access または該当 locked revision を持つ Cargo cache が検証の前提であることを明記する。この文書は仕様の代替ではなく実装・検証記録である。現在の環境で同じ失敗を再現した記録、あるいは未実行 tests の合格証拠として転用しない。

公開 self-host 用 Docker build は別経路である。`FEATURES` が空のとき、**build context 内**で private billing 行・feature の依存参照・Remote Cargo.lock を除いてから build する。非空ならその除去をせず `--locked` と features を渡す。[Docker の明示分岐](../../crates/remote/Dockerfile#L88-L103)。この Docker build の条件を、元 checkout の直接 Cargo 検証へそのまま適用できない。検証目的で source の依存宣言を stub に置き換えた結果も、元構成の合格とは扱わない。

なお過去の checkpoints は root の check/lint も Remote へ進んだと記録するが、現在の [root script](../../package.json#L13-L15) は上表の範囲であり、Remote Rust は独立 script に分かれている。記録された当時の制約と、現在のコマンド範囲を分けて読む。

## Docker 配布と同梱 binary

LVK の配布先は GHCR の Docker image のみ。`.github/workflows/publish-server.yml`
が root Dockerfile の `server` target を build・検証し、version tag で公開する。
手動 dispatch は公開せず検証する。`server` と `agent-process-host`、
`vibe-kanban-mcp` を同じ image に同梱する。既定 MCP 設定は同梱バイナリを直接起動し、
npm から別バージョンを取得しない。

`npx-cli/` と旧 version validator は upstream 由来の保管コードであり、
現在の LVK の公開手順ではない。旧 NPX 文書も廃止案内へ置き換えた。
配布 image と Compose の更新・旧名 volume の引継ぎは
[サーバーコンテナー](server-container.md)を参照する。

## LVK の名称

製品表記・AI 指示・設定・CI fixture は LVK に統一する。
移行元の旧環境・データはないというユーザーの確認に基づき、名称変更用の
別名対応・二重ロック・旧パスのリンク・Docker 移行 overlay は設けない。
オリジナル Vibe Kanban の `VK_*`、native binary、保存先、外部サービスの識別子は
変更対象ではない。上流への帰属表記と実際の過去の記録は保持する。

| 境界 | 現在の契約 | 根拠 |
| --- | --- | --- |
| 実行設定 | `LVK_*` を読み書きし、process host へそのまま渡す。旧接頭辞への変換はしない。 | [ExecutionEnv](../../crates/executors/src/env.rs)、[HostExecutionEnv](../../crates/local-deployment/src/process_host.rs) |
| Card Context | `lvk:` marker を読み書きする。説明の編集では保存済み指示と明示的 opt-out を維持する。 | [Card Context](../../packages/web-core/src/features/pipeline/model/cardContext.ts) |
| 共有ファイル | `.lvk-shared` から repository storage を参照する。追跡済み mount・別 repository 向きのリンク・ユーザーファイルを上書きしない。watcher、Git diff、Docker context から除外する。 | [共有リンク](../../crates/workspace-manager/src/shared_resources.rs)、[除外](../../crates/utils/src/path.rs) |
| Git publication | `LVK-Memory-Source` / `LVK-Memory-Integration` / `LVK-Wiki-Publication` trailer を記録し、復旧時も同じ marker を検索して重複 commit を防ぐ。 | [Memory 復旧](../../crates/services/src/services/repository_memory.rs)、[Wiki 公開](../../crates/services/src/services/openwiki.rs) |
| publication lock | repository 共通の `lvk-publication.lock` 一つを保持する。この lock は外部の任意 Git 操作までは制御しない。 | [GitPublicationGuard](../../crates/git/src/publication.rs) |
| サーバー | Compose project/service は `lvk-server` / `lvk`。接続先は IP・DNS 共通で `LVK_HOST`、Certbot lineage は `lvk`。 | [Compose](../../deploy/server/compose.yaml)、[ACME](../../deploy/server/acme.py) |
| MCP・配布 | 既定 MCP は image に同梱した `vibe-kanban-mcp` を直接実行する。LVK の npm package を取得する preset は提供しない。 | [既定 MCP](../../crates/executors/default_mcp.json)、[接続手順](../../docs/integrations/vibe-kanban-mcp-server.mdx) |

### 名称統一の検証記録（2026-10-06）

ユーザー依頼に基づくローカル作業差分の検証。基点は `0f7f7fc3`。
旧名の互換処理を削除した未コミット差分に対する結果であり、過去の release image の結果ではない。

- `cargo test --workspace --locked -j 3`: 994 passed、0 failed、8 ignored。
- `pnpm run server:check`: 22 passed、0 skipped。`LVK_TEST_NGINX` と
  `LVK_TEST_COMPOSE` に実行ファイルを指定し、nginx の HTTPS/Basic 認証・WS/SSE と
  Compose の LVK 設定・volume を実際に検査した。
- `pnpm --filter @vibe/web-core run test:wiki`: 32 passed。
- `pnpm run check`、`pnpm run lint`、`pnpm run format` は成功。
- 旧名用の互換テストは削除した。実行コード・設定・テストに旧製品名の文字列は残っていない。
  上流への帰属・過去の記録、および lockfile のハッシュ内の偶然の一致は対象外。

この検証では Docker release image の build・公開・稼働サーバーの更新は行っていない。
独立した private remote backend の Cargo 検証も実行していない。
既存 claim evidence の version と生成時刻は以前の検証を示すため、名称の手動更新を
OpenWiki の新たなモデル検証・再生成成功として扱わない。

## サーバー配布 kit の検証

`pnpm run server:check` は `node --test deploy/server/*.test.mjs` を実行する。設定・証明書・process supervision 等の検査と、nginx を実際に起動する TLS/Basic 認証・WebSocket・SSE・preview 境界の検査を含む。nginx がなければ後者は skip するため、全 subtest が実行されたかを結果で確認する。`LVK_TEST_NGINX` で実行ファイルを指定でき、`LVK_REQUIRE_NGINX_TEST=1` は欠落を失敗にする。[script](../../package.json#L22)、[native test の前提](../../deploy/server/nginx.test.mjs)

この確認だけで Docker image の build・実配置・live agent 認証が成功したことにはならない。配布済み binary と作業 checkout の分離、永続 volume、停止・更新・復旧の契約は [サーバーコンテナー](server-container.md)に置く。[記録された検証限界](../../docs/self-hosting/server-container.mdx#validation-boundaries)

## 版番号の検証と選別移植の追跡

旧 npm 用の version parser は過去の `easy` 入力も互換用に受け付ける。
そのテストは現在の Docker image の build・公開成功を証明しない。
現在の配布検証は `.github/workflows/publish-server.yml` と
`deploy/server/release.test.mjs` を参照する。

上流変更の採否・LVK 向け適応・故障注入・実 Codex を使った MCP 受入は [選別移植の実装記録](../../docs/design/upstream-selective-backport-implementation.md)にある。各試験の source revision と実行対象を識別し、過去の成功を別 revision の成功と取り違えない。特に server と `agent-process-host` を同じソースから更新し、旧 Host の互換接続と新 Host の journal 回復を分けて試す。[Host の回復契約](../architecture/agent-runtime.md#接続断と確認済み終了を分ける)

Workflow fixture には draft / Undo、設定安全性、route loading、Runtime input、diff tree の harness がある。これは製品 component のブラウザー試験であり、認証済み provider・publication を含む実機試験とは別である。[fixture の入口](../../tests/workflow/fixture/src/main.tsx#L1-L29)

## 型・schema・SQLx を変更する場合

local API の生成元は Rust の generate_types binary であり、shared/types.ts と shared/schemas を生成・比較する。Remote には shared/remote-types.ts 用の独立した generator がある。API 変更は生成先だけを編集せず、Rust 側を更新して各 generator と --check を使う。[local generator](../../crates/server/src/bin/generate_types.rs#L673-L705)・[remote generator](../../crates/remote/src/bin/generate_types.rs#L30-L56)

prepare-db は一時 SQLite を作り、migration を適用してから cargo sqlx prepare を実行し、finally で一時 DB を削除する。--check でも一時 DB の作成と migration は行うため、単なる読み取り専用の型検査ではない。Remote には別の prepare-db / prepare-db:check script が用意される。[SQLite 準備](../../scripts/prepare-db.js#L7-L47)・[Remote の入口](../../package.json#L56-L57)

本番 DB の migration checksum 不一致は、debug build と非 Windows ではエラーになる。Windows release は記録済み checksum を更新して再試行する分岐を持ち、その根拠として改行等の platform 差を code comment が挙げる。これは任意の schema 不一致を修復する保証ではない。起動時の schema guard は [システムの永続化境界](../architecture/system.md) を参照する。[platform 分岐と記録された理由](../../crates/db/src/lib.rs#L29-L73)

## 文書の適用範囲

- [CONTRIBUTORS](../../CONTRIBUTORS.md) は maintainer review、変更管理、生成ファイルを直接編集しないという方針を記録する。実装の挙動の証拠とは分けて読む。[変更管理](../../CONTRIBUTORS.md)・[生成物の方針](../../CONTRIBUTORS.md)
- [LVK release distribution](../../docs/easy-npx-npm-publish.md) は旧 npm 手順の廃止案内である。現行の Docker 公開・検証は [サーバー配布手順](../../docs/self-hosting/server-container.mdx)を参照する。
- [Mobile testing](../../mobile-testing.md) は remote-web をスマートフォンから試すための Tailscale/Caddy 手順。手元の local-web への直接接続とは対象が異なる。[対象](../../mobile-testing.md)・[直接接続の説明](../integrations/remote-access.md#self-hosting-と直接接続)
- [OpenWiki maintenance](openwiki-maintenance.md) は、通常開発の tests や release と別の生成・レビュー・公開 checkpoint を説明する。

この Wiki は実装理解のための派生知識であり、試験成績そのものではない。初期生成時の静的調査と、その後の選別移植で行った自動検証・MCP受入を区別する。再検証するときは上記の実装記録と現行コマンドを参照し、未実行や既存 skip を合格に数えない。
