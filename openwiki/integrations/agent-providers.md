---
type: reference
title: Agent provider・設定・MCP・Skill
description: EVK の provider adapter が保証する能力、設定の解決、Skill の注入、MCP の二つの接続方向と変更時の境界。
tags: [providers, capabilities, mcp, skills, configuration]
sources:
  - id: openwiki-source-a5b446edff482bd8a00e1a92
    resource: repo://crates/executors/src/agent_settings/public_api.rs
  - id: openwiki-source-ff0c687c0ecdbf94f91da913
    resource: repo://crates/executors/src/agent_tools.rs
  - id: openwiki-source-9ad3b97da67cba82e9d5ffc0
    resource: repo://crates/executors/src/agent_tools/public_api.rs
  - id: openwiki-source-13210275fc6cb3d346bc25ae
    resource: repo://crates/executors/src/approvals.rs
  - id: openwiki-source-7fd554effd70be44f98fe686
    resource: repo://crates/executors/src/config_write.rs
  - id: openwiki-source-22b82518475034ade38104e6
    resource: repo://crates/executors/src/executors/codex.rs
  - id: openwiki-source-c88a7c6fabdac15166554032
    resource: repo://crates/executors/src/executors/codex/client.rs
  - id: openwiki-source-38fc795b99b30483be700b71
    resource: repo://crates/executors/src/executors/codex/client/openwiki.rs
  - id: openwiki-source-1222b138c478c7fe70625991
    resource: repo://crates/executors/src/executors/mod.rs
  - id: openwiki-source-f4e0f9b2ce513d3f8b8b070e
    resource: repo://crates/executors/src/executors/provider_adapter.rs
  - id: openwiki-source-38bb7eaa90f0c92a0514844d
    resource: repo://crates/executors/src/legacy_wiki.rs
  - id: openwiki-source-f07f62eba81c4ffbaf0db9d5
    resource: repo://crates/executors/src/profile.rs
  - id: openwiki-source-8fd7f4fe2d73e778cc63c256
    resource: repo://crates/local-deployment/src/process_host.rs
  - id: openwiki-source-0a300791bc056c6eee6a577a
    resource: repo://crates/mcp/src/bin/vibe_kanban_mcp.rs
  - id: openwiki-source-773e3d7e88410badd1cab059
    resource: repo://crates/mcp/src/task_server/mod.rs
  - id: openwiki-source-4c00e6ad62d940e70c2ebbb3
    resource: repo://crates/mcp/src/task_server/tools/mod.rs
  - id: openwiki-source-a9b6d4be264787a78357c9d6
    resource: repo://crates/server/src/routes/config.rs
  - id: openwiki-source-dd14ba47f6d5e604d8574f1d
    resource: repo://crates/utils/src/shell.rs
  - id: openwiki-source-bd8b8c5ef2cd6ade70bfa3e5
    resource: repo://docs/future/agent-runtime/delegated-agent-display.md
generated: { by: "codex", at: "2026-09-22T02:14:43.589Z" }
verified:
  - by: openwiki/0.5.1
    at: 2026-09-22T02:14:43.589Z
---

# Agent provider・設定・MCP・Skill

