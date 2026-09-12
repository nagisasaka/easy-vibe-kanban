import { ExecutionMode, type ExecutorConfig } from 'shared/types';

export const GOAL_OBJECTIVE_MAX_CHARS = 4000;

export function goalPromptError(
  prompt: string,
  config: ExecutorConfig
): string | null {
  if (
    config.executor !== 'CODEX' ||
    config.execution_mode !== ExecutionMode.goal ||
    prompt.trimStart().startsWith('/')
  )
    return null;
  // Keep this suffix aligned with codex::goal_concurrency_constraint. Validate
  // the actual objective, including adapter-added instructions, not only the draft.
  const limit = config.goal_max_concurrent_agents;
  const suffix =
    limit == null
      ? ''
      : limit === 0
        ? '\n\nExecution constraint: do not spawn or delegate to subagents for this goal.'
        : `\n\nExecution constraint: when work can be divided safely, use at most ${limit} concurrent spawned subagents (excluding the primary agent). Avoid concurrent writes to the same files.`;
  const count = Array.from(prompt + suffix).length;
  if (count > GOAL_OBJECTIVE_MAX_CHARS) {
    return `Codex Goal objective is ${count} characters; maximum is ${GOAL_OBJECTIVE_MAX_CHARS}. Shorten the instructions before sending.`;
  }
  return null;
}
