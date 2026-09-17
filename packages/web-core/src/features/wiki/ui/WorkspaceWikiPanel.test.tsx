import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { WorkspaceWikiPanel } from './WorkspaceWikiPanel';
import type { Workspace } from 'shared/types';

vi.mock('@/shared/hooks/useTheme', () => ({
  useTheme: () => ({ theme: 'light' }),
  getResolvedTheme: () => 'light',
}));
vi.mock('@/shared/components/MarkdownPreview', () => ({
  MarkdownPreview: () => null,
}));

afterEach(() => vi.unstubAllGlobals());

describe('OpenWiki-only Viewer', () => {
  const workspace = {
    id: 'workspace-one',
    workspace_kind: 'direct_folder',
    name: 'Current repo',
  } as Workspace;
  it('always displays read-only OpenWiki with no legacy controls', () => {
    const html = renderToStaticMarkup(
      createElement(WorkspaceWikiPanel, { workspace, repos: [] })
    );
    expect(html).not.toContain('Wiki format');
    expect(html).not.toContain('.llm-wiki/');
    expect(html).toContain('openwiki/');
    expect(html).toContain('Current workspace');
  });
  it('ignores obsolete format storage on reload', () => {
    const getItem = vi.fn((key: string) =>
      key === 'evk-wiki-viewer-format:workspace-one' ? 'llm-wiki' : null
    );
    vi.stubGlobal('sessionStorage', { getItem });
    const html = renderToStaticMarkup(
      createElement(WorkspaceWikiPanel, { workspace, repos: [] })
    );
    expect(getItem).not.toHaveBeenCalled();
    expect(html).toContain('openwiki/');
    expect(html).toContain('including unpublished changes');
    expect(html).not.toContain('Wiki output language');
    expect(html).not.toContain('Create Wiki from existing code');
    const other = renderToStaticMarkup(
      createElement(WorkspaceWikiPanel, {
        workspace: { ...workspace, id: 'other' },
        repos: [],
      })
    );
    expect(other).toContain('openwiki/');
    expect(other).not.toContain('.llm-wiki/');
  });
});
