---
type: architecture
title: Web の実行環境と同期
description: Local・Remote shell が共有画面へ接続先を注入する仕組みと、Electric・fallback・AgentRun stream の異なる同期契約。
tags: [frontend, synchronization, electric, transport]
sources:
  - id: openwiki-source-81dada2f014b934994b8ed97
    resource: repo://crates/remote/src/routes/electric_proxy.rs
  - id: openwiki-source-162bcfe1ee47c1047ccf7d52
    resource: repo://crates/server/src/routes/agent_runs.rs
  - id: openwiki-source-a623a548ee805a854d7d5e18
    resource: repo://packages/local-web/src/app/entry/App.tsx
  - id: openwiki-source-c0121fb1c48b7f86e87da731
    resource: repo://packages/remote-web/src/app/entry/App.tsx
  - id: openwiki-source-71d9a385f4b03b8ca71765b0
    resource: repo://packages/web-core/src/features/wiki/model/wikiViewerController.ts
  - id: openwiki-source-202a77c218ffbc8ac2871a04
    resource: repo://packages/web-core/src/features/wiki/ui/WorkspaceWikiProvider.tsx
  - id: openwiki-source-2fb74c0fc81d817149606557
    resource: repo://packages/web-core/src/pages/workspaces/PreservedChatPanel.tsx
  - id: openwiki-source-0894249b732085a9800940bd
    resource: repo://packages/web-core/src/pages/workspaces/WorkspacesLayout.tsx
  - id: openwiki-source-c03336fce241c80068d03edc
    resource: repo://packages/web-core/src/pages/workspaces/WorkspacesMainContainer.tsx
  - id: openwiki-source-2bc4d71db2cf73c3210b9268
    resource: repo://packages/web-core/src/shared/hooks/useNotifications.ts
  - id: openwiki-source-43d7a2f6bd06c556e7d48e3c
    resource: repo://packages/web-core/src/shared/hooks/useScratch.ts
  - id: openwiki-source-b885bc17dfe85c4ca45223e9
    resource: repo://packages/web-core/src/shared/hooks/useUiPreferencesScratch.ts
  - id: openwiki-source-ffa337e3881da0103a10e223
    resource: repo://packages/web-core/src/shared/hooks/useVisualViewportHeightVar.ts
  - id: openwiki-source-012d640135a03bd20065dc93
    resource: repo://packages/web-core/src/shared/hooks/useWorkspaceOwner.ts
  - id: openwiki-source-97cd8794805cef30e1c2f047
    resource: repo://packages/web-core/src/shared/hooks/useWorkspaces.ts
  - id: openwiki-source-8860c99d38e029d2bcc112c2
    resource: repo://packages/web-core/src/shared/hooks/useWorkspaceSessions.ts
  - id: openwiki-source-29a47cc7d1a22c6d0ddf4b6c
    resource: repo://packages/web-core/src/shared/hooks/workspaceSessionSelection.test.ts
  - id: openwiki-source-cf7deb2cd54542f4c2fd7515
    resource: repo://packages/web-core/src/shared/hooks/workspaceSessionSelection.ts
  - id: openwiki-source-e665e0fe8e478ec542e0ace4
    resource: repo://packages/web-core/src/shared/lib/electric/collections.ts
  - id: openwiki-source-9ca1dca04d93c4fed8664d25
    resource: repo://packages/web-core/src/shared/lib/localApiTransport.ts
  - id: openwiki-source-d4726fdd33da0f7c2f1ae1ee
    resource: repo://packages/web-core/src/shared/lib/remoteApi.ts
