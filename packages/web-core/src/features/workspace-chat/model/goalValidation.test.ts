import { describe, expect, it } from 'vitest';
import {
  ExecutionMode,
  BaseCodingAgent,
  type ExecutorConfig,
} from 'shared/types';
import { goalPromptError } from './goalValidation';
describe('Goal objective validation', () => {
  const config = {
    executor: BaseCodingAgent.CODEX,
    execution_mode: ExecutionMode.goal,
  } as ExecutorConfig;
  it('accepts 4000 Unicode characters and rejects 4001', () => {
    expect(goalPromptError('😀'.repeat(4000), config)).toBeNull();
    expect(goalPromptError('あ'.repeat(4001), config)).toContain('4001');
  });
  it('does not limit ordinary code, planning, other providers or resume commands', () => {
    for (const execution_mode of [
      ExecutionMode.code,
      ExecutionMode.plan,
      ExecutionMode.plan_with_goal,
    ]) {
      expect(
        goalPromptError('a'.repeat(5000), { ...config, execution_mode })
      ).toBeNull();
    }
    expect(
      goalPromptError('a'.repeat(5000), {
        ...config,
        executor: BaseCodingAgent.CLAUDE_CODE,
      })
    ).toBeNull();
    expect(goalPromptError('/goal resume', config)).toBeNull();
  });
  it('counts the adapter concurrency instructions too', () => {
    expect(
      goalPromptError('a'.repeat(4000), {
        ...config,
        goal_max_concurrent_agents: 3,
      })
    ).toContain('maximum is 4000');
    expect(
      goalPromptError('a'.repeat(3800), {
        ...config,
        goal_max_concurrent_agents: 65535,
      })
    ).toBeNull();
  });
});
