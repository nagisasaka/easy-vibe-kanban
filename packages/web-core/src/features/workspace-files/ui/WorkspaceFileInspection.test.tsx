import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { WorkspaceFilePreviewTarget } from '../model/types';
import { WorkspaceFilePreviewHeader } from './WorkspaceFilePreviewHeader';
import { UnsupportedFileViewer } from './viewers/UnsupportedFileViewer';

const state = vi.hoisted(() => ({ usage: undefined as string | undefined }));
vi.mock('@/shared/hooks/useWorkspaceRecord', () => ({
  useWorkspaceRecord: () => ({
    data: state.usage ? { usage: state.usage } : undefined,
  }),
}));
vi.mock('@/shared/hooks/useOpenInEditor', () => ({
  useOpenInEditor: () => vi.fn(),
}));
vi.mock('@/shared/providers/HostIdProvider', () => ({ useHostId: () => null }));
vi.mock('@/shared/hooks/useTheme', () => ({
  useTheme: () => ({ theme: 'light' }),
}));

describe('passive file inspection controls', () => {
  const target: WorkspaceFilePreviewTarget = {
    workspaceId: 'workspace',
    repoId: 'repo',
    path: 'file.bin',
    source: 'file-tree',
  };
  it.each([undefined, 'execution_only'])(
    'does not offer an editor for usage %s',
    (usage) => {
      state.usage = usage;
      const header = renderToStaticMarkup(
        createElement(WorkspaceFilePreviewHeader, {
          target,
          onRefresh: vi.fn(),
        })
      );
      const unsupported = renderToStaticMarkup(
        createElement(UnsupportedFileViewer, { target, rawUrl: '/safe-raw' })
      );
      expect(header).toContain('Refresh preview');
      expect(header).not.toContain('Open in editor');
      expect(unsupported).toContain('Open raw');
      expect(unsupported).not.toContain('Open editor');
    }
  );
  it('preserves ordinary editor affordances', () => {
    state.usage = 'interactive';
    expect(
      renderToStaticMarkup(
        createElement(WorkspaceFilePreviewHeader, {
          target,
          onRefresh: vi.fn(),
        })
      )
    ).toContain('Open in editor');
    expect(
      renderToStaticMarkup(createElement(UnsupportedFileViewer, { target }))
    ).toContain('Open editor');
  });
});
