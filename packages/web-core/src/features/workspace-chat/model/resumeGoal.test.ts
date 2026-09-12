import { describe, expect, it } from 'vitest';
import {
  ExecutionMode,
  PermissionPolicy,
  type ExecutorConfig,
} from 'shared/types';
import { resumeGoalRequest } from './resumeGoal';

describe('saved Goal resume', () => {
  const config = {
    executor: 'CODEX',
    execution_mode: ExecutionMode.code,
    permission_policy: PermissionPolicy.AUTO,
    goal_token_budget: 1234,
  } as ExecutorConfig;
  it('sends an explicit resume command, not a replacement objective', () => {
    const request = resumeGoalRequest(config);
    expect(request.prompt).toBe('/goal resume');
    expect(request.executorConfig.execution_mode).toBe(ExecutionMode.goal);
    expect(request.executorConfig.permission_policy).toBe(
      PermissionPolicy.AUTO
    );
    expect(config.execution_mode).toBe(ExecutionMode.code);
  });
  it('does not retain legacy plan-only permissions', () => {
    expect(
      resumeGoalRequest({ ...config, permission_policy: PermissionPolicy.PLAN })
        .executorConfig.permission_policy
    ).toBe(PermissionPolicy.SUPERVISED);
  });
  it('rejects other providers', () => {
    expect(() =>
      resumeGoalRequest({ ...config, executor: 'CLAUDE_CODE' })
    ).toThrow('Codex');
  });
});
