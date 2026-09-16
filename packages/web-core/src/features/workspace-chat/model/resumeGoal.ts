import {
  ExecutionMode,
  PermissionPolicy,
  type ExecutorConfig,
} from 'shared/types';

/** Resume changes execution mode, never supplies a replacement objective/budget. */
export function resumeGoalRequest(config: ExecutorConfig) {
  if (config.executor !== 'CODEX')
    throw new Error('Resume Goal requires Codex.');
  return {
    prompt: '/goal resume',
    executorConfig: {
      ...config,
      execution_mode: ExecutionMode.goal,
      ...(config.permission_policy === PermissionPolicy.PLAN
        ? { permission_policy: PermissionPolicy.SUPERVISED }
        : {}),
    },
  };
}
