# Lucky Vibe Kanban (LVK)

**Plan work on a kanban board, run coding agents, and review their changes.**

Lucky Vibe Kanban is a self-hosted development workspace built on the Vibe Kanban
and Easy Vibe Kanban projects. Run it on your computer, or on your own server so
agent sessions can continue while your laptop is disconnected.

[中文](README.zh-CN.md) · [Server setup](docs/self-hosting/server-container.mdx) ·
[Source](https://github.com/nagisasaka/lucky-vibe-kanban) · [Apache-2.0](LICENSE)

## What you can do

- Organise issues and run coding agents such as Codex and Claude Code in Git worktrees.
- Keep multiple sessions and workspaces, inspect execution history, and review diffs.
- Compose agent steps into visual workflows, including parallel branches and conditions.
- Integrate completed local-board work through a dedicated validation workspace.
- Maintain repository memory with OpenWiki, with reconciliation after source integration.
- Run the application and agents on an always-on server through HTTPS and Basic authentication.

Agent accounts and provider subscriptions are configured separately. LVK does not
provide included model usage or a public, unrestricted agent service.

## Run locally

Install Node.js 20 or newer, install and authenticate the coding agent you want to
use, then run:

```bash
npx lucky-vibe-kanban
```

The npm distribution targets **Linux x64 and Windows x64**. For other platforms,
use a supported source-development environment; this release workflow does not
publish macOS or ARM binaries.

The local app listens on loopback by default. Use the server distribution below
for internet-facing access.

## Run on your own server

Use the [single-user server guide](docs/self-hosting/server-container.mdx) and the
files in [`deploy/server`](deploy/server). The published Linux amd64 image contains
LVK, nginx, Codex CLI, Git, Node.js, pnpm and a Rust development toolchain.

- A stable public IP is sufficient: **no purchased domain is required**.
- Let's Encrypt certificates are obtained and renewed automatically.
- HTTPS and Basic authentication protect the application.
- Docker volumes retain repositories, application data and agent credentials.
- A server or container restart stops running processes; persistence does not
  promise automatic resumption of interrupted agent work.

This is for one trusted user. Wildcard project previews require additional setup;
Android/ADB remote access is not included. The local application deployed this
way is separate from the upstream multi-user cloud backend.

## Develop LVK

Use the Node.js and pnpm versions in `package.json` and the Rust toolchain in
`rust-toolchain.toml`. Install `cargo-watch`, then:

```bash
pnpm install --frozen-lockfile
pnpm run dev
```

Vite reloads frontend changes. `cargo watch` rebuilds and restarts the Rust
backend. Keep the Cargo target directory between runs to reuse build output.

For development on a server, follow the
[source-development setup](docs/self-hosting/server-container.mdx#develop-lvk-on-the-server).
It runs a separate development container using an existing published image:
source edits do **not** require an image rebuild. The development app has its own
HTTPS port, database, home and work volumes. The stable app continues to manage
agent work while the development app restarts.

| Command                   | Purpose                                                |
| ------------------------- | ------------------------------------------------------ |
| `pnpm run dev`            | Local frontend and backend development                 |
| `pnpm run dev:container`  | Fixed development ports for container access           |
| `pnpm run check`          | Web TypeScript checks and the root Rust workspace      |
| `pnpm run lint`           | Web lint, root Rust Clippy and unused translation keys |
| `pnpm run format`         | Rust and web formatting                                |
| `pnpm run server:check`   | Server configuration and ingress tests                 |
| `cargo test --workspace`  | Root Rust workspace tests                              |
| `pnpm run generate-types` | Generate shared TypeScript types from Rust             |

The separate `crates/remote` Rust workspace has an upstream private dependency.
It is not required for local LVK development. See [AGENTS.md](AGENTS.md) for the
supported validation scope. Do not edit generated files in `shared/` directly.

Release builds and publication run in GitHub Actions. The npm workflow is
[`publish-easy-npx.yml`](.github/workflows/publish-easy-npx.yml); its historical
filename remains stable. Server images are built, tested and published by
[`publish-server.yml`](.github/workflows/publish-server.yml). Routine source
checks and hot reload do not require a release build.

## Workspaces and repository memory

Each registered repository can share local files through `.evk-shared/persistent`
and `.evk-shared/cache`. These are Git-ignored links shared across its workspaces;
configure language-specific caches explicitly and back up important local files.

[OpenWiki](docs/workspaces/openwiki.mdx) is configured per repository. Coding
agents contribute change manifests; repository memory is reconciled after source
integration. See the [integration operation guide](docs/design/parallel-integration-implementation.md#operation-and-recovery)
for source reservations, validation, publication and recovery.

## Naming and compatibility

The product is **Lucky Vibe Kanban**, abbreviated **LVK**, and the public package
is `lucky-vibe-kanban`. Historical storage names (`vibe-kanban`, `.evk-shared`),
`EVK_*`/`VK_*` configuration keys and native binary names remain compatible.
Renaming these identifiers requires a separate data migration. Existing
`easy-vibe-kanban` installations do not automatically switch npm packages.

## Acknowledgements and licence

LVK builds on [Easy Vibe Kanban](https://github.com/toby1123yjh/easy-vibe-kanban)
and [Vibe Kanban](https://github.com/BloopAI/vibe-kanban). Their contributions and
history remain part of this project. Licensed under [Apache-2.0](LICENSE); see
[NOTICE](NOTICE) for attribution.
