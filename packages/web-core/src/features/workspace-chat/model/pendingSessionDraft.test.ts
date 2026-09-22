import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DraftFollowUpData } from 'shared/types';
import {
  acknowledgePendingSessionDraft,
  readPendingSessionDraft,
  writePendingSessionDraft,
} from './pendingSessionDraft';

describe('unacknowledged tab-local Session draft', () => {
  let values: Map<string, string>;
  const scope = JSON.stringify(['local', 'host-a', 'session-a']);
  const data: DraftFollowUpData = {
    message: '日本語の未保存入力',
    executor_config: {
      executor: 'CODEX',
      execution_mode: 'goal',
      reasoning_id: 'max',
      goal_max_concurrent_agents: 0,
    },
  };
  beforeEach(() => {
    values = new Map();
    vi.stubGlobal('window', {
      sessionStorage: {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => values.set(key, value),
        removeItem: (key: string) => values.delete(key),
      },
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it('roundtrips the full configuration and isolates runtime, Host and Session', () => {
    writePendingSessionDraft(scope, data);
    expect(readPendingSessionDraft(scope)?.data).toEqual(data);
    for (const other of [
      ['remote', 'host-a', 'session-a'],
      ['local', 'host-b', 'session-a'],
      ['local', 'host-a', 'session-b'],
    ])
      expect(readPendingSessionDraft(JSON.stringify(other))).toBeNull();
  });

  it('a late acknowledgment cannot remove a newer edit, even with identical text', () => {
    const old = writePendingSessionDraft(scope, data);
    const next = writePendingSessionDraft(scope, data);
    expect(next).not.toBe(old);
    acknowledgePendingSessionDraft(scope, old);
    expect(readPendingSessionDraft(scope)?.raw).toBe(next);
    acknowledgePendingSessionDraft(scope, next);
    expect(readPendingSessionDraft(scope)).toBeNull();
  });

  it('invalid or incompatible storage never becomes an editor value', () => {
    for (const raw of [
      'not json',
      'null',
      '{}',
      '{"version":2}',
      JSON.stringify({ version: 1, nonce: 'id', data: { message: 'text' } }),
    ]) {
      values.set(`vibe.pendingSessionDraft.v1.${scope}`, raw);
      expect(readPendingSessionDraft(scope)).toBeNull();
    }
  });

  it('storage failure leaves the existing backup untouched', () => {
    const saved = writePendingSessionDraft(scope, data);
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('quota exceeded');
    });
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(
      writePendingSessionDraft(scope, { ...data, message: 'new' })
    ).toBeUndefined();
    acknowledgePendingSessionDraft(scope, undefined);
    expect(readPendingSessionDraft(scope)?.raw).toBe(saved);
    log.mockRestore();
  });
});
