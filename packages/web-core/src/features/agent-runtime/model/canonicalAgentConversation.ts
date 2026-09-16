import type {
  AgentEventEnvelope,
  NormalizedEntry,
  PatchType,
  ToolStatus,
} from 'shared/types';
import { AgentRuntimeToolStatus } from 'shared/types';

import type {
  CanonicalConversationIdentity,
  CanonicalConversationProjection,
  PatchTypeWithKey,
} from '@/shared/hooks/useConversationHistory/types';
import {
  isCanonicalRunActive,
  type CanonicalAgentSessionRun,
  type CanonicalAgentSessionTimeline,
} from './canonicalAgentSessionTimeline';

function numericTokenCount(value: number | bigint): number {
  const count = typeof value === 'bigint' ? Number(value) : value;
  return Number.isSafeInteger(count) ? count : Number.MAX_SAFE_INTEGER;
}

function displaySequence(event: AgentEventEnvelope): bigint {
  const nativeSequence = event.native_refs?.at(-1)?.sequence;
  const sequence = nativeSequence ?? event.sequence;
  return typeof sequence === 'bigint' ? sequence : BigInt(sequence);
}

function displayEventSort(
  a: AgentEventEnvelope,
  b: AgentEventEnvelope
): number {
  if (a.run_attempt_number !== b.run_attempt_number) {
    return a.run_attempt_number - b.run_attempt_number;
  }
  const aSequence = displaySequence(a);
  const bSequence = displaySequence(b);
  return aSequence < bSequence ? -1 : aSequence > bSequence ? 1 : 0;
}

function identityFromEvents(
  events: readonly AgentEventEnvelope[],
  active: boolean
): CanonicalConversationIdentity {
  const first = events[0];
  const latest = events.at(-1) ?? first;
  return {
    agentRunId: latest.agent_run_id,
    runAttemptId: latest.run_attempt_id,
    runAttemptNumber: latest.run_attempt_number,
    eventId: latest.event_id,
    eventIds: events.map((event) => event.event_id),
    sequence:
      typeof latest.sequence === 'bigint'
        ? latest.sequence
        : BigInt(latest.sequence),
    active,
  };
}

function normalizedPatch(
  event: AgentEventEnvelope,
  entry: NormalizedEntry,
  active = false
): PatchTypeWithKey {
  return {
    type: 'NORMALIZED_ENTRY',
    content: entry,
    patchKey: `agent-event:${event.event_id}`,
    canonical: identityFromEvents([event], active),
  };
}

function toolStatus(
  status: AgentRuntimeToolStatus,
  approvalId?: string
): ToolStatus {
  switch (status) {
    case AgentRuntimeToolStatus.created:
    case AgentRuntimeToolStatus.running:
    case AgentRuntimeToolStatus.approved:
      return { status: 'created' };
    case AgentRuntimeToolStatus.waiting_approval:
      return approvalId
        ? { status: 'pending_approval', approval_id: approvalId }
        : { status: 'created' };
    case AgentRuntimeToolStatus.denied:
      return { status: 'denied', reason: null };
    case AgentRuntimeToolStatus.succeeded:
      return { status: 'success' };
    case AgentRuntimeToolStatus.failed:
      return { status: 'failed' };
    case AgentRuntimeToolStatus.timed_out:
      return { status: 'timed_out' };
  }
}

function toolIsActive(status: AgentRuntimeToolStatus, runActive: boolean) {
  return (
    runActive &&
    (status === AgentRuntimeToolStatus.created ||
      status === AgentRuntimeToolStatus.running ||
      status === AgentRuntimeToolStatus.waiting_approval)
  );
}

function toolEntry(
  event: AgentEventEnvelope,
  status: AgentRuntimeToolStatus,
  toolName: string,
  runActive: boolean,
  approvalId?: string,
  argumentsValue: Extract<
    AgentEventEnvelope['payload'],
    { type: 'tool_call' }
  >['data']['arguments'] = null,
  resultValue: Extract<
    AgentEventEnvelope['payload'],
    { type: 'tool_call' }
  >['data']['result'] = null
): PatchTypeWithKey {
  const result =
    resultValue == null
      ? null
      : { type: { type: 'json' as const }, value: resultValue };
  return normalizedPatch(
    event,
    {
      entry_type: {
        type: 'tool_use',
        tool_name: toolName,
        action_type: {
          action: 'tool',
          tool_name: toolName,
          arguments: argumentsValue ?? null,
          result,
        },
        status: toolStatus(status, approvalId),
      },
      content:
        resultValue == null
          ? toolName
          : typeof resultValue === 'string'
            ? resultValue
            : JSON.stringify(resultValue, null, 2),
      timestamp: event.timestamp,
    },
    toolIsActive(status, runActive)
  );
}

