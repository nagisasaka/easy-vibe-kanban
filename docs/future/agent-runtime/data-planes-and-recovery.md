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

## Bounded process-host replay (protocol 2)

New hosts keep attempt/host-scoped replay evidence under
`runtime/host-events/<attempt>-<host>.v1.jsonl` in the asset directory. This is
not another raw audit: it retains semantic events, usage snapshots, lifecycle
evidence and Native Audit references, but strips transient text deltas. Existing
Native Audit raw-frame writes and checksums are unchanged.

The host groups up to 128 events or roughly 256 KiB and flushes every 50 ms;
Started/Terminal flush immediately. Only a successful sync makes its cursor
visible. A Live ring is limited to 256 events / 4 MiB. The projection queue has
64 slots and an 8 MiB byte budget. Pages have at most 128 events / 4 MiB, with an
explicit singleton exception for a semantic event up to 8 MiB. Larger semantic
events fail closed with a diagnostic; their received raw frame remains audited.
Individual provider frame decoding still allocates a complete frame; these are
backlog bounds, not a guarantee of constant process memory. The seek index uses
one offset per committed batch and grows with run length. Journal retention
follows runtime evidence retention; no automatic deletion policy is introduced.

Authenticated Subscribe connections repair from the committed database cursor
after disconnect/timeout. Heartbeats are 10 seconds, read timeout 30 seconds,
write timeout 10 seconds. Attach remains page-bounded. A slow reader may lose
old Live deltas, never the completed semantic message. The journal is an ordered
transport replay source; SQLite remains the canonical product state store.

Recovery requires repeated transport failure **and confirmed host absence**.
On Linux, boot ID and process start time distinguish PID reuse; older/other
platform records use conservative presence checks. A missing PID, permission
error or live/unknown process is not proof of death. Recovery validates the
whole journal (identity, sequence, checksum, complete vs unfinished tail) and
its Native Audit references before projection, and syncs complete records before
advancing the database cursor. It never replacement-spawns a provider or retries
an unacknowledged non-idempotent control command. Provider/group presence is
checked separately; a dead host with a live child remains active/cancellable.

Only a Host Terminal with closed, verified Audit proves successful completion.
Provider turn-complete notifications no longer close the canonical run ahead of
Audit/process finalization. An incomplete journal tail may be discarded as
uncommitted evidence, but cannot prove success. Corruption/gaps/foreign identity
fail closed with a durable projection-degraded diagnostic and no cursor advance.
Emergency audited Cancel remains available. Old protocol hosts reconnect with
Attach, never with guessed journal paths; unknown versions are diagnosed.
Canonical completion, process exit, owner cleanup and OpenWiki's operation
completion proof remain separate contracts.

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
