---
type: architecture
title: ファイル・差分・プレビュー・端末の境界
description: Workspace 内のファイル・Wiki 閲覧、添付、差分配信、dev server preview、PTY、editor 操作で異なるパス・ホスト・寿命の契約を説明する。
tags: [workspace, files, wiki, diff, preview, terminal]
verified:
  - by: openwiki/0.5.1
    at: 2026-09-16T18:13:42.498Z
sources:
  - id: openwiki-source-63b741b02f122d47e34fd409
    resource: repo://crates/api-types/src/attachment.rs
  - id: openwiki-source-5aa384eaafe1283b1b8cc4a4
    resource: repo://crates/preview-proxy/src/api.rs
  - id: openwiki-source-0099d841c439c1dde012669e
    resource: repo://crates/preview-proxy/src/lib.rs
  - id: openwiki-source-a41dc363a6e8b2266dcfebdf
    resource: repo://crates/server/src/routes/host_relay/open_remote_editor.rs
  - id: openwiki-source-875a0d6590ed93103083efa6
    resource: repo://crates/server/src/routes/preview.rs
  - id: openwiki-source-7c1b10f112cb99ba82426b5a
    resource: repo://crates/server/src/routes/terminal.rs
  - id: openwiki-source-75b2a3e21f7d8cbca4f8565a
    resource: repo://crates/server/src/routes/workspaces/attachments.rs
  - id: openwiki-source-bdb4dae5722fe76ec5e44001
    resource: repo://crates/server/src/routes/workspaces/create.rs
  - id: openwiki-source-babbae011de25ca5bb9ba725
    resource: repo://crates/server/src/routes/workspaces/execution.rs
  - id: openwiki-source-37945a97e2b18bbb762fafc9
    resource: repo://crates/server/src/routes/workspaces/files.rs
  - id: openwiki-source-303d74f74def684d4690dea9
    resource: repo://crates/server/src/routes/workspaces/git.rs
  - id: openwiki-source-6c4d6d22a9d50e0def61c50d
    resource: repo://crates/server/src/routes/workspaces/integration.rs
  - id: openwiki-source-5590f47e4cee001a35a42117
    resource: repo://crates/server/src/routes/workspaces/wiki.rs
  - id: openwiki-source-b04f793d57fd8b570d4984e3
    resource: repo://crates/services/src/services/diff_stream.rs
  - id: openwiki-source-965b490de6f8364cb7695944
    resource: repo://crates/workspace-manager/src/workspace_manager.rs
  - id: openwiki-source-69854837467c3beec23ecb8d
    resource: repo://packages/web-core/src/features/wiki/model/wikiNavigation.ts
  - id: openwiki-source-0cd94f38cc1a16e284fe2b1c
    resource: repo://packages/web-core/src/features/wiki/ui/WorkspaceWikiPanel.tsx
  - id: openwiki-source-1be0c4423a1ed260c67b498f
    resource: repo://packages/web-core/src/features/workspace-files/model/workspaceFileRawUrl.test.ts
  - id: openwiki-source-5ef837b2bc54d286b3dedd8f
    resource: repo://packages/web-core/src/shared/stores/useUiPreferencesStore.ts
generated: { by: "codex", at: "2026-09-16T18:13:42.498Z" }
---

# ファイル・差分・プレビュー・端末の境界

これらの機能は同じ [Workspace](../concepts/workspace.md) の成果物を扱うが、実ファイルの閲覧、Git との差分、アプリの実行、shell の操作は別の経路である。画面の操作方法は [Reviewing Code](../../docs/reviewing-code.mdx) と [Browser Testing](../../docs/browser-testing.mdx) に譲り、ここでは実装変更に関わる契約を整理する。

## Repo 内ファイルを読む

