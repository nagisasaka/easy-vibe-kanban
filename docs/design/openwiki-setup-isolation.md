# OpenWiki setup isolation in maintenance worktrees

## Decision

Keep the unmodified, pinned OpenWiki host integration and the existing dedicated
maintenance workspace. OpenWiki's root `AGENTS.md` / `CLAUDE.md` additions are not
EVK instructions and must not enter a Wiki publication. Existing repository
instructions, their file types and link targets belong to the user.

OpenWiki 0.5.1 writes both instruction files during `openwiki_begin`, before its
source fingerprint. A repository with `CLAUDE.md -> AGENTS.md` therefore receives
two concurrent writes to one inode. The public MCP contract has neither a setup
disable option nor a separate setup output directory.

## Implementation plan and invariants

1. Preflight the instruction files and setup workflow path before launching a
   model. Reject escaping/dangling links, special files and unsafe parent paths.
   Save a versioned workspace-scoped journal through the existing repository
   shared-folder store before changing any instruction file.
2. Materialise repository-internal instruction symlinks as independent temporary
   regular files, retaining their original text. Never change a linked target.
   Keep this view stable throughout the host run, including MCP/server restarts.
3. After the AgentRun is terminal, validate that changes consist only of the
   upstream-managed instruction block; reject unrelated edits rather than
   overwriting them. Journal the exact pre-restoration state before restoring
   original bytes, modes, absence and symlink targets. Restoration is idempotent.
4. Require successful native-audit host finalisation before publication. Do not
   restore during an active run: that changes OpenWiki's source fingerprint.
   Reject generated Wiki provenance referring to instruction files changed by
   setup or other unpublished setup artifacts. Keep OpenWiki Claims and private
   metadata untouched; inspect public page provenance, not opaque evidence hashes.
5. Publish only `openwiki/`, retaining the existing INSTRUCTIONS preservation,
   source-drift, Git identity, lock and publication-checkpoint guards. Known
   installer/CI by-products remain in the maintenance worktree, never the target
   branch. Failed runs retain their Wiki for inspection; no paid retry is automatic.

No API/type migration, extra worktree, custom MCP server, upstream patch, new model
provider, or changes to ordinary workspace/Goal/LLM Wiki execution are required.

## Compatibility and limits

Existing unjournalled runs must not silently manufacture a pre-run snapshot.
Their original Git instructions can be restored explicitly after verifying the
audit and preserving a backup. Existing publication checkpoints remain valid.

OpenWiki may classify a source-unchanged update as active after recreating its
discarded setup blocks. The supported empty update/finalisation path remains
valid. This change does not claim to fix update scheduling or semantic no-op
detection. Do not hide genuine instruction changes through `.openwikiignore`,
Git index flags, or by editing OpenWiki's fingerprints/Claims.

## Validation plan

Cover ordinary, absent, imported and symlinked instructions; outside links and
symlinked parents; user edits; byte/mode preservation; preparation/restoration
restart boundaries; duplicate cleanup; retained Wiki; provenance rejection; and
Wiki-only publication. Exercise the installed public MCP with deterministic pages
and no model calls, then run services/utils/server tests, formatting, type
generation checks, repository typechecks and lint. Preserve unrelated changes.

## Validation results

Validated in the Linux development container against unmodified OpenWiki 0.5.1:

- Services/utils: 85 unit/integration tests passed, including 15 setup-isolation
  tests. The opt-in installed-CLI probe also passed without a model call.
- Server: 124 unit tests and 38 integration tests passed.
- Existing Wiki/Pipeline UI and repository-memory settings: 22 Vitest tests passed.
- Public CLI/MCP fixture: initialisation, finalisation, alias isolation, repeated
  restoration, Wiki-only publication and the supported empty update passed.
- Repository formatting, generated-type check, frontend checks/lint and the root
  Rust workspace check/Clippy passed. Final targeted Clippy also passed.
- Aggregate `pnpm run check` and `pnpm run lint` cannot complete the separate
  remote Rust workspace: the container cannot obtain its existing private
  `BloopAI/vibe-kanban-private` billing dependency (SSH is unavailable). This is
  not waived or reported as a successful full-repository quality gate.

No paid Codex generation, upstream modification, release, push or automatic
repair of an older unjournalled maintenance run was performed. Windows-specific
link/permission behaviour still requires a platform smoke test. Inspection is
bounded (32 KiB per instruction file, the shared store's 128 KiB journal limit,
2 MiB per public page, and 20,000 Wiki entries); oversized input fails closed.
