import { describe, expect, it } from 'vitest';
import {
  BaseCodingAgent,
  ExecutionMode,
  type ExecutorConfig,
} from 'shared/types';
import { resolveExecutorOverride } from './executorOverrides';

describe('immutable Session launch restoration', () => {
  const launch: ExecutorConfig = { executor: BaseCodingAgent.CODEX };
  const editedPreset: ExecutorConfig = {
    ...launch,
    reasoning_id: 'low',
    execution_mode: ExecutionMode.plan,
  };
  it('does not replace inherited values with a subsequently edited preset', () => {
    expect(
      resolveExecutorOverride('reasoning_id', {}, null, launch, editedPreset)
    ).toBeUndefined();
    expect(
      resolveExecutorOverride('execution_mode', {}, null, launch, editedPreset)
    ).toBeUndefined();
    expect(
      resolveExecutorOverride('reasoning_id', {}, null, null, editedPreset)
    ).toBe('low');
  });
  it('preserves explicit null, max/ultra, zero concurrency and user/draft edits', () => {
    expect(
      resolveExecutorOverride(
        'reasoning_id',
        { reasoning_id: null },
        null,
        editedPreset,
        null
      )
    ).toBeNull();
    for (const reasoning_id of ['max', 'ultra']) {
      expect(
        resolveExecutorOverride(
          'reasoning_id',
          {},
          { ...launch, reasoning_id },
          editedPreset,
          null
        )
      ).toBe(reasoning_id);
    }
    expect(
      resolveExecutorOverride(
        'goal_max_concurrent_agents',
        {},
        null,
        { ...launch, goal_max_concurrent_agents: 0 },
        null
      )
    ).toBe(0);
  });
});
