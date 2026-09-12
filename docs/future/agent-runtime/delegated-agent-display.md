---
title: "Delegated agent display and Codex compatibility"
description: "Separate child activity from parent control and preserve readable execution history."
---

## Design

Native Audit retains unmodified app-server frames. A provider adapter maintains
per-stream thread scope, learned from the thread start or resume response.
Delegated activity is persisted as `agent_activity`, with thread ID, optional
parent ID and path, observation kind and optional content. Missing ancestry is
shown as unknown rather than assigned to the primary agent.

Only the primary thread control response binds the resumable provider session.
`thread/started` notifications, including child notifications and notifications
received before that response, do not change this identity.

The reducer does not use delegated activity to change parent status, Goal,
usage or terminal output. Child deltas remain in Audit; completed commentary,
answers and tool observations update the child card. Parent streaming remains
live. No database table migration is required: existing event JSON is retained.
Codex mapper v3 uses thread scope; v1 and v2 replay mappers remain available.

## Display

You see primary-agent messages in the normal conversation. Expand a delegated
agent card to read its completed reports and tool observations. The card header
shows the parent thread, agent path or ID, and latest observed state. Repeated
work on the same child stays in the same card within the Run. Grandchildren
have separate cards with their immediate parent identifier.

Child observations do not split an interleaved primary-agent message. Live
chunks are appended as deltas, including repeated text. A generic child
completion notification does not override an observed failure or interruption;
a subsequent running turn can reset that state.

Old Canonical history without ancestry is not automatically relabelled. Native
Audit re-projection is a separate future operation. Child token-by-token display
and a graphical agent tree are outside this implementation.

## Control and skills

Resume decodes required thread identity and model, not historical display
items. Turn completion validates ID and terminal status independently of item
history. Unknown control states still fail closed; unknown historical items do
not block resume. Raw responses remain available in Audit.

Goal activation supplies selected Skill references through thread developer
context before starting or resuming the thread. Existing developer instructions
are preserved. This introduces no extra inference turn and does not enlarge the
Goal objective. Skill availability does not require execution. Ordinary chat
continues to use native Skill inputs.

Completed Canonical messages settle live deltas regardless of commentary versus
final-answer phase. These concepts are not interchangeable.

## Validation plan

Cover unknown history variants, required control fields, initial and resumed
Skill context, child and unknown-thread isolation, nested ancestry, replay,
parent reducer state, grouped child UI and commentary completion. Keep existing
Goal lifecycle and Native Audit regression tests.
