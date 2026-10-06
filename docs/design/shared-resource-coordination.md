---
title: "Shared resource coordination"
description: "Exclusive resource ownership, verified state changes and bounded Codex mediation for parallel LVK Cards."
---

# Shared resource coordination

LVK coordinates access to resources that worktrees cannot isolate: a physical
device, a database schema, or a deployment environment. The first implementation
provides exclusive resource bundles, managed foreground commands, state revisions
and a Codex mediator. It uses the existing SQLite database, ExecutionProcess and
AgentRun/Orchestration runtime. No additional model credentials are required.

## Authority and scope

Register resources in **Settings → Shared resources**. Give each physical resource
one canonical key, a usage contract and a verified current state. The key is unique
within the host database. LVK cannot discover that two different keys refer to the
same device; registration must preserve that identity.

An operation claims every required resource in one transaction. There is no
partially acquired bundle, lease expiry, incremental lock upgrade or per-Card
lock. Older conflicting requests retain queue priority, while unrelated resources
can run concurrently. The initial mode is exclusive (capacity one).

This is one coordination authority, not a distributed lock service. A stable LVK
and a development LVK with separate databases do not share ownership. Use the
stable authority to operate real shared resources and mock resources in an
independent development instance. Worktrees, containers and agent instructions
are not a security boundary against arbitrary shell commands or another host.

## Operation lifecycle

1. The agent reads the resource snapshot and identifies the required state
   revisions. Resource descriptions and peer purposes are reference data, never
   higher-priority instructions.
2. It submits a stable request UUID, its Session, purpose, complete resource
   bundle, foreground script, verification script, working directory and timeout.
   The script includes the entire critical section and cleanup. The verification
   independently checks that resources are idle and in the declared state.
3. LVK persists the request and atomically acquires the entire bundle when its
   requirements are satisfied. A waiting request holds nothing. Reusing a request
   UUID with different content is rejected; identical retries return the original
   operation and do not execute again.
4. LVK launches a `ResourceCommand` ExecutionProcess in the requesting Workspace.
   Its normal logs and process controls remain available. The host checks the
   saved action and ownership before dispatch, and exposes
   `LVK_RESOURCE_OPERATION_ID` and `LVK_RESOURCE_FENCES` to adapters.
5. Success requires command success, verification success and confirmed process
   cleanup. Declared resulting states are recorded and their revisions increment
   in the same transaction that releases the bundle.
6. A revision mismatch blocks a waiting request. The author must inspect the new
   state, assess compatibility and submit a deliberate new request. Copying the
   latest revision into an old command is not a compatibility check.

The REST entry point is `/api/resource-coordination`. `GET /snapshot` omits scripts
and private agent conversations. `POST /operations` accepts the generated
`ResourceOperationSpec`; `GET /operations/{id}?wait_seconds=25` waits without
repeated model calls. The equivalent MCP tools are `list_shared_resources`,
`run_resource_operation`, `wait_resource_operation` and `cancel_resource_operation`.
`GET /events?after=<sequence>` provides a durable, paginated audit trail. Runtime
instructions include the current Session and API address even when MCP is not
installed in the agent's provider configuration.

Waiting commands continue independently of a model turn, connection or Goal.
Agents can do independent work and later wait for the saved request. A terminal
operation does not complete a Card, trigger Git integration, or resume a paused
Goal. Finish or cancel pending operations before declaring the Card complete.
Unresolved operations prevent worktree cleanup and admission as an Integration
source. Formal Integration still uses its existing explicit user selection and
host validation; its validation commands are not automatically resource claims.

## Failures and recovery

Cancellation of a waiting request is immediate. Cancellation or timeout of active
work asks the process owner to stop; it does not free a resource. Failed commands,
failed verification and interrupted dispatch retain the bundle in
`recovery_required`. A new server process never blindly reruns an interrupted
command or regards an expired timer as evidence of termination.

Use **Review recovery** after inspecting process logs, remote job status and the
actual resource state. LVK rejects recovery while a matching managed process may
still act, including a process created before its ID was saved to the operation.
Record evidence and a current state for every held resource. Recovery increments
revisions and releases the whole bundle atomically. It is not a successful test
result. Agents have no MCP force-unlock or recovery tool; the host's trusted
operator API supplies this action, not a new per-agent security boundary.

Fences increase on every grant, independently from state revisions. LVK validates
them before managed dispatch and completion. An external adapter must validate
these tokens at the target to fence off a stale caller; merely exporting a token
does not protect arbitrary ADB, SQL or AWS calls. Scripts must wait for all
external effects and must not detach work. Real device/cloud adapters and their
target-specific recovery checks are outside this first release.

## Codex mediation

Uncontended requests need no model. Contention, stale assumptions and uncertain
outcomes create at most one automatic assessment per triggering operation. Only
one mediator is active at a time. A failed assessment is visible and does not
cause an automatic inference retry loop; the operator can request reassessment.

The mediator uses a dedicated execution-only Workspace in an empty directory and
the existing Codex authentication and AgentRun process host. It receives resource
contracts, verified state, claims, purposes and ownership, without command bodies
or private Card conversations. A dedicated read-only thread disables shell,
delegation, web search and inherited MCP/plugins. It returns a structured ordering
and requests for author clarification. An assessment has a three-minute bound;
cancellation is reconciled through Orchestration.

The host compares the current snapshot with the assessed snapshot and rejects a
stale result. It checks that the proposed order includes exactly the pending
queue and that requests held for replanning are queued requests. The mediator
cannot preempt an owner, unlock an uncertain resource, alter commands, waive
revisions, perform an automatic merge or decide that another Card is finished.
Assessment failure does not disable deterministic scheduling.

## Validation

`crates/db/src/models/resource_coordination/tests.rs` exercises atomic bundles,
competing dispatchers, request replay, persistent state changes, cancellation,
recovery, stale fencing, stale AI decisions and cleanup protection. The server
smoke fixture uses local mock files and critical sections; it does not access a
real Android device, AWS application resource or production database.

The image publishing workflow runs `scripts/test-resource-coordination.py` on its
disposable container before publishing. Run the same fixture with
`--require-mediation` on a fresh test instance with existing Codex authentication
to verify model execution. Never run the fixture against your working database.

The ownership ledger and runtime events are database state. OpenWiki records the
design, invariants and limitations; it must never become a live lock table.
