import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { describe, expect, it, vi } from 'vitest';
import { WorkspaceWorkflowLink } from './WorkspaceWorkflowLink';
import { workspaceRepositoryWorkflowKey } from '@/shared/hooks/useWorkspaceRepositoryWorkflow';
import { workflowApi } from '@/shared/lib/workflowApi';

function render(
  status: string | null,
  hostId: string | null = null,
  workspaceId = 'workspace-1'
) {
  const client = new QueryClient();
  client.setQueryData(
    workspaceRepositoryWorkflowKey('workspace-1'),
    status ? { id: 'run-1', status } : null
  );
  const result = renderToStaticMarkup(
    createElement(
      QueryClientProvider,
      { client },
      createElement(WorkspaceWorkflowLink, { workspaceId, hostId })
    )
  );
  client.clear();
  return result;
}

describe('repository workflow entry point', () => {
  it.each([
    'pending',
    'running',
    'cancelling',
    'succeeded',
    'failed',
    'canceled',
  ])('links to durable workflow history when %s', (status) => {
    const markup = render(status);
    expect(markup).toContain('href="/workspaces/workspace-1/workflow"');
    expect(markup).toContain(status);
    expect(markup).not.toContain('/issues/');
  });

  it('does not advertise a bootstrap workflow for normal Sync or ordinary workspaces', () => {
    expect(render(null)).toBe('');
  });

  it('does not reuse another workspace or machine identity', () => {
    const fetch = vi.spyOn(workflowApi, 'getRepositoryRunForWorkspace');
    expect(render('running', null, 'workspace-2')).toBe('');
    expect(render('running', 'remote-host')).toBe('');
    expect(
      workspaceRepositoryWorkflowKey('workspace-1', 'remote-host')
    ).not.toEqual(workspaceRepositoryWorkflowKey('workspace-1'));
    expect(fetch).not.toHaveBeenCalled();
    fetch.mockRestore();
  });
});