files API は tree / directory / content / raw の GET を公開する。repo_id は WorkspaceRepo membership で解決し、その Workspace の container_ref/repo.name を root とする。任意の filesystem path を読む API ではない。[router](../../crates/server/src/routes/workspaces/files.rs#L126-L131)・[root の解決](../../crates/server/src/routes/workspaces/files.rs#L295-L320)

パスは slash を正規化し、NUL、絶対パス、Windows drive、親への .. を拒否する。さらに canonicalize した最終 target が Repo root 内にあるか確認し、外への symlink は NotFound とする。directory の列挙でも root 外へ解決される entry を省く。[パス検証](../../crates/server/src/routes/workspaces/files.rs#L323-L382)・[列挙](../../crates/server/src/routes/workspaces/files.rs#L385-L436)・[traversal / symlink test](../../crates/server/src/routes/workspaces/files.rs#L784-L821)

directory は最大2,000 entries、text preview は512 KiBまでで truncated を返す。content は Text / Image / Binary / Unsupported に分かれ、NUL や不正 UTF-8 も binary 判定に使う。先頭だけの preview を file 全体と解釈しない。[上限](../../crates/server/src/routes/workspaces/files.rs#L33-L34)・[content 分類と読み取り](../../crates/server/src/routes/workspaces/files.rs#L476-L605)

raw は streaming 応答で no-store と nosniff を付ける。明示された raster image MIME のみ inline とし、それ以外は application/octet-stream / attachment で返す。HTML や SVG を raw URL で main UI の実行 content に変えない。[raw 応答](../../crates/server/src/routes/workspaces/files.rs#L247-L292)・[MIME policy](../../crates/server/src/routes/workspaces/files.rs#L646-L664)

Remote の raw URL は host ID で /api/host/{host}/... に scope する。既に scope 済みの URL を二重化せず、外部 URL は保持する test がある。Workspace ID だけを別 host に転用しない。[URL test](../../packages/web-core/src/features/workspace-files/model/workspaceFileRawUrl.test.ts#L4-L36) [Remote 接続](../integrations/remote-access.md) は host の認証と transport を説明する。

## Wiki の本文・ツリーと表示元を確認する

Wiki を開くとメイン領域が本文、右側が repository / 形式の選択、検索、ページツリーになる。`LLM Wiki · .llm-wiki/` と `OpenWiki · openwiki/ (read-only)` は別の保存形式であり、間違った形式を選ぶと目的の Wiki は表示されない。OpenWiki の作成・Sync は repository memory settings から行い、reader は初期化や破損修復をしない。[形式と操作の分離](../../packages/web-core/src/features/wiki/ui/WorkspaceWikiPanel.tsx#L104-L161)

表示は **Current workspace**、すなわち選択 repository の現在の worktree であり、未公開の変更も含む。本文 header の repository、Workspace 名、branch、ページ path を確認する。別 worktree の成果物や未公開 Wiki を「target branch へ公開済み」と取り違えない。ファイル変更後は `Reload Wiki files` で再取得する。[表示元と再読込](../../packages/web-core/src/features/wiki/ui/WorkspaceWikiPanel.tsx#L364-L419)、[API の snapshot source](../../crates/server/src/routes/workspaces/wiki.rs#L158-L184)

ツリーは公開された Wiki snapshot のページだけから組み立て、index を入口にする。検索は本文、タイトル、summary、tags 等へ空白区切りの全 term を適用する。OpenWiki の階層内相対リンクや `/openwiki/...`、`[[wikilink]]` は既知ページへ解決するが、Wiki root 外の source path を任意ファイル読取へ変換しない。ページ切替はパス単位であり、リンクの fragment を保持した節スクロールまでこの resolver が保証するわけではない。[ツリーと検索](../../packages/web-core/src/features/wiki/model/wikiNavigation.ts#L61-L121)、[リンク解決](../../packages/web-core/src/features/wiki/model/wikiNavigation.ts#L141-L190)

本文は既存の `MarkdownPreview` を再利用する。Wiki を閉じると同一ブラウザー内で保存していた panel 構成へ戻り、再読込で戻り先が失われた場合は通常チャットへ戻る。表示切替で [Session / AgentRun](../concepts/session-and-agent-run.md) を作り直すものではない。共有 reader とチャット保持の実装は [Web の状態管理](../architecture/web-and-sync.md#wiki-のナビゲーションと本文を一つの状態で動かす)に集約する。[renderer](../../packages/web-core/src/features/wiki/ui/WorkspaceWikiPanel.tsx#L437-L451)、[表示の保存・復帰](../../packages/web-core/src/shared/stores/useUiPreferencesStore.ts#L639-L697)

legacy reader と階層 OpenWiki adapter のファイル範囲・容量上限は [Card context と LLM Wiki](../concepts/card-context-and-llm-wiki.md#閲覧変更時の注意点)、生成物の公開条件は [OpenWiki maintenance](openwiki-maintenance.md)を参照する。

## Session の添付と Cloud Attachment

この節は local の **File と Session の添付**を扱う。Cloud の Issue/Comment に属する Attachment と Azure Blob の確認・commit・期限 cleanup は [Issue の共同作業](../concepts/issue-collaboration.md) が正本の説明先であり、保存主体も削除条件も異なる。

添付 upload は保存レコードを作成し、Session の作業先へ file ID でコピーする。raw 添付の取得は Session が Workspace に属するか確認し、その agent_working_dir または Workspace root 配下の .vibe-attachments を使う。最終 canonical path が添付ディレクトリから外れれば拒否する。Repo file viewer と添付の基準 root は同じとは限らない。[upload](../../crates/server/src/routes/workspaces/attachments.rs#L82-L97)・[添付のパス](../../crates/server/src/routes/workspaces/attachments.rs#L215-L238)・[Session scope](../../crates/server/src/routes/workspaces/attachments.rs#L298-L322)

Issue を関連づけて Workspace を作る経路では、remote client が取得できれば Issue 添付を local File へ取り込み、prompt 内の参照を `.vibe-attachments/` へ書き換える。取り込みや関連づけに失敗しても warning を残して作成を続ける分岐があるため、Workspace があるだけでは Cloud 添付の利用可能性を証明しない。[取り込みと部分失敗](../../crates/server/src/routes/workspaces/create.rs#L377-L416)、[参照の生成](../../crates/server/src/routes/workspaces/create.rs#L182-L199)。upload からこの受け渡しまでの寿命は [Attachment の説明](../concepts/issue-collaboration.md) を参照する。

## 差分は現在の比較基準から再構成する

Git diff は専用の signed WebSocket 経路を持つ。diff stream は現在の base commit と worktree から差分を計算し、Repo ID と複数 Repo 用 prefix を付けて replace patch を配信する。WorkspaceRepo の target_branch が変わったら base を再計算して stream を reset する。[API](../../crates/server/src/routes/workspaces/git.rs#L137-L175)・[snapshot](../../crates/services/src/services/diff_stream.rs#L318-L375)・[target 変更](../../crates/services/src/services/diff_stream.rs#L443-L459)

stats-only と累積本文容量の上限により、diff の old/new content を省くことがある。本文がないことと変更がないことを同一視しない。追加・削除行数は可能なら省略前に計算して保持する。[省略の契約](../../crates/services/src/services/diff_stream.rs#L667-L706) レビュー後に変更を統合する手順は [Task から統合まで](../workflows/task-to-integration.md) に続く。

## Dev server と preview proxy

Dev server の開始は Workspace 内で既存 dev server の停止を試み、script のある各 Repo を Bash の ScriptRequest / DevServer として起動する。script が一つもなければ API error。途中の起動に失敗した場合、それ以前の起動を全件 rollback する構造ではない。[起動処理](../../crates/server/src/routes/workspaces/execution.rs#L40-L143)

この script 実行は [coding AgentRun](../concepts/session-and-agent-run.md) とは別の ExecutionProcess 経路である。通常の設定とログ上の URL 検出の説明は [Browser Testing](../../docs/browser-testing.mdx) を参照する。

Preview proxy は main app とは別の port で iframe content を提供する。local は {port}.localhost、remote は {port}--{host UUID}.localhost の host label から転送先を解決する。local は localhost の指定 port、remote はその host の preview API へ転送する。[分離の意図](../../crates/preview-proxy/src/lib.rs#L1-L8)・[host の解決](../../crates/preview-proxy/src/lib.rs#L366-L399)・[転送先](../../crates/preview-proxy/src/lib.rs#L481-L491)

この target は port と host の routing 情報である。Repo membership を検証する file API とは異なり、preview route は target_port を受け取る。アクセス制御を追加するときは、見えている Workspace の識別子だけで proxy の許可が決まると仮定しない。[preview route](../../crates/server/src/routes/preview.rs#L13-L65)

subdomain proxy は HTML を読み替え、head に React 検査用 bundle、body に DevTools / click-to-component 等を挿入する。CSP、X-Frame-Options などの header も除外する。Eruda の読み込みには CDN 依存がある。これは開発 preview の加工経路であり、file raw の MIME policy と同じ規則ではない。[header policy](../../crates/preview-proxy/src/lib.rs#L77-L126)・[HTML 加工](../../crates/preview-proxy/src/lib.rs#L591-L618)

一方 /api/preview/{port} の HTTP 転送は localhost upstream を呼び、失敗時は BadGateway、応答は streaming で返す。subdomain 側の injection と同じ責務を重ねない。redirect が remote host scope を維持することには focused test がある。[API 転送](../../crates/preview-proxy/src/api.rs#L33-L81)・[応答](../../crates/preview-proxy/src/api.rs#L116-L134)・[redirect test](../../crates/preview-proxy/src/lib.rs#L974-L987)

## Terminal と editor

Terminal は Workspace directory を検証し、Repo が一つならその下、複数なら Workspace root を初期 cwd とする。WebSocket 接続ごとに PTY session を作り、input/output は base64、resize は別 command として扱い、接続終了時に PTY を閉じる。会話の Session を resume する経路ではない。[cwd と起動](../../crates/server/src/routes/terminal.rs#L52-L113)・[通信と終了](../../crates/server/src/routes/terminal.rs#L118-L165)

editor は path の解決と外部 editor の起動を担う。file_path 未指定で Repo が一つならその Repo を開き、指定された file_path は Workspace path へ join する。現行 editor resolver は file viewer の normalize/canonical-root 検査を再利用していない。この差を消す変更では、既存の editor path 契約を先に確認する。[editor path](../../crates/server/src/routes/workspaces/integration.rs#L158-L183)

Remote editor は host に editor path を問い合わせ、SSH tunnel を作って desktop bridge に渡す。file URL のブラウザー表示とは依存が異なる。[Remote editor](../../crates/server/src/routes/host_relay/open_remote_editor.rs#L35-L89) IDE extension の導入手順は [VSCode Extension Integration](../../docs/integrations/vscode-extension.mdx) を参照する。外部 extension の実装や公開版の互換性は、この checkout だけでは確定しない。

## 実行履歴を調査・保全する

画面の会話履歴や process logs だけでは provider の元フレームをすべて保持した証拠にならない。[Native Audit の保存先・version 照合・bundle エクスポート](../architecture/agent-runtime.md#native-audit-の保全エクスポート寿命) は再現調査の一次説明である。Workspace の明示削除は Session の Audit ファイルも background 削除するので、保全の判断は [Workspace の削除 lifecycle](../concepts/workspace.md#archive削除期限-cleanup) と合わせて行う。[削除対象](../../crates/workspace-manager/src/workspace_manager.rs#L436-L515)
