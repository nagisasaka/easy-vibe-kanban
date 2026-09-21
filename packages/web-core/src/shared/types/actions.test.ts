import { describe, expect, it } from 'vitest';
import {
  ActionTargetType,
  isActionEnabled,
  isActionVisible,
  type ActionDefinition,
  type ActionVisibilityContext,
} from './actions';

const context = { workspaceReadOnly: true } as ActionVisibilityContext;
function action(id: string, requiresTarget = ActionTargetType.WORKSPACE) {
  return {
    id,
    requiresTarget,
    isVisible: () => true,
    isEnabled: () => true,
  } as ActionDefinition;
}

describe('execution-only action policy', () => {
  it.each([
    'duplicate-workspace',
    'rename-workspace',
    'archive-workspace',
    'delete-workspace',
    'start-review',
    'spin-off-workspace',
    'run-setup-script',
    'run-cleanup-script',
    'run-archive-script',
    'future-write-operation',
  ])('blocks %s independently of the owner feature', (id) => {
    expect(isActionVisible(action(id), context)).toBe(false);
    expect(isActionEnabled(action(id), context)).toBe(false);
    expect(
      isActionEnabled(action(id), { ...context, workspaceReadOnly: false })
    ).toBe(true);
  });
  it.each([
    'git-merge',
    'git-rebase',
    'git-push',
    'git-create-pr',
    'git-change-target',
    'repo-open-in-ide',
  ])('blocks Git mutation %s', (id) =>
    expect(isActionEnabled(action(id, ActionTargetType.GIT), context)).toBe(
      false
    )
  );
  it.each(['open-in-ide', 'toggle-dev-server', 'toggle-preview-mode'])(
    'also blocks global workspace side effects: %s',
    (id) =>
      expect(isActionVisible(action(id, ActionTargetType.NONE), context)).toBe(
        false
      )
  );
  it.each([
    'toggle-files-mode',
    'toggle-wiki-mode',
    'toggle-logs-mode',
    'toggle-changes-mode',
    'copy-workspace-path',
    'new-workspace',
  ])('preserves inspection/navigation: %s', (id) =>
    expect(isActionEnabled(action(id, ActionTargetType.NONE), context)).toBe(
      true
    )
  );
  it('keeps repository path copy available', () =>
    expect(
      isActionEnabled(action('repo-copy-path', ActionTargetType.GIT), context)
    ).toBe(true));
});
