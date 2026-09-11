# Agent Runtime data planes and projection recovery

Agent Runtime separates provider output into four data planes. This prevents
display-oriented token deltas from turning SQLite into a second lossless audit
log.

| Plane | Source of truth | Contents | Recovery behavior |
| --- | --- | --- | --- |
| Audit | Native Audit `frames.jsonl` and manifest | Every raw provider frame, checksum, version metadata | Lossless replay input |
| Live | In-memory broadcast and AgentRun WebSocket `live` message | Message, reasoning, and tool-output deltas | May be lost on disconnect; a durable completion repairs the UI |
| Canonical | SQLite `agent_events` and `agent_run_state` | Completed messages, lifecycle, tool transitions, controls, errors, and session observations | Replayed after reconnect and used by product logic |
| Snapshot | SQLite `agent_run_usage_snapshots` plus latest-value state | Cumulative usage; rate-limit, diff, and provider status where a history is not required | Replaced rather than treated as a raw-frame history |

The provider adapter classifies each decoded native frame as durable semantic,
live-only, audit-only, or a required frame that cannot be interpreted. Known
Codex `item/agentMessage/delta` frames are live-only. The matching
`item/completed` frame creates one durable message with the same stable message
identity. Known ignored notifications and unknown optional notifications stay
in Native Audit and do not fall back to `ProviderExtension`.

Codex token-usage notifications contain cumulative totals. They are delivered
live and UPSERTed once per run attempt by native sequence; they are not summed
as canonical history. Reconnect emits the stored latest snapshot. Stats use the
snapshot for new attempts and retain the legacy event calculation for older
runs.

## Host batch transaction

One process-host attach response is normally applied in one SQLite transaction.
The transaction assigns dense canonical sequences, inserts semantic events in
order, reduces `RunState`, updates `agent_run_state`, `agent_runs`, and
`agent_run_attempts` once, and advances `last_host_event_sequence`. A rollback
therefore leaves both the canonical projection and host cursor unchanged.

`SQLITE_BUSY` and `SQLITE_LOCKED`, including extended codes, are temporary.
They receive bounded retries and do not mark the projection permanently
degraded. All normal database pools keep `journal_mode=DELETE` and use a finite
five-second busy timeout.

For a deterministic, non-retryable contract or reducer error, the runtime
retries the batch one host event at a time. Successful observations commit
normally. The exact failing observation is recorded in
`agent_projection_failures`; that record, the degraded status, and its host
cursor advance commit atomically. Later observations can continue. Database
infrastructure errors are never quarantined: their cursor remains unchanged so
the next attach can replay them. The Native Audit reference remains the evidence
needed for a future projection rebuild.

## UI convergence and emergency cancellation

The WebSocket protocol distinguishes `event` (durable) from `live` (ephemeral).
The UI buffers live message deltas by run attempt and provider message identity.
Duplicate live event IDs are ignored and native sequence orders out-of-order
delivery. When a completed durable message arrives, the buffer is discarded and
the authoritative full text replaces it. Reconnect starts from durable history;
an incomplete live message does not become history.

Cancellation is an emergency control-plane operation. It remains available for
`pending`, `starting`, `running`, `awaiting_input`, and `awaiting_approval` even
when projection is degraded. Input, approval, retry, and resume remain
fail-closed. Terminal and already-cancelling runs cannot be cancelled from the
UI; a backend cancel for an already terminal run is an idempotent no-op.

## Compatibility and future work

Existing canonical events, including historical delta events, are not deleted
or rewritten. The new compaction applies only to newly received frames. A
v2 mapper version labels the new semantics; the v1 mapper remains available to
replay previously written Native Audit bundles without relabeling their output.
A management operation that rebuilds a complete canonical projection from Native
Audit remains future work; `agent_projection_failures` provides the explicit
work list and reason ledger for that operation.