function mergeEventIdentity(
  entry: PatchTypeWithKey,
  event: AgentEventEnvelope,
  active: boolean
) {
  const previousIds = entry.canonical?.eventIds ?? [];
  entry.canonical = identityFromEvents(
    [
      ...previousIds.map(
        (eventId) => ({ ...event, event_id: eventId }) as AgentEventEnvelope
      ),
      event,
    ],
    active
  );
}

function appendRunEntries(
  run: CanonicalAgentSessionRun,
  output: PatchTypeWithKey[]
) {
  const events = run.timeline
    ? [...run.timeline.events, ...run.timeline.transientEvents].sort(
        displayEventSort
      )
    : [];
  const state = run.timeline?.state ?? run.summary.state;
  const runActive = isCanonicalRunActive(state);
  const liveIds = new Set(
    run.timeline?.transientEvents.map((event) => event.event_id)
  );
  const toolIndexes = new Map<string, number>();
  const agentEntries = new Map<
    string,
    {
      index: number;
      content: string[];
      path: string;
      parent: string | null;
      status: ToolStatus;
    }
  >();
  const approvalIndexes = new Map<string, number>();
  const inputIndexes = new Map<string, number>();
  let previousMessage:
    | { index: number; messageId: string; role: string }
    | undefined;

  const resetMessageMerge = () => {
    previousMessage = undefined;
  };

  for (const event of events) {
    const payload = event.payload;
    switch (payload.type) {
      case 'agent_activity': {
        // Child observations can interleave with a parent's message deltas.
        // They belong to a separate card, not a new parent message boundary.
        const activity = payload.data.activity;
        let agent = agentEntries.get(activity.thread_id);
        if (!agent) {
          agent = {
            index: output.length,
            content: [],
            path: activity.agent_path ?? activity.thread_id,
            parent: activity.parent_thread_id,
            status: { status: 'created' },
          };
          agentEntries.set(activity.thread_id, agent);
          output.push(
            normalizedPatch(event, {
              entry_type: { type: 'system_message' },
              content: '',
              timestamp: event.timestamp,
            })
          );
        }
        if (activity.agent_path) agent.path = activity.agent_path;
        if (activity.parent_thread_id) agent.parent = activity.parent_thread_id;
        if (activity.content)
          agent.content.push(`### ${activity.kind}\n\n${activity.content}`);
        if (activity.kind === 'completed' && agent.status.status !== 'failed')
          agent.status = { status: 'success' };
        else if (['failed', 'interrupted'].includes(activity.kind))
          agent.status = { status: 'failed' };
        else if (['started', 'running'].includes(activity.kind))
          agent.status = { status: 'created' };
        const status = agent.status;
        const entry = normalizedPatch(
          event,
          {
            entry_type: {
              type: 'tool_use',
              tool_name: 'Agent',
              action_type: {
                action: 'task_create',
                description: `${agent.parent ?? '?'} → ${agent.path} · ${activity.kind === 'completed' && status.status === 'failed' ? 'failed' : activity.kind}`,
                subagent_type: agent.path,
                result: {
                  type: { type: 'markdown' },
                  value: agent.content.join('\n\n'),
                },
              },
              status,
            },
            content: agent.path,
            timestamp: event.timestamp,
          },
          runActive && status.status === 'created'
        );
        entry.patchKey = output[agent.index].patchKey;
        entry.canonical = output[agent.index].canonical;
        mergeEventIdentity(
          entry,
          event,
          runActive && status.status === 'created'
        );
        output[agent.index] = entry;
        break;
      }
      case 'message': {
        const message = payload.data.message;
        const entryType =
          message.role === 'user'
            ? 'user_message'
            : message.role === 'assistant'
              ? 'assistant_message'
              : 'system_message';
        const canMerge =
          message.role === 'assistant' &&
          previousMessage?.messageId === message.message_id &&
          previousMessage.role === message.role;
        if (canMerge && previousMessage) {
          const previous = output[previousMessage.index];
          if (previous.type === 'NORMALIZED_ENTRY') {
            const previousContent = previous.content.content;
            previous.content = {
              ...previous.content,
              content:
                !liveIds.has(event.event_id) &&
                message.content.startsWith(previousContent)
                  ? message.content
                  : `${previousContent}${message.content}`,
              timestamp: event.timestamp,
            };
            mergeEventIdentity(previous, event, runActive);
          }
          break;
        }
        output.push(
          normalizedPatch(event, {
            entry_type: { type: entryType },
            content: message.content,
            timestamp: event.timestamp,
          })
        );
        previousMessage = {
          index: output.length - 1,
          messageId: message.message_id,
          role: message.role,
        };
        break;
      }
      case 'thinking':
        resetMessageMerge();
        output.push(
          normalizedPatch(event, {
            entry_type: { type: 'thinking' },
            content: payload.data.content,
            timestamp: event.timestamp,
          })
        );
        break;
      case 'tool_call': {
        resetMessageMerge();
        const toolId = payload.data.tool_call_id ?? event.event_id;
        const existingIndex = toolIndexes.get(toolId);
        const next = toolEntry(
          event,
          payload.data.status,
          payload.data.tool_name,
          runActive,
          undefined,
          payload.data.arguments,
          payload.data.result
        );
        if (existingIndex === undefined) {
          output.push(next);
          toolIndexes.set(toolId, output.length - 1);
        } else {
          const previous = output[existingIndex];
          next.patchKey = previous.patchKey;
          next.canonical = identityFromEvents(
            [
              ...(previous.canonical?.eventIds ?? []).map(
                (eventId) =>
                  ({ ...event, event_id: eventId }) as AgentEventEnvelope
              ),
              event,
            ],
            next.canonical?.active ?? false
          );
          output[existingIndex] = next;
        }
        break;
      }
      case 'approval_requested': {
        resetMessageMerge();
        const existingIndex = payload.data.tool_call_id
          ? toolIndexes.get(payload.data.tool_call_id)
          : undefined;
        const next = toolEntry(
          event,
          AgentRuntimeToolStatus.waiting_approval,
          payload.data.tool_name,
          runActive,
          payload.data.approval_id
        );
        if (existingIndex === undefined) {
          output.push(next);
          approvalIndexes.set(payload.data.approval_id, output.length - 1);
        } else {
          const previous = output[existingIndex];
          next.patchKey = previous.patchKey;
          mergeEventIdentity(previous, event, runActive);
          next.canonical = previous.canonical;
          output[existingIndex] = next;
          approvalIndexes.set(payload.data.approval_id, existingIndex);
        }
        break;
      }
      case 'approval_resolved': {
        resetMessageMerge();
        const index = approvalIndexes.get(payload.data.approval_id);
        if (index !== undefined) {
          const previous = output[index];
          if (
            previous.type === 'NORMALIZED_ENTRY' &&
            previous.content.entry_type.type === 'tool_use'
          ) {
            previous.content = {
              ...previous.content,
              entry_type: {
                ...previous.content.entry_type,
                status: payload.data.approved
                  ? { status: 'created' }
                  : { status: 'denied', reason: payload.data.reason ?? null },
              },
              timestamp: event.timestamp,
            };
            mergeEventIdentity(
              previous,
              event,
              payload.data.approved && runActive
            );
          }
        }
        break;
      }
      case 'input_requested': {
        resetMessageMerge();
        output.push(
          normalizedPatch(
            event,
            {
              entry_type: {
                type: 'tool_use',
                tool_name: 'Agent input',
                action_type: {
                  action: 'ask_user_question',
                  questions: [
                    {
                      question: payload.data.prompt,
                      header: 'Agent input',
                      options: [],
                      multiSelect: false,
                    },
                  ],
                },
                status: {
                  status: 'pending_approval',
                  approval_id: payload.data.input_id,
                },
              },
              content: payload.data.prompt,
              timestamp: event.timestamp,
            },
            runActive
          )
        );
        inputIndexes.set(payload.data.input_id, output.length - 1);
        break;
      }
      case 'input_resolved': {
        resetMessageMerge();
        const index = inputIndexes.get(payload.data.input_id);
        if (index !== undefined) {
          const previous = output[index];
          if (
            previous.type === 'NORMALIZED_ENTRY' &&
            previous.content.entry_type.type === 'tool_use'
          ) {
            previous.content = {
              ...previous.content,
              entry_type: {
                ...previous.content.entry_type,
                status: payload.data.answered
                  ? { status: 'success' }
                  : { status: 'denied', reason: null },
              },
              timestamp: event.timestamp,
            };
            mergeEventIdentity(previous, event, false);
          }
        }
        break;
      }
      case 'token_usage':
        resetMessageMerge();
        output.push(
          normalizedPatch(event, {
            entry_type: {
              type: 'token_usage_info',
              total_tokens:
                numericTokenCount(payload.data.input_tokens) +
                numericTokenCount(payload.data.output_tokens),
              model_context_window: 0,
            },
            content: '',
            timestamp: event.timestamp,
          })
        );
        break;
      case 'error':
        resetMessageMerge();
        output.push(
          normalizedPatch(event, {
            entry_type: {
              type: 'error_message',
              error_type: { type: 'other' },
            },
            content: payload.data.error.message,
            timestamp: event.timestamp,
          })
        );
        break;
      case 'projection_degraded':
        resetMessageMerge();
        output.push(
          normalizedPatch(event, {
            entry_type: { type: 'system_message' },
            content: payload.data.reason,
            timestamp: event.timestamp,
          })
        );
        break;
      case 'session_observed':
        resetMessageMerge();
        output.push(
          normalizedPatch(event, {
            entry_type: { type: 'system_message' },
            content: `Agent session ${payload.data.provider_session.provider_session_id}`,
            timestamp: event.timestamp,
          })
        );
        break;
      case 'unknown':
        resetMessageMerge();
        output.push(
          normalizedPatch(event, {
            entry_type: { type: 'system_message' },
            content: `Unsupported agent event: ${payload.data.event_type}`,
            timestamp: event.timestamp,
          })
        );
        break;
      case 'lifecycle_changed':
      case 'provider_extension':
        resetMessageMerge();
        break;
      case 'goal_updated':
      case 'goal_cleared':
        // Goal progress has its own UI and must not split a streaming message.
        break;
    }
  }

  const hasPendingControl = output.some(
    (entry) =>
      entry.canonical?.agentRunId === run.summary.agent_run_id &&
      entry.type === 'NORMALIZED_ENTRY' &&
      entry.content.entry_type.type === 'tool_use' &&
      entry.content.entry_type.status.status === 'pending_approval'
  );
  if (runActive && !hasPendingControl) {
    const latestEvent = events.at(-1);
    if (latestEvent) {
      const loadingPatch: PatchType = {
        type: 'NORMALIZED_ENTRY',
        content: {
          entry_type: { type: 'loading' },
          content: '',
          timestamp: latestEvent.timestamp,
        },
      };
      output.push({
        ...loadingPatch,
        patchKey: `agent-run:${run.summary.agent_run_id}:loading`,
        canonical: identityFromEvents([latestEvent], true),
      });
    }
  }
}

export function projectCanonicalAgentConversation(
  timeline: CanonicalAgentSessionTimeline | null,
  isLoading = false
): CanonicalConversationProjection {
  if (!timeline) {
    return {
      entries: [],
      activeAgentRunIds: new Set(),
      runCount: 0,
      isLoading,
      isRunning: false,
      projectionDegraded: false,
      latestStatus: null,
    };
  }

  const entries: PatchTypeWithKey[] = [];
  for (const run of timeline.runs) appendRunEntries(run, entries);

  return {
    entries,
    activeAgentRunIds: new Set(
      timeline.runs
        .filter((run) =>
          isCanonicalRunActive(run.timeline?.state ?? run.summary.state)
        )
        .map((run) => run.summary.agent_run_id)
    ),
    runCount: timeline.runs.length,
    isLoading,
    isRunning: timeline.isRunning,
    projectionDegraded: timeline.projectionDegraded,
    latestStatus:
      timeline.latestRun?.timeline?.state?.status ??
      timeline.latestRun?.summary.state.status ??
      null,
  };
}
