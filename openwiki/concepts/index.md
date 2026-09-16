# ファイル

- [Arena による比較と選択](arena.md) - 同じ Issue に対する複数の Workspace を比較する Arena の意味、Design と Implementation の契約、選択・再試行・終了の寿命。
- [Card context・Pipeline・LLM Wiki](card-context-and-llm-wiki.md) - カードに保存される初期指示と、作業ブランチ内の .llm-wiki の生成・参照・更新契約。
- [Issue の添付・購読・通知](issue-collaboration.md) - Cloud の Issue・Comment に付随するファイルの確定と保存寿命、Assignee・Follower の通知対象、同期と部分失敗の契約。
- [Organization・Member・Invitation](organization-and-membership.md) - Cloud の組織が所有するデータ、所属と管理権限、personal organization、招待の受理と最後の Admin の保護。
- [Project・Issue と実施の関係](project-and-issue.md) - ボード上の仕事と実行環境を結ぶ Project・Issue の意味、ローカル保存、旧 Task との境界。
- [Repository Memory の三層と統合契約](repository-memory.md) - 会話、Workspace Memory、canonical OpenWiki の権威と寿命、Change Manifest と統合記録・receipt の関係。
- [Session・AgentRun・RunAttempt](session-and-agent-run.md) - 会話・依頼・実行試行の識別、継続と retry、provider binding、Goal と停止の契約。
- [Workflow Attempt とグラフの契約](workflow-attempt.md) - Issue 専用の実施グラフ、安定 Session、run snapshot、辺の発火・分岐・再実行の意味。
- [Workspace と Repository](workspace.md) - 作業環境の所有権、複数 Repo と Git 状態、DirectFolder、共有資源、削除と回復の境界。
