import { afterEach, describe, expect, it, vi } from 'vitest';
import type { WorkspaceWikiSnapshot } from 'shared/types';
import { createWikiViewerController } from './wikiViewerController';

function snapshot(
  workspaceId = 'workspace',
  repoId = 'repo'
): WorkspaceWikiSnapshot {
  return {
    workspace_id: workspaceId,
    repo_id: repoId,
    repo_name: repoId,
    repo_display_name: repoId,
    source: 'current_workspace',
    source_links: [],
    wiki: {
      exists: true,
      config: null,
      index: { path: 'index.md', metadata: null, content: '# Wiki' },
      pages: [
        { path: 'pages/concept.md', metadata: null, content: 'A concept' },
      ],
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

afterEach(() => vi.unstubAllGlobals());

describe('shared Wiki viewer controller', () => {
  it('shares one fetch, page selection and links across navigation/article, preserving state while hidden', async () => {
    const api = {
      snapshot: vi.fn().mockResolvedValue(snapshot()),
    };
    const controller = createWikiViewerController('workspace', 'repo', api);
    const navigation = vi.fn();
    const article = vi.fn();
    const stopNavigation = controller.subscribe(navigation);
    const stopArticle = controller.subscribe(article);
    await controller.reload();
    expect(api.snapshot).toHaveBeenCalledTimes(1);
    expect(controller.getSnapshot().selectedPath).toBe('index.md');
    controller.selectPage('pages/concept.md');
    controller.setQuery('concept');
    controller.rememberScroll('workspace:repo:openwiki:pages/concept.md', 321);
    controller.toggleDirectory('pages');
    controller.setEnabled(true);
    controller.setEnabled(false);
    controller.setEnabled(true);
    expect(api.snapshot).toHaveBeenCalledTimes(1);
    expect(controller.getSnapshot()).toMatchObject({
      selectedPath: 'pages/concept.md',
      query: 'concept',
      collapsedDirectories: ['pages'],
    });
    expect(
      controller.scrollPosition('workspace:repo:openwiki:pages/concept.md')
    ).toBe(321);
    expect(controller.followLink('../index.md')).toBe(true);
    expect(controller.getSnapshot().selectedPath).toBe('index.md');
    expect(controller.followLink('https://example.com/')).toBe(false);
    expect(controller.followLink('../../outside.md')).toBe(false);
    controller.selectPage('unknown.md');
    expect(controller.getSnapshot().selectedPath).toBe('index.md');
    expect(navigation.mock.calls.length).toBe(article.mock.calls.length);
    stopNavigation();
    stopArticle();
  });

  it('reloads the same source without dropping current selection, query or scroll', async () => {
    const api = {
      snapshot: vi.fn().mockResolvedValue(snapshot()),
    };
    const controller = createWikiViewerController('workspace', 'repo', api);
    await controller.reload();
    controller.selectPage('pages/concept.md');
    controller.setQuery('concept');
    controller.rememberScroll('selected-page', 250);
    const changed = snapshot();
    changed.wiki.pages[0].content = 'Updated concept';
    api.snapshot.mockResolvedValue(changed);
    await controller.reload();
    expect(controller.getSnapshot()).toMatchObject({
      selectedPath: 'pages/concept.md',
      query: 'concept',
      snapshot: changed,
    });
    expect(controller.scrollPosition('selected-page')).toBe(250);
    changed.wiki.pages = [];
    await controller.reload();
    expect(controller.getSnapshot().selectedPath).toBe('index.md');
  });

  it('does not let stale repository responses replace the selected source', async () => {
    const old = deferred<WorkspaceWikiSnapshot>();
    const next = deferred<WorkspaceWikiSnapshot>();
    const api = {
      snapshot: vi
        .fn()
        .mockReturnValueOnce(old.promise)
        .mockReturnValueOnce(next.promise),
    };
    const controller = createWikiViewerController('workspace', 'repo', api);
    const first = controller.reload();
    controller.selectRepository('other');
    controller.setQuery('new');
    const second = controller.reload();
    old.resolve(snapshot());
    await first;
    expect(controller.getSnapshot()).toMatchObject({
      repoId: 'other',
      snapshot: null,
      loading: true,
      query: 'new',
    });
    next.resolve(snapshot('workspace', 'other'));
    await second;
    expect(controller.getSnapshot().snapshot?.repo_id).toBe('other');

    const stale = deferred<WorkspaceWikiSnapshot>();
    api.snapshot.mockReturnValueOnce(stale.promise);
    const third = controller.reload();
    controller.rememberScroll('old-format', 80);
    controller.selectRepository('third');
    stale.reject(new Error('old format failed'));
    await third;
    expect(controller.getSnapshot()).toMatchObject({
      repoId: 'third',
      snapshot: null,
      selectedPath: 'index.md',
      query: '',
      error: null,
    });
    expect(controller.scrollPosition('old-format')).toBe(0);
  });

  it('rejects response identity mismatches and treats errors/absence explicitly', async () => {
    const api = {
      snapshot: vi.fn().mockResolvedValue(snapshot('another-workspace')),
    };
    const controller = createWikiViewerController('workspace', 'repo', api);
    await controller.reload();
    expect(controller.getSnapshot().snapshot).toBeNull();
    expect(controller.getSnapshot().error).toContain('does not match');
    const absent = snapshot();
    absent.wiki = { exists: false, config: null, index: null, pages: [] };
    api.snapshot.mockResolvedValue(absent);
    await controller.reload();
    expect(controller.getSnapshot()).toMatchObject({
      snapshot: absent,
      error: null,
      loading: false,
    });
    controller.selectRepository('');
    await controller.reload();
    expect(controller.getSnapshot()).toMatchObject({
      snapshot: null,
      loading: false,
    });
  });

  it('ignores retired format preferences and keeps independent workspace read/scroll state', async () => {
    const values = new Map([['evk-wiki-viewer-format:one', 'llm-wiki']]);
    vi.stubGlobal('sessionStorage', {
      getItem: (key: string) => values.get(key),
      setItem: (key: string, value: string) => values.set(key, value),
    });
    const api = {
      snapshot: vi
        .fn()
        .mockImplementation((workspaceId: string) =>
          Promise.resolve(snapshot(workspaceId))
        ),
    };
    const one = createWikiViewerController('one', 'repo', api);
    const two = createWikiViewerController('two', 'repo', api);
    expect(one).not.toHaveProperty('selectFormat');
    expect(one).not.toHaveProperty('saveLanguage');
    await one.reload();
    await two.reload();
    expect(api.snapshot).toHaveBeenCalledWith('one', 'repo');
    expect(api.snapshot).toHaveBeenCalledWith('two', 'repo');
    one.rememberScroll('page', 100);
    expect(two.scrollPosition('page')).toBe(0);
    expect(values.get('evk-wiki-viewer-format:one')).toBe('llm-wiki');
  });
});
