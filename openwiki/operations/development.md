---
type: guide
title: 開発・検証・運用文書の使い分け
description: 起動形態、検証の適用範囲、型と SQLx の更新、DB 回復の制約、リリース文書の位置づけを整理する。
tags: [development, testing, database, releases]
sources:
  - id: openwiki-source-9c10153f85a40a7543c8914e
    resource: repo://.github/workflows/pre-release.yml
  - id: openwiki-source-0d0b05d2fac028aecd3be162
    resource: repo://.github/workflows/publish-easy-npx.yml
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

## NPX 配布経路と同梱 binary

NPX wrapper の platform 名を解決できることと、その配布物に binary が存在することは別である。現在の追跡済み release 設定には次の二経路がある。

| 経路 | この checkout にある build / package の契約 |
| --- | --- |
| `publish-easy-npx.yml` | Linux x64（musl）と Windows x64 を build し、`npx-cli/dist/{platform}/*.zip` を npm package に同梱する |
| `pre-release.yml` | Linux・Windows・macOS の x64/arm64 を対象にし、version ごとの manifest と binary を R2 に upload。NPX の bundled JS に R2 URL と binary tag を埋め込む |

[Easy build/package](../../.github/workflows/publish-easy-npx.yml#L145-L227)、[npm に含める files](../../npx-cli/package.json#L32-L35)、[pre-release matrix](../../.github/workflows/pre-release.yml#L213-L237)、[R2 manifest](../../.github/workflows/pre-release.yml#L655-L695)、[URL/tag の注入](../../.github/workflows/pre-release.yml#L1168-L1175)

`download.ts` は dist directory の存在、または `VIBE_KANBAN_LOCAL=1` で local/bundled mode に入る。このとき対象 zip がなければエラーとなり、**R2 へ fallback しない**。そのため CLI が macOS/ARM64 を認識していても、上記 Easy 同梱 package でそれらを提供する証明にはならない。dist がない R2 経路では tag/platform 別 cache と manifest を使い、取得した zip の checksum を検査する。[mode 選択](../../npx-cli/src/download.ts#L7-L17)、[取得分岐](../../npx-cli/src/download.ts#L155-L197)、[platform の解決](../../npx-cli/src/cli.ts#L66-L94)、[取得時検証](../../npx-cli/src/download.ts#L126-L143)

アプリ用 `vibe-kanban.zip` は server に加えて **`agent-process-host`（Windows は .exe）を同梱**する。wrapper は archive 全体を同じ directory に展開し、server の起動解決は環境指定、build 時指定、実行ファイルの隣などから process host を探す。server 単体の差し替えでこの companion を欠くと AgentRun の起動は Unavailable になり得る。[同梱と検査](../../.github/workflows/publish-easy-npx.yml#L189-L212)、[展開](../../npx-cli/src/cli.ts#L126-L180)、[host 解決](../../crates/local-deployment/src/agent_run_port.rs#L3122-L3163)。これは [Agent Runtime の独立 process host](../architecture/agent-runtime.md) が配布へ課す依存である。

廃止表示のない [Easy NPX 発行文書](../../docs/easy-npx-npm-publish.md) は Trusted Publishing と dry-run の手順を記録するが、build 対象の記述は Windows x64 のみで、現在の Linux x64 追加を反映していない。手順の意図を残し、対象 platform は実際の workflow と package contents を優先する。ここでは npm registry、公開済み archive、R2 の稼働状態を検査していないため、現在配信されている版の対応を断定しない。

廃止表示のない [NPX README の Supported Platforms](../../npx-cli/README.md#supported-platforms) は Linux/Windows x64 と macOS x64/arm64 を列挙するが、配布経路を分けていない。この一覧を Easy 同梱 package の対応表として使うと macOS を過大に解釈する一方、pre-release の Linux/Windows arm64 は一覧に含まれない。導入判断では README の名前だけでなく、上表のどの経路で作られた成果物かを確認する。

## サーバー配布 kit の検証

`pnpm run server:check` は `node --test deploy/server/*.test.mjs` を実行する。設定・証明書・process supervision 等の検査と、nginx を実際に起動する TLS/Basic 認証・WebSocket・SSE・preview 境界の検査を含む。nginx がなければ後者は skip するため、全 subtest が実行されたかを結果で確認する。`EVK_TEST_NGINX` で実行ファイルを指定でき、`EVK_REQUIRE_NGINX_TEST=1` は欠落を失敗にする。[script](../../package.json#L22)、[native test の前提](../../deploy/server/nginx.test.mjs)

この確認だけで Docker image の build・実配置・live agent 認証が成功したことにはならない。配布済み binary と作業 checkout の分離、永続 volume、停止・更新・復旧の契約は [サーバーコンテナー](server-container.md)に置く。[記録された検証限界](../../docs/self-hosting/server-container.mdx#validation-boundaries)

## 版番号の検証と選別移植の追跡

Easy NPX の版番号は stable `0.1.44`、`0.1.44-beta.1`、`0.1.44-easy.1` の形式を許し、空白・数値の先頭ゼロ・別 prerelease 形式を拒否する。実際の発行 workflow が共通 validator を呼ぶため、単なる UI の入力補助ではない。validator test は公開済み package の動作や発行成功を証明しない。[validator](../../scripts/validate-npm-version.cjs)、[CI の呼出し](../../.github/workflows/publish-easy-npx.yml#L57-L64)

上流変更の採否・LVK 向け適応・故障注入・実 Codex を使った MCP 受入は [選別移植の実装記録](../../docs/design/upstream-selective-backport-implementation.md)にある。各試験の source revision と実行対象を識別し、過去の成功を別 revision の成功と取り違えない。特に server と `agent-process-host` を同じソースから更新し、旧 Host の互換接続と新 Host の journal 回復を分けて試す。[Host の回復契約](../architecture/agent-runtime.md#接続断と確認済み終了を分ける)

Workflow fixture には draft / Undo、設定安全性、route loading、Runtime input、diff tree の harness がある。これは製品 component のブラウザー試験であり、認証済み provider・publication を含む実機試験とは別である。[fixture の入口](../../tests/workflow/fixture/src/main.tsx#L1-L29)

## 型・schema・SQLx を変更する場合

local API の生成元は Rust の generate_types binary であり、shared/types.ts と shared/schemas を生成・比較する。Remote には shared/remote-types.ts 用の独立した generator がある。API 変更は生成先だけを編集せず、Rust 側を更新して各 generator と --check を使う。[local generator](../../crates/server/src/bin/generate_types.rs#L673-L705)・[remote generator](../../crates/remote/src/bin/generate_types.rs#L30-L56)

prepare-db は一時 SQLite を作り、migration を適用してから cargo sqlx prepare を実行し、finally で一時 DB を削除する。--check でも一時 DB の作成と migration は行うため、単なる読み取り専用の型検査ではない。Remote には別の prepare-db / prepare-db:check script が用意される。[SQLite 準備](../../scripts/prepare-db.js#L7-L47)・[Remote の入口](../../package.json#L56-L57)

本番 DB の migration checksum 不一致は、debug build と非 Windows ではエラーになる。Windows release は記録済み checksum を更新して再試行する分岐を持ち、その根拠として改行等の platform 差を code comment が挙げる。これは任意の schema 不一致を修復する保証ではない。起動時の schema guard は [システムの永続化境界](../architecture/system.md) を参照する。[platform 分岐と記録された理由](../../crates/db/src/lib.rs#L29-L73)

## 文書の適用範囲

- [CONTRIBUTORS](../../CONTRIBUTORS.md) は maintainer review、変更管理、生成ファイルを直接編集しないという方針を記録する。実装の挙動の証拠とは分けて読む。[変更管理](../../CONTRIBUTORS.md)・[生成物の方針](../../CONTRIBUTORS.md)
- [Easy NPX npm 発行](../../docs/easy-npx-npm-publish.md) は Trusted Publishing、dry-run、版番号運用の記録された手順である。ここでは registry の公開状況や外部 publisher 設定を検証していないため、文書中の実例が現在の外部設定と一致するとは限らない。[文書の目的](../../docs/easy-npx-npm-publish.md)・[dry-run](../../docs/easy-npx-npm-publish.md)
- [Mobile testing](../../mobile-testing.md) は remote-web をスマートフォンから試すための Tailscale/Caddy 手順。手元の local-web への直接接続とは対象が異なる。[対象](../../mobile-testing.md)・[直接接続の説明](../integrations/remote-access.md#self-hosting-と直接接続)
- [OpenWiki maintenance](openwiki-maintenance.md) は、通常開発の tests や release と別の生成・レビュー・公開 checkpoint を説明する。

この Wiki は実装理解のための派生知識であり、試験成績そのものではない。初期生成時の静的調査と、その後の選別移植で行った自動検証・MCP受入を区別する。再検証するときは上記の実装記録と現行コマンドを参照し、未実行や既存 skip を合格に数えない。
