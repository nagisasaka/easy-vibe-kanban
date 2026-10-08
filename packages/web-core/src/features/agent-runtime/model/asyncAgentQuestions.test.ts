import { describe, expect, it, vi } from 'vitest';
import type { AgentEventEnvelope, JsonValue } from 'shared/types';
import {
  collectAsyncAgentQuestions,
  deliverAsyncQuestionAnswer,
  formatAsyncQuestionAnswer,
} from './asyncAgentQuestions';

function event(payload: JsonValue, attempt = 'attempt-1'): AgentEventEnvelope {
  return {
    schema_version: 1,
    payload_version: 1,
    event_id: `questions-${attempt}`,
    session_id: 'session',
    agent_run_id: 'run',
    turn_id: 'turn',
    run_attempt_id: attempt,
    run_attempt_number: 1,
    sequence: 1n,
    correlation_id: 'correlation',
    timestamp: '2026-10-08T00:00:00Z',
    native_refs: [],
    payload: {
      type: 'provider_extension',
      data: {
        provider_namespace: 'codex',
        provider_event: 'async_questions',
        payload,
      },
    },
  };
}

describe('asynchronous questions', () => {
  it('steers a running agent, follows up after completion, and never retries an ambiguous delivery', async () => {
    const question = { id: 'q', title: 'Which colour?', options: [] };
    const steer = vi.fn().mockResolvedValue({});
    const followUp = vi.fn().mockResolvedValue({});
    expect(
      await deliverAsyncQuestionAnswer(question, 'Blue', {
        activeRunId: 'running',
        steer,
        followUp,
      })
    ).toBe(true);
    expect(steer).toHaveBeenCalledWith(
      'running',
      'Question: Which colour?\nAnswer: Blue'
    );
    expect(followUp).not.toHaveBeenCalled();
    steer.mockRejectedValueOnce(new Error('connection lost'));
    await expect(
      deliverAsyncQuestionAnswer(question, 'Blue', {
        activeRunId: 'running',
        steer,
        followUp,
      })
    ).rejects.toThrow('connection lost');
    expect(followUp).not.toHaveBeenCalled();
    expect(
      await deliverAsyncQuestionAnswer(question, 'Green', {
        activeRunId: null,
        steer,
        followUp,
      })
    ).toBe(true);
    followUp.mockResolvedValueOnce(null);
    expect(
      await deliverAsyncQuestionAnswer(question, 'Green', {
        activeRunId: null,
        steer,
        followUp,
      })
    ).toBe(false);
  });
  it('retains multiple questions and free text across runs without merging distinct message identities', () => {
    const input = {
      schema_version: 1,
      message_id: 'message',
      questions: [
        { title: 'Which colour?', options: ['Blue', 'Green'] },
        { title: 'Any constraints?', options: null },
      ],
    };
    const first = event(input);
    const questions = collectAsyncAgentQuestions([
      first,
      first,
      event(input, 'attempt-2'),
    ]);
    expect(questions).toHaveLength(4);
    expect(new Set(questions.map((q) => q.id)).size).toBe(4);
    expect(questions[1].options).toEqual([]);
    expect(formatAsyncQuestionAnswer(questions[0], ' Green ')).toBe(
      'Question: Which colour?\nAnswer: Green'
    );
  });

  it('ignores unrelated and malformed optional metadata', () => {
    const values: JsonValue[] = [
      null,
      [],
      {},
      { schema_version: 2, message_id: 'm', questions: [] },
      {
        schema_version: 1,
        message_id: 'm',
        questions: [null, {}, { title: ' ' }, { title: 3 }],
      },
    ];
    expect(
      collectAsyncAgentQuestions(values.map((value) => event(value)))
    ).toEqual([]);
    const other = event({
      schema_version: 1,
      message_id: 'm',
      questions: [{ title: 'Hidden' }],
    });
    if (other.payload.type === 'provider_extension')
      other.payload.data.provider_namespace = 'other-provider';
    expect(collectAsyncAgentQuestions([other])).toEqual([]);
  });
});