generated: { by: "codex", at: "2026-09-21T08:16:36.701Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-21T08:16:36.701Z
---

# Web の実行環境と同期

## 画面と実行環境を分ける

Local と Remote の App はどちらも TanStack Router と Hotkeys を利用し、`AppRuntimeProvider` にそれぞれの runtime を渡す。Local はさらに設定、認証、Tauri listener を組み立てる。共有の業務画面・hooks は `web-core` に置き、shell の接続・ナビゲーションと結び付ける。[Local App](../../packages/local-web/src/app/entry/App.tsx#L1-L43)、[Remote App](../../packages/remote-web/src/app/entry/App.tsx#L1-L16)

UI package は既に Kanban や chat、preview などの再利用部品を持つ。[Kanban の例](../../packages/ui/src/components/KanbanBoard.tsx#L1-L58)。`packages/ui/README.md` の「initial scaffold」と [抽出前の監査](../../docs/frontend-ui-library-refactor-audit.md)は現行配置の全体図ではない。監査は責任分割の経緯を知る資料として読み、古い `local-web/src/components` の位置を変更先にしない。

## 同じ API 呼び出しでも対象ホストが変わる

`localApiTransport` は HTTP と WebSocket を統一し、`current`、`explicit`、`none` の host scope を解決する。対象 host がある `/api/...` は `/api/host/{hostId}/...` へ変換される。ただし relay 認証や remote editor のローカル資格情報を使う入口は変換しない。[scope 解決](../../packages/web-core/src/shared/lib/localApiTransport.ts#L28-L88)

新しいファイル表示や端末処理で素の `fetch` / `WebSocket` を追加すると、この境界を落とす可能性がある。対象ホスト上の [Workspace 操作](../operations/workspace-inspection.md)は transport 経由で追う。

`remoteApi` は build-time URL を初期値とし、実行時設定で上書きする。local remote API が有効かつ外部 URL が空なら `/v1/...` を `/api/local/v1/...` に送り、host scope は `none` とする。それ以外は Bearer token を取得し、401 では一度 refresh して再送する。[接続先の契約](../../packages/web-core/src/shared/lib/remoteApi.ts#L19-L134)

Cloud の access / refresh token は、ホスト pairing の signing session と別物である。401 の診断では [AuthSession の期限・更新競合・失効](../integrations/remote-access.md#cloud-authsession-と-token-の寿命)を確認する。

## Issue データの読み取りと書き込み

[Project・Issue](../concepts/project-and-issue.md)の共有画面は collection を利用する。Remote の Electric proxy は table と WHERE をサーバー側の shape から設定し、client parameter は許可された項目だけ転送する。認証・membership の境界は [Remote 接続](../integrations/remote-access.md)を参照。[proxy](../../crates/remote/src/routes/electric_proxy.rs#L30-L79)

書き込みは REST mutation であり、成功レスポンスの `txid` を Electric collection に返す。fallback 時は txid の同期を待つ経路ではなく、snapshot の再取得を促す。[mutation handler](../../packages/web-core/src/shared/lib/electric/collections.ts#L650-L682)

| 読み取り方式 | 契約 |
| --- | --- |
| ローカルの互換 API | 最初から fallback snapshot を利用 |
| Remote の通常経路 | Electric を開始し、利用不能時に fallback へ切り替える |
| fallback | 初回取得と 30 秒周期の再取得。source 単位で切り替え状態を保持 |

表示中のページで Electric が 3 秒経過しても Ready にならないと、その source を fallback に固定する。非表示中は判定を延期する。fallback の取得失敗はエラーを報告し、初回 Ready 待ちを解除することがあるため、Ready はデータ取得成功と同義ではない。[fallback](../../packages/web-core/src/shared/lib/electric/collections.ts#L453-L532)、[timeout](../../packages/web-core/src/shared/lib/electric/collections.ts#L580-L605)、[モード選択](../../packages/web-core/src/shared/lib/electric/collections.ts#L817-L846)

## ドラフト・表示設定・通知の保存境界

Scratch は Electric の共有 Issue データとは別の UI 状態である。local-web は対象 local API の SQLite と JSON Patch WebSocket、remote-web はそのブラウザー origin の localStorage を使う。remote-web の Scratch は Cloud への保存や端末間同期を意味しない。composer のキー、保存失敗、復元・消去と送信待ち queue の違いは [Session の未送信ドラフト](../concepts/session-and-agent-run.md#未送信ドラフトと-scratch)に集約する。[runtime 分岐](../../packages/web-core/src/shared/hooks/useScratch.ts#L26-L86)

Kanban の Project / view 別表示設定も UI preferences の Scratch に入る。同じ画面でも、表示設定を保存する処理と Issue を変更する REST mutation では共有範囲が異なる。Team / Personal、列操作、blocked 判定の意味は [ボード表示と共有状態](../concepts/project-and-issue.md#ボード表示と共有状態)で確認する。[preferences の保存形式](../../packages/web-core/src/shared/hooks/useUiPreferencesScratch.ts#L27-L98)

Cloud 通知はログイン user_id を引数に notifications shape を購読し、既読更新用の mutation を使う。UI は通知を group 化して未読件数を求める。送信先、通知生成の失敗、ローカル OS 通知との関係は [Issue の購読と通知](../concepts/issue-collaboration.md#購読と通知)が一次説明である。[購読 hook](../../packages/web-core/src/shared/hooks/useNotifications.ts#L10-L42)

## Agent の会話は別の同期経路

AgentRun stream は Electric ではない。サーバーは Live を先に購読し、その間の到着を受け止めてから永続履歴を replay する。cursor は RunAttempt 番号と canonical sequence で進め、Live 通知で進めない。再接続時に usage の最新 snapshot も送る。[stream](../../crates/server/src/routes/agent_runs.rs#L391-L484)

会話 projection は表示用 entry に変換しても AgentRun、RunAttempt、event ID を保持する。この識別を外すと、別 attempt の表示と制御が混ざり得る。[表示 identity](../../packages/web-core/src/features/agent-runtime/model/canonicalAgentConversation.ts#L25-L73)。永続化・一時差分・劣化時の詳細は [Agent Runtime](agent-runtime.md)に集約する。

## Wiki のナビゲーションと本文を一つの状態で動かす

Workspace の Wiki は本文をメイン領域、ナビゲーションを右側へ分けて表示するが、二つの独立した reader ではない。`WorkspaceWikiProvider` が Workspace ID ごとに一つの controller を所有し、repository、選択ページ、検索、読取結果を両 surface で共有する。モバイルでは本文とナビゲーションを切り替える。[共有 controller](../../packages/web-core/src/features/wiki/ui/WorkspaceWikiProvider.tsx#L39-L91)、[画面への配置](../../packages/web-core/src/pages/workspaces/WorkspacesLayout.tsx#L509-L628)

表示対象は canonical OpenWiki 一種類であり、legacy 形式の切替状態は持たない。repository を切り替えると snapshot・選択・検索・scroll 状態をリセットし、再取得する。controller はリクエスト sequence と応答の Workspace / repository identity を検査し、遅れて返った別対象の結果で現在の画面を上書きしない。[scope と応答の扱い](../../packages/web-core/src/features/wiki/model/wikiViewerController.ts#L11-L119)。実際に読む checkout と reader の範囲は [Wiki inspection](../operations/workspace-inspection.md#wiki-の本文ツリーと表示元を確認する)を参照する。

Wiki を開くことはチャットや AgentRun の終了ではない。`PreservedChatPanel` は composer と timeline を unmount せず、非表示時に `inert` / `aria-hidden` を付ける。最後の表示幅を保つのは、幅ゼロによる仮想化 timeline の再計算を避けるためである。Wiki の表示設定と実行 lifecycle を結び付けない。[chat の保持](../../packages/web-core/src/pages/workspaces/PreservedChatPanel.tsx#L4-L52)。操作と表示元の確認は [Workspace の検査](../operations/workspace-inspection.md)を参照する。

## 作業一覧と実行履歴を分ける

Workspace stream と summary は用途を持ち、通常 Work / Archive から `execution_only` を除き、archived の実行専用環境も Executions / History 側へ集める。通常作業の running 表示は最新一件だけで判定せず、summary の実行集計を優先する。[一覧の分割](../../packages/web-core/src/shared/hooks/useWorkspaces.ts#L61-L79)、[用途による分類](../../packages/web-core/src/shared/hooks/useWorkspaces.ts#L237-L255)。用途と製品所有者の契約は [Workspace](../concepts/workspace.md#操作用途と実行所有者)に集約する。

実行専用画面は composer の代わりに inspection panel を表示し、製品の owner view を明示 host scope で取得する。query key に host と Workspace を含め、失敗を成功した空データに変換しない。[画面分岐](../../packages/web-core/src/pages/workspaces/WorkspacesMainContainer.tsx#L241-L263)、[owner の取得と Stop](../../packages/web-core/src/shared/hooks/useWorkspaceOwner.ts#L6-L59)。これらの表示制御に加え、backend も変更操作を検査する。

Session 一覧の再取得では利用者が選んだ古いログや new-session draft を維持する。host / Workspace または明示 Session link の変更時だけ選択を再解決し、入口に使った URL がその後の手動選択を奪わない。[選択規則](../../packages/web-core/src/shared/hooks/workspaceSessionSelection.ts#L3-L25)、[scope の更新](../../packages/web-core/src/shared/hooks/useWorkspaceSessions.ts#L51-L87)、[回帰テスト](../../packages/web-core/src/shared/hooks/workspaceSessionSelection.test.ts#L4-L36)

## モバイルと文書の適用範囲

モバイル shell は `visualViewport.height` を CSS 変数 `--app-vh` に反映する。キーボード表示時の composer 隠れを避け、重い shell を viewport のたびに React render しない意図がコードに記録されている。[実装と理由](../../packages/web-core/src/shared/hooks/useVisualViewportHeightVar.ts#L3-L50)

[モバイル設計](../../docs/ai-mobile/design.md)は draft、[390px 監査](../../docs/ai-mobile/audit-2026-07-07.md)は過去時点のギャップ調査である。QR、PWA、native shell の構想を現行保証へ置き換えず、実画面・hooks・transport の実装を確認する。画布の見た目や操作設計は [Workflow UI 設計](../../docs/future/ai-workflow/spec-ui.md)に委ね、実行順序の意味は [Workflow Attempt](../concepts/workflow-attempt.md)で確認する。
