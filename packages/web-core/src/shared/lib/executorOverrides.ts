import type { ExecutorConfig } from 'shared/types';

/** A stored launch is a complete snapshot, not a patch over today's preset.
 * In particular absent/null reasoning means provider inheritance, not "low"
 * or the possibly edited preset. A draft, in contrast, may be a partial edit.
 */
export function resolveExecutorOverride<K extends keyof ExecutorConfig>(
  field: K,
  selection: Partial<ExecutorConfig>,
  scratch: ExecutorConfig | null | undefined,
  launch: ExecutorConfig | null | undefined,
  preset: ExecutorConfig | null | undefined
): ExecutorConfig[K] | undefined {
  if (Object.hasOwn(selection, field)) return selection[field];
  if (scratch && Object.hasOwn(scratch, field)) return scratch[field];
  if (launch) return launch[field];
  return preset?.[field];
}
