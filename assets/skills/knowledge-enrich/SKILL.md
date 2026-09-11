---
name: knowledge-enrich
description: Add durable, reusable knowledge to the affected repository's LLM Wiki after verified work.
---

# Knowledge enrichment

Use this skill only after implementation and verification, and only if the
work produced durable knowledge that will help future tasks. "No reusable
knowledge to record" is a successful outcome.

Do not record changelog material, obvious code descriptions, card-specific
progress, temporary TODOs, secrets, credentials, personal data, speculation,
or claims not verified against the final code and tests.

1. Identify the affected repository from the current task and diff. For a
   multi-repository workspace, write to each clearly affected repository's own
   `.llm-wiki`; if a destination is ambiguous, skip it and report why.
2. Never infer repository location from `~/.vibe-kanban/worktrees`. Resolve
   from the current workspace roots. Refuse parent traversal and absolute page
   paths. Do not follow a `.llm-wiki` or `pages` symlink, and never read or
   write outside the selected repository's `.llm-wiki`.
3. The application initialises the Wiki before an LLM Wiki-enabled agent run.
   Require this exact structure in the selected repository:

   ```text
   .llm-wiki/config.toml
   .llm-wiki/index.md
   .llm-wiki/pages/
   ```

   If it is absent, incomplete, or invalid, report the problem and skip
   enrichment. Do not create or repair the layout or `config.toml`. Read the
   schema version 1 BCP 47 `output_language` already persisted by the
   application, and do not switch language based on the conversation.
4. Search existing index, titles, summaries, tags and bodies for a near
   duplicate. Update the closest page instead of creating overlapping pages.
5. Page files must be lowercase ASCII slugs ending in `.md`, inside `pages/`,
   and begin with this YAML frontmatter schema:

   ```yaml
   ---
   schema_version: 1
   title: ...
   language: ja
   summary: ...
   tags:
     - stable-english-identifier
   sources:
     - EASY-123
   repos:
     - repository-name
   created: YYYY-MM-DD
   updated: YYYY-MM-DD
   ---
   ```

6. Write the title, summary, body, and index description in the configured
   output language. Preserve symbols, paths, commands, API endpoints,
   configuration keys, identifiers, and exact errors verbatim. Keep useful
   English code identifiers in `tags`, `repos`, and `sources` so search works
   across languages.
7. Add or update an entry in `.llm-wiki/index.md` using a relative Markdown
   link such as `[Title](pages/topic.md)`. Record the current source task or
   issue when it is known. Preserve unrelated index content.
8. Review only the Wiki diff for duplication, path safety, language, secrets,
   source traceability, and factual agreement with the final implementation.
   Leave the Wiki changes uncommitted or committed exactly like the code; do
   not create a mandatory separate Wiki commit or repository.

Changing `output_language` does not authorise automatic translation of
existing pages.