EVK は provider ごとの通信を adapter で解釈し、共通の [AgentRun](../concepts/session-and-agent-run.md) として実行する。現行の通常ビルドの CodingAgent は ClaudeCode、Gemini、Codex、OhMyPi の4種類である。provider を追加するときは enum と起動処理だけでなく、能力の解決、制御の符号化、native event の正規化を一緒に扱う。[対象 enum](../../crates/executors/src/executors/mod.rs#L130-L137)・[adapter 境界](../../crates/executors/src/executors/provider_adapter.rs#L1-L6)

## Profile と実行能力

Profile は保存した provider 設定の variant であり、個々の実行では model、agent、reasoning、permission、execution mode、budget、subagent concurrency の override を適用できる。Code / Plan / Goal / PlanWithGoal と PermissionPolicy は別軸である。設定画面の操作は [Agent Configurations](../../docs/settings/agent-configurations.mdx)、実行中の変更・Goal の寿命は [Session と AgentRun](../concepts/session-and-agent-run.md) を参照する。[型と override](../../crates/executors/src/profile.rs#L21-L37)・[override の項目](../../crates/executors/src/profile.rs#L141-L176)

Profile は既定値と user の profiles.json を合成して読み、保存時は既定値との差分を正規化・検証して書く。読み込みの parse 失敗はログを残して既定値へ戻るため、画面に選択肢があるだけでは保存した設定の適用を確認できない。[読み書き](../../crates/executors/src/profile.rs#L351-L410)

次は adapter が実行用 snapshot に記録する状態であり、上流製品全体の機能一覧ではない。4種類とも SessionResume / Images / Review / MCP / TokenUsage は Native。差がある項目は以下の通り。

| Provider | Steering | Approval | Subagents | Goal |
| --- | --- | --- | --- | --- |
| Gemini | Unsupported | Native | Unknown | Unsupported |
| Codex | Native | Native | Unknown | Native |
| ClaudeCode | Native | Native | Native | Unsupported |
| OhMyPi | Native | Unsupported | Native | Unsupported |

Unknown は能力が解決されていない状態であり、「上流に機能がない」という証拠にはならない。能力を要求する経路は Native を許可し、Emulated は明示許可を要し、Unknown と Unsupported は拒否する。native 子 agent の観測と capability gating の整合性を変える際は両方を確認する。[snapshot](../../crates/executors/src/executors/provider_adapter.rs#L364-L530)・[能力の検証](../../crates/executors/src/executors/provider_adapter.rs#L684-L703)

Approval が Native という capability snapshot と、host に人の応答を待つサービスが接続されていることは別である。[Codex の現行 Noop 配線](../concepts/session-and-agent-run.md#承認質問と現在の接続状態) を合わせて確認する。

起動時には provider と profile の一致を検証する。Initial は native session を持てず、FollowUp / Resume は native session を必要とする。reset_to_message_id を伴う会話の巻き戻しは ClaudeCode の FollowUp / Resume に限定される。native session ID は UUID と仮定せず、空白・制御文字や provider ごとの option 解釈の危険を拒否する。[起動の検証](../../crates/executors/src/executors/provider_adapter.rs#L130-L243)

### Codex の override と解決済み effort を分ける

EVK の profile/実行 override が空でも、app-server は利用者・project 設定から model と reasoning effort を解決できる。client は `thread/start` / `thread/resume` の応答を採用してから collaboration mode と turn を構築する。したがって依頼の JSON の `reasoning_effort: null` だけで「設定ファイルの effort が無視された」と結論しない。逆に provider が返した `None` も解決結果として採用し、前回の明示値を残さない。[採用処理](../../crates/executors/src/executors/codex/client.rs#L183-L190)、[start/resume と turn](../../crates/executors/src/executors/codex/client.rs#L257-L322)

実際の値を診断するときは、依頼 override と native の thread 応答・後続 turn の設定を分けて確認する。継承の回帰テストは [reasoning_tests](../../crates/executors/src/executors/codex/client/reasoning_tests.rs#L67-L205) にある。EVK の型が値を表現できることを、任意の provider/model がその値に対応する保証とは扱わない。

## Skill と Tool Manager

Codex の selected_skills は name/path の参照であり、渡し方は実行経路によって異なる。Skill が利用可能であることは、その Skill を必ず実行する要求ではない。

| 経路 | Skill を渡す場所と順序 |
| --- | --- |
| 通常 chat（Goal の直接起動以外） | 構造化 `UserInput::Skill` を本文 `UserInput::Text` より前に並べ、turn を開始する |
| Goal の直接起動 | 選択した name/path の JSON 参照を thread の `developer_instructions` に追加してから thread を start/resume し、Goal を開始する。既存の developer instructions は保持する |

[起動分岐](../../crates/executors/src/executors/codex.rs#L1141-L1190)、[chat 入力](../../crates/executors/src/executors/codex.rs#L1347-L1364)、[Goal context と resume への継承](../../crates/executors/src/executors/codex.rs#L26-L40)、[resume 変換](../../crates/executors/src/executors/codex.rs#L78-L98)

この区別の理由は、Goal activation に UserInput 配列がないこと、余分な推論 turn を発生させず、永続する 4,000 文字上限の objective を Skill 参照で膨らませないことである。これはソースコメントと [Control and skills の設計説明](../../docs/future/agent-runtime/delegated-agent-display.md#control-and-skills) に記録された理由である。同文書は明示的な承認・廃止ステータスを持たず Validation plan も含むが、この Skill の節は現行コードおよび [通常 chat の順序テスト](../../crates/executors/src/executors/codex.rs#L1915-L1942)、[Goal の初回・再開・既存指示保持テスト](../../crates/executors/src/executors/codex.rs#L2031-L2054) と整合する。文書内の将来の再投影構想まで実装済みとは扱わない。

両経路とも、旧 EVK 同梱 `skills/llm-wiki` 配下の参照を除外する。同じ名前でも利用者が別の場所に置いた Skill は残し、既存ファイルを削除しない。旧 Pipeline の互換処理は [Card context と LLM Wiki の終了](../concepts/card-context-and-llm-wiki.md)に置く。[選択のフィルター](../../crates/executors/src/legacy_wiki.rs#L101-L119)

PlanWithGoal の承認後の遷移は [Session の承認・質問](../concepts/session-and-agent-run.md#承認質問と現在の接続状態)、resume 時に現在の host context を会話へ反映する順序と失敗は [Provider session の継続](../concepts/session-and-agent-run.md#provider-session-の継続と取り込み)を参照する。

Tool Manager は MCP Server と Skill を User / Project scope で扱う。Project は project_path があるときに探索し、個別の読み取りエラーも結果に収集する。これは会話単位の設定ではなく provider の実ファイルを管理する API である。[型](../../crates/executors/src/agent_tools.rs#L89-L117)・[探索](../../crates/executors/src/agent_tools.rs#L591-L618)・[操作 API](../../crates/server/src/routes/agent_tools.rs#L1-L156)

変更・削除と copy 元は expected_revision を照合し、既存対象への copy は replace と対象 revision を必要とする。provider をまたぐ MCP コピーは移植可能な部分を出力する。元 provider 専用 metadata は内部の結果には残るが、通常の公開応答には秘密値を含む原文として返さない。コピー成功は元設定の全項目の再現を意味しない。[copy の内部処理](../../crates/executors/src/agent_tools.rs#L495-L548)、[変更・削除の照合](../../crates/executors/src/agent_tools.rs#L698-L728)、[copy の公開応答](../../crates/executors/src/agent_tools/public_api.rs#L226-L239)

## 設定の公開と変更の安全境界

設定一覧・Tool discovery・diff は編集用の原文ではなく safe DTO である。env・引数・URL・header・未知の拡張、native file 本文や Skill asset は通常応答へ返さず、設定有無・項目数・非機密 allowlist を示す。parser エラーに秘密の原文行が含まれる可能性もあるため、通常エラーは定型化する。[設定 view](../../crates/executors/src/agent_settings/public_api.rs#L13-L109)、[Tool view](../../crates/executors/src/agent_tools/public_api.rs#L138-L174)

機密原文の表示・編集は明示確認と revision 検証を要する。非表示値を `***` のような文字列で round-trip せず、`Preserve / Replace / Clear` を明示する。省略項目を勝手に空にせず、未知の provider 設定も対象外なら維持する。native 設定の変更は共通の process-local lock を使うが、外部 editor の書込をロックするものではなく、観測 revision を別途照合する。[書込 intent](../../crates/executors/src/config_write.rs#L1-L27)、[明示読取](../../crates/executors/src/agent_tools/public_api.rs#L289-L302)

Host を切り替えた後に古い応答を反映しないことも、この機密設定の境界の一部である。選択した host が unavailable の場合に別 host の設定へ書き込まない。[Web の Host identity](../architecture/web-and-sync.md#ドラフト表示設定通知の保存境界)

## CLI 検出と provider 互換性

実行可否の確認と実 launch は同じ executable 解決を使う。絶対パス指定は別の PATH 候補へ置き換えず、通常名は PATH、有限時間の login-shell refresh、利用者の一般的な bin 配置を調べる。検出のために CLI を自動インストールしない。[共通 resolver](../../crates/utils/src/shell.rs#L55-L90)、[command builder](../../crates/executors/src/command.rs)

adapter は provider の nested message / tool block も解釈するが、raw frame は引き続き Audit に残す。現行 mapper v3 の補正を古い v1/v2 Audit replay へ遡及適用せず、選んだ version の意味で再生する。Claude の一つの envelope 内の text/tool の展開も adapter 内で行い、UI に provider 固有解析を持ち込まない。[version 選択](../../crates/executors/src/executors/provider_adapter.rs#L876-L916)、[mixed content](../../crates/executors/src/executors/provider_adapter.rs#L1905-L1976)

## MCP の二つの方向

### Coding agent → 外部 MCP server

EVK の設定操作は agent の native config を更新する。Codex は TOML の mcp_servers、他の現行 provider は JSON の mcpServers を用いる。advanced editor は明示的な機密読取確認と expected_revision を要し、既存 enclosing config の対象 server map を置換する。同じ provider を EVK 外で使う場合にも設定が及ぶ。[config 形式](../../crates/executors/src/executors/mod.rs#L139-L171)・[書き込み](../../crates/server/src/routes/config.rs#L363-L443)

接続例と通常の設定手順は [Connecting MCP Servers](../../docs/integrations/mcp-server-configuration.mdx) を参照する。そこに示す JSON 例を Codex の保存形式そのものと解釈しない。

OpenWiki maintenance writer は例外的に、登録 MCP が実行の前提である。EVK は該当 thread だけに `openwiki mcp --host codex` を設定し、`enabled=true` / `required=true` と 10 秒の startup timeout を指定する。thread 登録後、通常 turn / Goal activation の前に、その thread の MCP catalog で六つの lifecycle tool が揃うことを確認する。server 名や ready 状態だけでは十分としない。欠落・不正な pagination・15 秒の preflight timeout は writer 起動失敗として返し、この確認自体では Wiki run を開始しない。[thread 設定](../../crates/executors/src/executors/codex.rs#L922-L949)、[起動順](../../crates/executors/src/executors/codex.rs#L1160-L1188)、[tool 検査](../../crates/executors/src/executors/codex/client/openwiki.rs#L9-L89)

通常 coding の thread は OpenWiki MCP を無効にし、旧 `.llm-wiki` の生成停止と canonical OpenWiki の読取専用契約を developer instructions に追加する。保守 writer に設定する required MCP とは異なる所有境界である。[通常 thread の設定](../../crates/executors/src/executors/codex.rs#L952-L966)

Reviewer は別の信頼済み role で、ReadOnly / approval Never を設定し、OpenWiki MCP と Skill を thread 内で無効化する。writer の可用性確認を Reviewer に適用して、書込道具を復活させてはいけない。[Reviewer override](../../crates/executors/src/executors/codex.rs#L905-L921)。登録 MCP ではなく自作の shell/stdin bridge で OpenWiki を呼んでも、EVK の [完了証明](../operations/openwiki-maintenance.md)を代替しない。

### 外部 MCP client → EVK

EVK の MCP server は stdio で動き、HTTP backend に要求を転送する。Global は Issue、Project、Repo、Workspace、Session などを扱い、Orchestrator は実行 context を必須とする限定 router で、list_workspaces と delete_workspace を公開しない。[起動](../../crates/mcp/src/bin/vibe_kanban_mcp.rs#L23-L49)・[context の取得](../../crates/mcp/src/task_server/mod.rs#L87-L119)・[公開 router](../../crates/mcp/src/task_server/tools/mod.rs#L52-L73)

backend の解決は VIBE_BACKEND_URL が優先され、続いて host/port の環境変数と port file を用いる。したがって「stdio client で接続する」ことと「backend が必ず localhost に固定される」ことは別である。導入例は [Vibe Kanban MCP Server](../../docs/integrations/vibe-kanban-mcp-server.mdx) を参照する。[backend URL の解決](../../crates/mcp/src/bin/vibe_kanban_mcp.rs#L100-L133)

失敗は MCP の error result と success:false / error / details で返す。HTTP 2xx でも backend envelope の success と data を確認してから成功扱いにする。自動操作を追加するときは transport の成功だけで Issue や Session の操作完了を判定しない。[エラー境界](../../crates/mcp/src/task_server/tools/mod.rs#L94-L143)
