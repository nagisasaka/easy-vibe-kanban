import type {
  AgentEventEnvelope,
  AgentLiveEvent,
  RunState,
} from 'shared/types';
import { AgentGoalStatus } from 'shared/types';
import { describe, expect, it } from 'vitest';
import {
  emptyCanonicalAgentTimeline,
  clearAgentLiveEvents,
  isCanonicalAgentRunTerminal,
  mergeAgentLiveEvent,
  mergeCanonicalAgentTimeline,
} from './canonicalAgentTimeline';

const state: RunState = {
  state_schema_version: 1,
  reducer_version: 1,
  session_id: 'session',
  agent_run_id: 'run',
  turn_id: 'turn',
  status: 'running',
  projection_status: 'current',
  last_run_attempt_id: 'attempt',
  last_run_attempt_number: 1,
  last_event_sequence: 1,
  last_event_id: 'event-1',
  provider_session: null,
  terminal_output: null,
  last_error: null,
  unknown_event_count: 0,
  updated_at: '2026-08-12T00:00:00Z',
};

const event = (
  sequence: number,
  eventId = `event-${sequence}`
): AgentEventEnvelope => ({
  schema_version: 1,
  payload_version: 1,
  event_id: eventId,
  session_id: 'session',
  agent_run_id: 'run',
  turn_id: 'turn',
  run_attempt_id: 'attempt',
  run_attempt_number: 1,
  sequence,
  correlation_id: 'correlation',
  timestamp: '2026-08-12T00:00:00Z',
  native_refs: [],
  payload: {
    type: 'message',
    data: {
      message: {
        message_id: eventId,
        role: 'assistant',
        content: `message ${sequence}`,
      },
      final_output: sequence === 2,
    },
  },
});

