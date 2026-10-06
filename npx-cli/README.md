# Lucky Vibe Kanban (LVK)

A self-hosted kanban workspace for coding agents, Git worktrees, visual workflows
and repository memory. Built on Easy Vibe Kanban and Vibe Kanban.

## Run

Install Node.js 20 or newer, install and authenticate your coding agent, then run:

```bash
npx lucky-vibe-kanban
```

This npm distribution includes Linux x64 and Windows x64 binaries. Agent accounts
and model usage are configured separately.

```bash
npx lucky-vibe-kanban --help
npx lucky-vibe-kanban --version
npx lucky-vibe-kanban review --help
npx lucky-vibe-kanban mcp --help
```

For an always-on server with automatic IP-address HTTPS and Basic authentication,
use the Docker distribution. See the
[repository and server guide](https://github.com/nagisasaka/lucky-vibe-kanban#run-on-your-own-server).

Historical data directories and native executable names remain compatible. This
new package does not automatically replace an installed `easy-vibe-kanban` package.

## Licence and attribution

Apache-2.0. See the repository's LICENSE and NOTICE. Based on
[Easy Vibe Kanban](https://github.com/toby1123yjh/easy-vibe-kanban) and
[Vibe Kanban](https://github.com/BloopAI/vibe-kanban).
