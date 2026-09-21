# ファイル

- [Arena による比較と選択](arena.md) - 同じ Issue に対する複数の Workspace を比較する Arena の意味、Design と Implementation の契約、選択・再試行・終了の寿命。
- [Card context と並列作業の参照](card-context-and-llm-wiki.md) - 保存済み Card context の継承、並列 Workspace の参照、固定 commit の読取、および旧 LLM Wiki の廃止境界。
- [正式 Integration の選択・検証・公開](formal-integration.md) - local Board で採用した Card と Workspace の変更を固定し、host 検証済み commit だけを公開する契約。予約、取消、条件付き Done、Wiki と回復の境界。
- [Issue の添付・購読・通知](issue-collaboration.md) - Cloud の Issue・Comment に付随するファイルの確定と保存寿命、Assignee・Follower の通知対象、同期と部分失敗の契約。
- [Organization・Member・Invitation](organization-and-membership.md) - Cloud の組織が所有するデータ、所属と管理権限、personal organization、招待の受理と最後の Admin の保護。
- [Project・Issue と実施の関係](project-and-issue.md) - ボード上の仕事と実行環境を結ぶ Project・Issue の意味、ローカル保存、旧 Task との境界。
- [Repository Memory の三層と統合契約](repository-memory.md) - 会話、Workspace Memory、canonical OpenWiki の権威と寿命、Change Manifest と統合記録・receipt の関係。
- [Session・AgentRun・RunAttempt](session-and-agent-run.md) - 会話・依頼・実行試行の識別、継続と retry、provider binding、Goal と停止の契約。
- [Workflow Attempt とグラフの契約](workflow-attempt.md) - Issue 専用の実施グラフ、安定 Session、run snapshot、辺の発火・分岐・再実行の意味。
- [Workspace と Repository](workspace.md) - 作業環境の所有権と用途、実行所有者、複数 Repo、共有資源、削除と回復の境界。