describe('mergeCanonicalAgentTimeline', () => {
  it('repairs completed commentary even when it is not the final answer', () => {
    const live: AgentLiveEvent = {
      schema_version: 1,
      event_id: 'live',
      session_id: 'session',
      agent_run_id: 'run',
      turn_id: 'turn',
      run_attempt_id: 'attempt',
      run_attempt_number: 1,
      native_sequence: 1,
      timestamp: '2026-09-12T00:00:00Z',
      payload: {
        type: 'message_delta',
        data: {
          message_id: 'commentary',
          provider_item_id: 'c',
          role: 'assistant',
          delta: 'world',
        },
      },
    };
    const complete = event(2, 'commentary');
    if (complete.payload.type !== 'message') throw new Error('fixture');
    complete.payload.data.final_output = false;
    complete.payload.data.message.content = 'Hello world';
    const streamed = mergeAgentLiveEvent(emptyCanonicalAgentTimeline(), live);
    const repaired = mergeCanonicalAgentTimeline(streamed, [complete]);
    expect(repaired.transientEvents).toEqual([]);
    expect(
      mergeAgentLiveEvent(repaired, {
        ...live,
        event_id: 'late',
        native_sequence: 3,
      })
    ).toBe(repaired);
  });
  it('buffers ordered live deltas without advancing the durable cursor and repairs on completion', () => {
    const live = (sequence: number, delta: string): AgentLiveEvent => ({
      schema_version: 1,
      event_id: `live-${sequence}`,
      session_id: 'session',
      agent_run_id: 'run',
      turn_id: 'turn',
      run_attempt_id: 'attempt',
      run_attempt_number: 1,
      native_sequence: sequence,
      timestamp: '2026-08-12T00:00:00Z',
      payload: {
        type: 'message_delta',
        data: {
          message_id: 'message-1',
          provider_item_id: 'provider-message-1',
          role: 'assistant',
          delta,
        },
      },
    });
    const initial = mergeCanonicalAgentTimeline(
      emptyCanonicalAgentTimeline(),
      [event(1)],
      state
    );
    const streamed = mergeAgentLiveEvent(
      mergeAgentLiveEvent(initial, live(3, 'B')),
      live(2, 'A')
    );
    expect(streamed.cursor).toEqual({ run_attempt_number: 1, sequence: 1n });
    expect(streamed.transientEvents.map((entry) => entry.sequence)).toEqual([
      2, 3,
    ]);
    expect(
      mergeAgentLiveEvent(streamed, {
        ...live(4, 'wrong attempt'),
        run_attempt_id: 'other-attempt',
      })
    ).toBe(streamed);

    const completed = {
      ...event(2, 'completed'),
      payload: {
        type: 'message',
        data: {
          message: {
            message_id: 'message-1',
            role: 'assistant',
            content: 'AB',
          },
          final_output: true,
        },
      },
    } as AgentEventEnvelope;
    const repaired = mergeCanonicalAgentTimeline(streamed, [completed]);
    expect(repaired.transientEvents).toEqual([]);
    expect(mergeAgentLiveEvent(repaired, live(4, 'late'))).toBe(repaired);
  });

  it('replaces cumulative usage snapshots and discards live state for reconnect', () => {
    const usage = (sequence: number, inputTokens: number): AgentLiveEvent => ({
      schema_version: 1,
      event_id: `usage-${sequence}`,
      session_id: 'session',
      agent_run_id: 'run',
      turn_id: 'turn',
      run_attempt_id: 'attempt',
      run_attempt_number: 1,
      native_sequence: sequence,
      timestamp: '2026-08-12T00:00:00Z',
      payload: {
        type: 'token_usage_snapshot',
        data: {
          input_tokens: inputTokens,
          output_tokens: 2,
          cached_input_tokens: null,
        },
      },
    });
    const initial = mergeCanonicalAgentTimeline(
      emptyCanonicalAgentTimeline(),
      [event(1)],
      state
    );
    const latest = mergeAgentLiveEvent(
      mergeAgentLiveEvent(initial, usage(5, 10)),
      usage(6, 20)
    );
    expect(latest.events).toHaveLength(1);
    expect(latest.transientEvents).toHaveLength(1);
    expect(latest.transientEvents[0]?.payload).toMatchObject({
      type: 'token_usage',
      data: { input_tokens: 20 },
    });
    expect(mergeAgentLiveEvent(latest, usage(4, 5))).toBe(latest);
    expect(clearAgentLiveEvents(latest).transientEvents).toEqual([]);
  });

  it('replays and deduplicates by event identity while preserving cursor order', () => {
    const first = mergeCanonicalAgentTimeline(
      emptyCanonicalAgentTimeline(),
      [event(2), event(1)],
      state
    );
    const second = mergeCanonicalAgentTimeline(first, [
      event(1),
      event(2),
      event(3),
    ]);

    expect(second.events.map((entry) => entry.sequence)).toEqual([1, 2, 3]);
    expect(second.items.map((entry) => entry.content)).toEqual([
      'message 1',
      'message 2',
      'message 3',
    ]);
    expect(second.cursor).toEqual({ run_attempt_number: 1, sequence: 3n });
  });

  it('keeps unknown/degraded events visible as canonical items', () => {
    const degraded = {
      ...event(1, 'degraded'),
      payload: {
        type: 'unknown',
        data: { event_type: 'future_event', payload: {} },
      },
    } as AgentEventEnvelope;
    const timeline = mergeCanonicalAgentTimeline(
      emptyCanonicalAgentTimeline(),
      [degraded],
      {
        ...state,
        projection_status: 'projection_degraded',
        unknown_event_count: 1,
      }
    );

    expect(timeline.items[0]?.kind).toBe('unknown');
    expect(timeline.state?.projection_status).toBe('projection_degraded');
  });

  it('keeps goal updates durable and deduplicated across replay', () => {
    const goalUpdate = {
      ...event(2, 'goal-update'),
      payload: {
        type: 'goal_updated',
        data: {
          goal: {
            objective: 'Ship the feature',
            status: AgentGoalStatus.active,
            token_budget: 50_000n,
            tokens_used: 1_200n,
            time_used_seconds: 42n,
          },
        },
      },
    } as AgentEventEnvelope;
    const first = mergeCanonicalAgentTimeline(
      emptyCanonicalAgentTimeline(),
      [goalUpdate],
      { ...state, goal: goalUpdate.payload.data.goal }
    );
    const replayed = mergeCanonicalAgentTimeline(first, [goalUpdate]);

    expect(replayed.events).toHaveLength(1);
    expect(replayed.items[0]).toMatchObject({
      kind: 'goal',
      content: 'Ship the feature',
    });
    expect(replayed.state?.goal?.tokens_used).toBe(1_200n);
  });
});

describe('isCanonicalAgentRunTerminal', () => {
  it('recognizes crash and audit failure as terminal', () => {
    expect(isCanonicalAgentRunTerminal({ ...state, status: 'crashed' })).toBe(
      true
    );
    expect(
      isCanonicalAgentRunTerminal({ ...state, status: 'audit_failed' })
    ).toBe(true);
    expect(isCanonicalAgentRunTerminal(state)).toBe(false);
  });
});
