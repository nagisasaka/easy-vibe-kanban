import { describe, expect, it } from 'vitest';
import type { WikiSnapshot } from 'shared/types';
import {
  expandWikiLinks,
  resolveWikiHref,
  searchWikiPages,
  wikiRepositoryOptions,
} from './wikiNavigation';

const wiki: WikiSnapshot = {
  exists: true,
  config: { version: 1, output_language: 'ja' },
  index: { path: 'index.md', metadata: null, content: '# Wiki' },
  pages: [
    {
      path: 'pages/pipeline.md',
      metadata: {
        schema_version: 1,
        title: '再合成',
        language: 'ja',
        summary: 'Pipeline markers',
        tags: ['pipeline'],
        sources: ['EASY-123'],
        repos: ['easy'],
        created: '2026-09-09',
        updated: '2026-09-09',
      },
      content: '本文',
    },
  ],
};

describe('Wiki navigation', () => {
  it('searches frontmatter and full text', () => {
    expect(searchWikiPages(wiki, 'EASY-123 markers')).toHaveLength(1);
    expect(searchWikiPages(wiki, 'missing')).toHaveLength(0);
  });

  it('resolves index, page-relative and wikilinks without escaping', () => {
    const paths = new Set(['index.md', 'pages/pipeline.md']);
    expect(resolveWikiHref('index.md', 'pages/pipeline.md', paths)).toBe(
      'pages/pipeline.md'
    );
    expect(resolveWikiHref('pages/pipeline.md', '../index.md', paths)).toBe(
      'index.md'
    );
    expect(resolveWikiHref('index.md', '../../secret', paths)).toBeNull();
    expect(expandWikiLinks('See [[pipeline|the page]].')).toBe(
      'See [the page](pipeline.md).'
    );
  });

  it('switches between attached repositories and direct folders', () => {
    const workspace = {
      id: 'workspace-one',
      workspace_kind: 'worktree',
      name: 'Workspace',
    } as never;
    expect(
      wikiRepositoryOptions(workspace, [
        { id: 'repo-one', display_name: 'One' } as never,
        { id: 'repo-two', display_name: 'Two' } as never,
      ])
    ).toEqual([
      { id: 'repo-one', name: 'One' },
      { id: 'repo-two', name: 'Two' },
    ]);
    expect(
      wikiRepositoryOptions(
        {
          ...workspace,
          id: 'direct-workspace',
          workspace_kind: 'direct_folder',
          name: 'Notes',
        },
        []
      )
    ).toEqual([{ id: 'direct-workspace', name: 'Notes' }]);
  });
});
