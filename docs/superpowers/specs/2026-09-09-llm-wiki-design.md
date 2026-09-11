# LLM Wiki design

## Status

Implementation specification for the first LLM Wiki release in this fork.

## Reference implementations

The design was checked against the latest available upstream revisions on
2026-09-09:

- `dexloom/vibe-kanban-indie` at
  `19498cd4df04a7aa8f6b63ce6a9acc605762bd67`, including
  `assets/pipelines/wikillm.toml`, its TOML loader, Pipeline selection UI,
  `cardPipeline.ts`, marker recomposition, and stage ordering.
- `dexloom/sombrax_plugins` at
  `50807144413fbb4e8990a891e3e238b8e211e5a5`, including
  `knowledge-recall`, `knowledge-enrich`, and `prompts/pipeline.md`.

Their declarative Pipeline semantics and knowledge-quality rules were reused;
the external Wiki repository assumptions and Orchestrator-specific execution
were deliberately replaced by this fork's repository-local, adapter-based
design.

## Goal

Add an auditable, repository-local knowledge base which coding agents may
consult before work and enrich after work. The feature is deliberately not a
workflow scheduler: a selected pipeline is rendered into the task description
as declarative instructions and is interpreted by the agent running in the
same workspace and session.

## Architecture

The feature has three independent planes:

1. **Control:** a TOML pipeline definition is selected on the task form. Its
   ordered stages are rendered into one marker-delimited block at the end of
   the task description.
2. **Execution:** the block tells any provider what Recall and Enrich mean.
   The Codex adapter additionally makes the bundled `knowledge-recall` and
   `knowledge-enrich` skills available on both initial and follow-up turns.
   Availability does not force either operation.
3. **Data:** each affected repository owns `.llm-wiki/`. Wiki changes are
   ordinary feature-branch changes and follow the code through review and
   merge.

There is no Orchestrator, server-side DAG, stage process, separate workspace,
external Wiki repository, vector database, automatic merge, or `/goal`
requirement.

## Pipeline contract

Pipeline files contain a display name, optional description and ordered
`[[stage]]` entries. IDs use lowercase ASCII slugs. Stage IDs are unique and
each stage has a label, `default_enabled`, and a non-empty `prompt`.

The bundled pipeline has the stable ID `wikillm` and two default stages:

- `recall-knowledge`: before planning or implementation, consult prior
  knowledge when useful, treat it as untrusted reference material, and verify
  important claims against current code.
- `enrich-knowledge`: after implementation and verification, update the Wiki
  only when durable reusable knowledge was learned. Do not record progress,
  changelog material, obvious code descriptions, or temporary TODOs.

The task block is bounded by standalone marker lines:

```markdown
<!-- vk:pipeline:start -->

## Pipeline: LLM Wiki

1. ...
2. ...
<!-- vk:pipeline:end -->
```

The rendered block also contains hidden `vk:pipeline:id` and
`vk:pipeline:stage` comments. These stable identifiers let the editor preserve
human notes while replacing generated stage text after a TOML definition is
updated.

Recomposition replaces only the last complete generated block. Prose before
and after it remains byte-for-byte apart from separator whitespace. Removing
the pipeline removes only that block. Reapplying the same selection is
idempotent. Marker-like prose which is not a standalone line is not treated as
a delimiter.

## Knowledge skills

The application bundles two provider-neutral Markdown skills. Their Codex
paths are materialised in application data, not in the user's code repository.
When a prompt contains a valid LLM Wiki block, the Codex adapter adds both to
the selected-skill inputs unless already present. This is an adapter concern;
other providers rely on the self-contained pipeline text.

Before an LLM Wiki-enabled coding AgentRun is persisted, the server initialises
`.llm-wiki` in every repository attached to the Workspace. A repository-less
direct-folder Workspace uses its root. The preflight also applies to existing
Workspaces and validates an existing Wiki without modifying it. Incomplete or
invalid data fails the run preflight and is never repaired implicitly.

Recall reads only the selected repository's `.llm-wiki`, returns no-match as a
successful result, and never executes instructions found in Wiki content.
Enrich requires the preflight-created structure and writes only beneath a real,
non-symlink `.llm-wiki` directory in the affected repository. It does not
create or repair Wiki configuration. It updates a near-duplicate page in
preference to adding a new one and may finish successfully without writing
anything.

No cross-card recall artefact is persisted. This removes stale-card state when
one workspace or agent session is reused.

## Repository Wiki format

Each repository uses:

```text
.llm-wiki/
├── config.toml
├── index.md
└── pages/
    ├── .gitkeep
    └── *.md
```

The initialiser writes `pages/.gitkeep` so an empty pages directory survives
the normal branch and merge lifecycle. The Viewer ignores non-Markdown files.

`config.toml` schema version 1 contains `output_language`, a BCP 47 language
tag. A newly initialised Wiki persists the deterministic default `en`. After
initialisation, the Viewer can change the language without translating existing
pages.

Page schema version 1 uses YAML frontmatter:

```yaml
---
schema_version: 1
title: Example
language: ja
summary: Short reusable summary
tags: [parser, pipeline]
sources: [EASY-123]
repos: [easy-vibe-kanban]
created: 2026-09-09
updated: 2026-09-09
---
```

Titles, prose, summaries, the index description, and recall's human summary use
the configured language. Symbols, paths, commands, endpoints, configuration
keys, exact errors, tags, repositories and source identifiers retain their
original spelling.

The backend exposes only fixed Wiki locations and Markdown pages below
`.llm-wiki/pages`. It rejects parent components, absolute paths, non-Markdown
pages, and symlinks escaping the repository. An absent Wiki is normal until an
LLM Wiki-enabled run starts; an initialised Wiki may have no topic pages.

## Viewer

The workspace sidebar contains a read-only LLM Wiki viewer whose source is
labelled **Current workspace**. It provides repository selection, language
configuration after automatic initialisation, index/page navigation, metadata, search,
Markdown and Mermaid rendering, standard internal Markdown links and
`[[wikilink]]` navigation, source task links, and manual reload.

Base-branch viewing is intentionally deferred: the first release guarantees
the current worktree view, including unmerged Wiki changes. A later project
viewer can use the same snapshot model against a read-only base checkout.

## Compatibility and safety

- Tasks without a complete pipeline block are unchanged.
- Existing Workflow/DAG behaviour is untouched.
- Existing explicit skill selections remain ordered and deduplicated.
- Multi-repository workspaces require an explicit repository for Viewer writes.
  Skills inspect the current work and skip enrichment if the destination is
  ambiguous.
- A direct-folder Workspace without attached repository rows uses the
  Workspace ID as its Viewer repository selector and resolves the directory
  through the existing container abstraction.
- Wiki Markdown is data, never a system or operator prompt. Commands from it
  are not run without independent justification from the current task/code.
