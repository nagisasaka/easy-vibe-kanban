# Repository Guidelines

## Project Structure & Module Organization
- `crates/`: Rust workspace crates — `server` (API + bins), `db` (SQLx models/migrations), `executors`, `services`, `utils`, `git` (Git operations), `api-types` (shared API types for local + remote), `review` (PR review tool), `deployment`, `local-deployment`, `remote`.
- `packages/local-web/`: Local React + TypeScript app entrypoint (Vite, Tailwind). Shell source in `packages/local-web/src`.
- `packages/remote-web/`: Remote deployment frontend entrypoint.
- `packages/web-core/`: Shared React + TypeScript frontend library used by local + remote web (`packages/web-core/src`).
- `shared/`: Generated TypeScript types (`shared/types.ts`, `shared/remote-types.ts`) and agent tool schemas (`shared/schemas/`). Do not edit generated files directly.
- `assets/`, `dev_assets_seed/`, `dev_assets/`: Packaged and local dev assets.
- `npx-cli/`: Files published to the npm CLI package.
- `scripts/`: Dev helpers (ports, DB preparation).
- `docs/`: Documentation files.

### Crate-specific guides
- [`crates/remote/AGENTS.md`](crates/remote/AGENTS.md) — Remote server architecture, ElectricSQL integration, mutation patterns, environment variables.
- [`docs/AGENTS.md`](docs/AGENTS.md) — Mintlify documentation writing guidelines and component reference.
- [`packages/local-web/AGENTS.md`](packages/local-web/AGENTS.md) — Web app design system styling guidelines.

## Managing Shared Types Between Rust and TypeScript

ts-rs allows you to derive TypeScript types from Rust structs/enums. By annotating your Rust types with #[derive(TS)] and related macros, ts-rs will generate .ts declaration files for those types.
When making changes to the types, you can regenerate them using `pnpm run generate-types`
Do not manually edit shared/types.ts, instead edit crates/server/src/bin/generate_types.rs

For remote/cloud types, regenerate using `pnpm run remote:generate-types`
Do not manually edit shared/remote-types.ts, instead edit crates/remote/src/bin/remote-generate-types.rs (see crates/remote/AGENTS.md for details).

## Build, Test, and Development Commands
- Install: `pnpm i`
- Run dev (web app + backend with ports auto-assigned): `pnpm run dev`
- Backend (watch): `pnpm run backend:dev:watch`
- Web app (dev): `pnpm run local-web:dev`
- Type checks: `pnpm run check` (local-web, remote-web, web-core, UI, and the root Rust workspace) and `pnpm run backend:check` (root Rust workspace only; excludes the separate `crates/remote` workspace)
- Rust tests: `cargo test --workspace` (root Rust workspace only)
- Generate TS types from Rust: `pnpm run generate-types` (or `generate-types:check` in CI)
- Prepare SQLx (offline): `pnpm run prepare-db`
- Prepare SQLx (remote package, postgres): `pnpm run remote:prepare-db`
- NPX release validation: follow `PROJECT.md`. Commit and push the branch, run the GitHub Actions npm publish workflow (`publish-easy-npx.yml`), then verify/install from the official npm registry. Do not use local build/pack artifacts as the validation or release path unless the user explicitly asks for local-only debugging.
- Format code: `pnpm run format` (runs `cargo fmt` for all backend Rust workspaces + web-core/web Prettier)
- Lint: `pnpm run lint` (runs web/ui ESLint, `cargo clippy` for the root Rust workspace, and the unused-i18n-key check)

### Public-fork validation scope

- Normal local EVK development must not require access to upstream private repositories. `crates/remote/Cargo.toml` declares the private `BloopAI/vibe-kanban-private` billing dependency. Direct Cargo checks can attempt to resolve it even when the billing feature is disabled; installing SSH alone does not grant access.
- Do not run the separate remote backend's Cargo checks, tests, lint, or type generator as routine completion gates in this environment. Do not repeatedly attempt the private fetch, install SSH, configure credentials, or rewrite Cargo manifests merely to make these gates pass.
- For remote backend work, run `pnpm run remote:check`, `pnpm run remote:lint`, and `pnpm run remote:test` only in an environment with the required dependency access, or use an explicitly scoped, supported public validation path. The existing self-hosted Docker build removes the private dependency inside its build context; changing the source manifest to imitate it is not a normal local validation step.
- Keep remote-web frontend type checking and remote Rust formatting: these do not require fetching the private billing crate.
- Report the scope actually validated. Unavailable remote backend checks are outside the required gates for local-only work, not a reason to keep that task unfinished. If changes affect the remote backend or its shared contracts, explicitly report that remote compilation/tests remain unverified; never count skipped checks as passed.

## Project Build Policy
- This section embeds the current local `PROJECT.md` policy. `PROJECT.md` itself is local-only and does not need to be committed or pushed.
- For validation builds, use GitHub Actions to build and publish to npm. Do not attempt to build release packages on this machine.
- After the action completes, verify the published `easy-vibe-kanban` version with the official npm registry (`https://registry.npmjs.org/`) and install/run from npm for user-facing checks.
- `git-build.md` may document historical artifact-based flows, but `PROJECT.md` overrides it for current build/release decisions.
- Server container distribution: `.github/workflows/publish-server.yml` builds and validates the root Dockerfile's `server` target in GitHub Actions, then publishes version-tagged images to GHCR. A manual dispatch builds/tests without publishing. This is separate from npm release validation; do not run local Docker release builds unless explicitly requested. Docker-free validation is `pnpm run server:check`, plus the relevant Rust/frontend gates. Report container/host tests as unverified when Docker or host access is unavailable.

## Before Completing a Task
- Run `pnpm run format` to format all Rust workspaces and web code.

## Coding Style & Naming Conventions
- Rust: `rustfmt` enforced (`rustfmt.toml`); group imports by crate; snake_case modules, PascalCase types.
- TypeScript/React: ESLint + Prettier (2 spaces, single quotes, 80 cols). PascalCase components, camelCase vars/functions, kebab-case file names where practical.
- Keep functions small, add `Debug`/`Serialize`/`Deserialize` where useful.

## Testing Guidelines
- Rust: prefer unit tests alongside code (`#[cfg(test)]`), run `cargo test --workspace`. Add tests for new logic and edge cases.
- Web app: ensure `pnpm run check` and `pnpm run lint` pass. If adding runtime logic, include lightweight tests (e.g., Vitest) in the same directory.

## Security & Config Tips
- Use `.env` for local overrides; never commit secrets. Key envs: `FRONTEND_PORT`, `BACKEND_PORT`, `HOST` 
- Dev ports and assets are managed by `scripts/setup-dev-environment.js`.
