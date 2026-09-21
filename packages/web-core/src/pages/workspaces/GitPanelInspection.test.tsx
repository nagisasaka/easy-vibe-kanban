import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { GitPanel } from '@vibe/ui/components/GitPanel';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe('Git inspection', () => {
  function render(readOnly: boolean) {
    return renderToStaticMarkup(
      createElement(GitPanel, {
        readOnly,
        repos: [
          {
            id: 'repo',
            name: 'Repository',
            targetBranch: 'test/target',
            commitsAhead: 1,
            commitsBehind: 0,
            showPushButton: true,
          },
        ],
        workingBranchName: 'execution/source',
        onWorkingBranchNameChange: vi.fn(),
      })
    );
  }
  it('retains branch metadata without editing, PR, target, or push controls', () => {
    const html = render(true);
    expect(html).toContain('Repository');
    expect(html).toContain('test/target');
    expect(html).toContain('execution/source');
    expect(html).not.toContain('<button');
    expect(html).not.toContain('<input');
    expect(html).not.toContain('Open pull request');
  });
  it('preserves ordinary Git controls', () => {
    expect(render(false)).toContain('Open pull request');
    expect(render(false)).toContain('<button');
  });
});
