import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { WorkspaceWikiPanel } from './WorkspaceWikiPanel';
import type { Workspace } from 'shared/types';

vi.mock('@/shared/hooks/useTheme', () => ({
  useTheme: () => ({ theme: 'light' }),
  getResolvedTheme: () => 'light',
}));
vi.mock('./WikiBootstrapDialog', () => ({ WikiBootstrapDialog: () => null }));
vi.mock('@/shared/components/MarkdownPreview', () => ({
  MarkdownPreview: () => null,
}));

afterEach(() => vi.unstubAllGlobals());

describe('Wiki Viewer format selection', () => {
  const workspace = {
    id: 'workspace-one',
    workspace_kind: 'direct_folder',
    name: 'Current repo',
  } as Workspace;
  it('keeps legacy defaults and exposes the separate read-only OpenWiki choice', () => {
    const html = renderToStaticMarkup(
      createElement(WorkspaceWikiPanel, { workspace, repos: [] })
    );
    expect(html).toContain('value="llm-wiki" selected=""');
    expect(html).toContain('OpenWiki · openwiki/ (read-only)');
    expect(html).toContain('Current workspace');
  });
  it('restores the workspace-specific OpenWiki view after reload', () => {
    const getItem = vi.fn((key: string) =>
      key === 'evk-wiki-viewer-format:workspace-one' ? 'openwiki' : null
    );
    vi.stubGlobal('sessionStorage', { getItem });
    const html = renderToStaticMarkup(
      createElement(WorkspaceWikiPanel, { workspace, repos: [] })
    );
    expect(getItem).toHaveBeenCalledWith(
      'evk-wiki-viewer-format:workspace-one'
    );
    expect(html).toContain('value="openwiki" selected=""');
    expect(html).toContain('including unpublished changes');
    expect(html).not.toContain('Wiki output language');
    expect(html).not.toContain('Create Wiki from existing code');
    const other = renderToStaticMarkup(
      createElement(WorkspaceWikiPanel, {
        workspace: { ...workspace, id: 'other' },
        repos: [],
      })
    );
    expect(other).toContain('value="llm-wiki" selected=""');
  });
});
