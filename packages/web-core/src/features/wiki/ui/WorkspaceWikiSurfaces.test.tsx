import { createElement, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Workspace, WorkspaceWikiSnapshot } from 'shared/types';
import { createWikiViewerController } from '../model/wikiViewerController';
import {
  WorkspaceWikiArticle,
  WorkspaceWikiNavigation,
} from './WorkspaceWikiPanel';

const context = vi.hoisted(() => ({
  get: (): unknown => null,
}));
vi.mock('./WorkspaceWikiProvider', () => ({
  useWorkspaceWiki: () => context.get(),
  WorkspaceWikiProvider: ({ children }: { children: ReactNode }) => children,
}));
vi.mock('@/shared/hooks/useTheme', () => ({
  useTheme: () => ({ theme: 'light' }),
  getResolvedTheme: () => 'light',
}));

vi.mock('@/shared/components/MermaidDiagram', () => ({
  MermaidDiagram: ({ chart }: { chart: string }) =>
    createElement('div', { 'data-mermaid': true }, chart),
}));

async function fixture() {
  const workspace = {
    id: 'workspace',
    name: 'Reading workspace',
    branch: 'feature/wiki',
    workspace_kind: 'worktree',
  } as Workspace;
  const snapshot: WorkspaceWikiSnapshot = {
    workspace_id: workspace.id,
    repo_id: 'repo',
    repo_name: 'repo',
    repo_display_name: 'Repository One',
    source: 'current_workspace',
    source_links: [
      { source: 'EVK-1', project_id: 'project-one', issue_id: 'issue-one' },
    ],
    wiki: {
      exists: true,
      config: null,
      index: {
        path: 'index.md',
        content: '# Home\n[Job](concepts/job.md)',
        metadata: null,
      },
      pages: [
        {
          path: 'concepts/job.md',
          content:
            '# Job\nJob definition with [execution](execution.md).\n\n| Contract | Rule |\n| --- | --- |\n| identity | stable |\n\n```mermaid\ngraph TD; Job-->Execution\n```',
          metadata: {
            schema_version: 1,
            title: 'Job concept',
            summary: 'Job contract',
            language: 'ja',
            tags: ['Job'],
            sources: ['EVK-1'],
            repos: ['repo'],
            created: '',
            updated: '2026-09-16',
          },
        },
        {
          path: 'concepts/execution.md',
          content: '# Execution\nOne execution belongs to a job.',
          metadata: null,
        },
      ],
    },
  };
  const api = {
    snapshot: vi.fn().mockResolvedValue(snapshot),
  };
  const controller = createWikiViewerController(workspace.id, 'repo', api);
  await controller.reload();
  context.get = () => ({
    workspace,
    repositoryOptions: [{ id: 'repo', name: 'Repository One' }],
    controller,
    ...controller.getSnapshot(),
  });
  return { controller, api, snapshot };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('split Wiki navigation and article surfaces', () => {
  it('renders a directory tree in the sidebar and keeps the body only in the main surface', async () => {
    const { controller } = await fixture();
    controller.selectPage('concepts/job.md');
    const navigation = renderToStaticMarkup(
      createElement(WorkspaceWikiNavigation)
    );
    expect(navigation).toContain('aria-label="Directory concepts"');
    expect(navigation).toContain('aria-expanded="true"');
    expect(navigation).toContain('Job concept');
    expect(navigation).toContain('aria-current="page"');
    expect(navigation).not.toContain('Job definition');
    expect(navigation).not.toContain('Wiki output language');
    expect(navigation).not.toContain('Create Wiki from existing code');

    const article = renderToStaticMarkup(
      createElement(WorkspaceWikiArticle, {
        actions: createElement('button', null, 'Return to Chat'),
      })
    );
    expect(article).toContain('Job definition');
    expect(article).toContain('href="execution.md"');
    expect(article).toContain('<table');
    expect(article).toContain('data-mermaid="true"');
    expect(article).toContain('Job--&gt;Execution');
    expect(article).toContain('href="/projects/project-one/issues/issue-one"');
    expect(article).toContain('Current workspace');
    expect(article).toContain('Workspace branch: feature/wiki');
    expect(article).toContain('Repository One');
    expect(article).toContain('Return to Chat');
    expect(article).toContain('aria-label="Reload Wiki files"');
    expect(article).not.toContain('aria-label="Wiki pages"');
    expect(article).not.toContain('Wiki output language');
  });

  it('shares link navigation, search, directory expansion and reload without duplicating fetches', async () => {
    const { controller, api, snapshot } = await fixture();
    controller.selectPage('concepts/job.md');
    expect(controller.followLink('execution.md')).toBe(true);
    expect(renderToStaticMarkup(createElement(WorkspaceWikiArticle))).toContain(
      'One execution belongs to a job'
    );
    expect(
      renderToStaticMarkup(createElement(WorkspaceWikiNavigation))
    ).toContain('title="concepts/execution.md" aria-current="page"');
    expect(api.snapshot).toHaveBeenCalledTimes(1);
    controller.toggleDirectory('concepts');
    expect(
      renderToStaticMarkup(createElement(WorkspaceWikiNavigation))
    ).not.toContain('Job concept');
    controller.setQuery('Job contract');
    const filtered = renderToStaticMarkup(
      createElement(WorkspaceWikiNavigation)
    );
    expect(filtered).toContain('Job concept');
    expect(filtered).not.toContain('title="concepts/execution.md"');
    snapshot.wiki.pages[1].content = '# Execution\nUpdated execution';
    await controller.reload();
    expect(renderToStaticMarkup(createElement(WorkspaceWikiArticle))).toContain(
      'Updated execution'
    );
    expect(controller.getSnapshot().selectedPath).toBe('concepts/execution.md');
  });

  it('does not expose retired configuration or bootstrap controls in either surface', async () => {
    await fixture();
    const navigation = renderToStaticMarkup(
      createElement(WorkspaceWikiNavigation)
    );
    expect(navigation).not.toContain('Wiki output language');
    expect(navigation).not.toContain('Supplement Wiki from code');
    const article = renderToStaticMarkup(createElement(WorkspaceWikiArticle));
    expect(article).not.toContain('.llm-wiki/');
    expect(article).not.toContain('Supplement Wiki from code');
  });

  it('makes reload accessible from empty mobile navigation', async () => {
    const { snapshot } = await fixture();
    snapshot.wiki.exists = false;
    snapshot.wiki.index = null;
    snapshot.wiki.pages = [];
    const mobile = renderToStaticMarkup(
      createElement(WorkspaceWikiNavigation, { showReload: true })
    );
    expect(mobile).toContain('No Wiki pages in this repository.');
    expect(mobile).toContain('aria-label="Reload Wiki files"');
    expect(
      renderToStaticMarkup(createElement(WorkspaceWikiNavigation))
    ).not.toContain('aria-label="Reload Wiki files"');
  });
});
