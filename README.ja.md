# Lucky Vibe Kanban（LVK）

**カンバンで作業を計画し、コーディングエージェントを実行して、変更をレビューする。**

Lucky Vibe Kanban は、Vibe Kanban と Easy Vibe Kanban を基にした、セルフホスト型の開発環境です。
手元のコンピューターで動かすことも、自分のサーバーに設置して、ノート PC を切断している間もエージェントの作業を続けることもできます。

[English](README.md) · [サーバーのセットアップ](docs/self-hosting/server-container.mdx) ·
[ソースコード](https://github.com/nagisasaka/lucky-vibe-kanban) · [Apache-2.0](LICENSE)

## できること

- 課題を整理し、Git worktree で Codex や Claude Code などのコーディングエージェントを実行する。
- 複数のセッションやワークスペースを管理し、実行履歴や差分を確認する。
- 並列分岐や条件分岐を含む、エージェントの作業手順を視覚的なワークフローとして組み立てる。
- ローカルボードで完了した作業を、専用の検証ワークスペースを通じて統合する。
- OpenWiki でリポジトリの知識を蓄積し、ソース統合後に更新する。
- 常時稼働するサーバー上でアプリとエージェントを動かし、HTTPS と Basic 認証でアクセスする。

エージェントのアカウントやプロバイダーの契約は、別途設定します。
LVK にモデルの利用料金は含まれておらず、誰でも無制限にエージェントを使える公開サービスでもありません。

## 配布方法

LVK は GHCR を通じて **Docker イメージのみ**を配布します。現在の対象は Linux amd64 です。
このフォークでは npm パッケージを公開しません。ローカルで開発する場合は、後述の手順でソースから起動してください。

## 自分のサーバーで動かす

[個人用サーバーの導入ガイド](docs/self-hosting/server-container.mdx)と、
[`deploy/server`](deploy/server) 内のファイルを使用します。
公開されている Linux amd64 イメージには、LVK、nginx、Codex CLI、Git、Node.js、pnpm、Rust の開発ツールが含まれています。

- 固定のパブリック IP アドレスがあれば利用でき、**ドメインの購入は不要**です。
- Let's Encrypt の証明書を自動で取得・更新します。
- HTTPS と Basic 認証でアプリへのアクセスを保護します。
- Docker ボリュームにリポジトリ、アプリのデータ、エージェントの認証情報を保存します。
- サーバーやコンテナを再起動すると、実行中のプロセスは停止します。データの永続化は、中断したエージェントの作業が自動で再開することを保証するものではありません。

この構成は、信頼できる 1 人のユーザーが利用するためのものです。
ワイルドカードを使ったプロジェクトのプレビューには追加設定が必要です。Android/ADB のリモート接続機能は含まれていません。
この方法で設置するローカルアプリは、上流プロジェクトの複数ユーザー向けクラウドバックエンドとは別の構成です。

## LVK を開発する

`package.json` に記載された Node.js と pnpm、および `rust-toolchain.toml` に記載された Rust ツールチェーンを使用します。
`cargo-watch` をインストールし、次を実行してください。

```bash
pnpm install --frozen-lockfile
pnpm run dev
```

フロントエンドの変更は Vite が反映します。Rust バックエンドの変更は `cargo watch` が再ビルドして再起動します。
ビルド結果を再利用できるよう、Cargo の target ディレクトリは実行のたびに削除しないでください。

サーバー上で開発する場合は、[ソース開発環境のセットアップ](docs/self-hosting/server-container.mdx#develop-lvk-on-the-server)に従ってください。
公開済みのイメージを使い、独立した開発用コンテナを起動します。**ソースを編集するたびにイメージを再ビルドする必要はありません。**
開発版には専用の HTTPS ポート、データベース、home ボリューム、work ボリュームがあります。
開発版の再起動中も、通常版でエージェントの作業を管理できます。

| コマンド                  | 用途                                                               |
| ------------------------- | ------------------------------------------------------------------ |
| `pnpm run dev`            | ローカルでフロントエンドとバックエンドを起動                       |
| `pnpm run dev:container`  | コンテナ向けに開発ポートを固定して起動                             |
| `pnpm run check`          | Web の TypeScript 型チェックとルート Rust ワークスペースのチェック |
| `pnpm run lint`           | Web の lint、ルート Rust の Clippy、未使用の翻訳キーの確認         |
| `pnpm run format`         | Rust と Web コードのフォーマット                                   |
| `pnpm run server:check`   | サーバー設定とアクセス制御のテスト                                 |
| `cargo test --workspace`  | ルート Rust ワークスペースのテスト                                 |
| `pnpm run generate-types` | Rust から共有 TypeScript 型を生成                                  |

独立した `crates/remote` Rust ワークスペースには、上流の非公開依存関係があります。
ローカル版 LVK の開発には不要です。検証範囲は [AGENTS.md](AGENTS.md) を参照してください。
`shared/` 内の生成ファイルは直接編集しないでください。

リリース用のビルドと公開は、GitHub Actions の [`publish-server.yml`](.github/workflows/publish-server.yml) で行います。
ワークフローでサーバーイメージをテストした後、バージョン付きのイメージを GHCR に公開します。
日常のソース検証やホットリロードに、リリースビルドは必要ありません。

## ワークスペースとリポジトリの記憶

登録したリポジトリでは、`.evk-shared/persistent` と `.evk-shared/cache` を通じてローカルファイルを共有できます。
これらは Git の管理対象外となるリンクで、各ワークスペースから共有されます。
言語ごとのキャッシュは明示的に設定し、重要なローカルファイルはバックアップしてください。

[OpenWiki](docs/workspaces/openwiki.mdx) はリポジトリごとに設定します。
コーディングエージェントが変更マニフェストを作成し、ソース統合後にリポジトリの知識を更新します。
ソースの予約、検証、公開、復旧については、[統合操作ガイド](docs/design/parallel-integration-implementation.md#operation-and-recovery)を参照してください。

## 名前と互換性

製品名は **Lucky Vibe Kanban**、略称は **LVK**、リポジトリ名は `lucky-vibe-kanban` です。
互換性のため、従来の保存先名（`vibe-kanban`、`.evk-shared`）、`EVK_*` / `VK_*` の設定キー、ネイティブバイナリ名は維持しています。
これらの識別子の変更には、別途データ移行が必要です。
既存の `easy-vibe-kanban` npm インストールが、自動で Docker に移行することはありません。

## 謝辞とライセンス

LVK は [Easy Vibe Kanban](https://github.com/toby1123yjh/easy-vibe-kanban) と
[Vibe Kanban](https://github.com/BloopAI/vibe-kanban) を基にしています。
両プロジェクトの貢献と履歴は、このプロジェクトの一部として引き継がれています。
ライセンスは [Apache-2.0](LICENSE) です。帰属表示は [NOTICE](NOTICE) を参照してください。
