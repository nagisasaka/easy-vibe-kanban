import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { describe, expect, it, vi } from 'vitest';
import type { Repo, RepositoryMemoryState } from 'shared/types';
import type { MachineClient } from '@/shared/lib/machineClient';
import { RepositoryMemorySettings } from './RepositoryMemorySettings';

function render(
  repoId: string,
  status: RepositoryMemoryState['status'],
  active = false,
  bootstrap = false
) {
  const query = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = {
    target: { id: 'local', kind: 'local', apiHostId: null, label: 'Local' },
    queryScopeKey: ['machine', 'local'],
    getRepositoryMemory: vi.fn(),
  } as unknown as MachineClient;
  query.setQueryData([...client.queryScopeKey, 'repository-memory', repoId], {
    enabled: true,
    status,
    output_language: 'ja',
    target_branch: 'main',
    last_success: status === 'uninitialized' ? null : '2026-09-13T00:00:00Z',
    active_run_id: active ? 'active-run' : null,
    bootstrap: bootstrap
      ? { workflow_run_id: 'bootstrap-run', phase: 'reviewing', child: null }
      : null,
    maintenance_workspace_id: `maintenance-${repoId}`,
    error: status === 'error' ? 'OpenWiki finalisation failed' : null,
  });
  return renderToStaticMarkup(
    createElement(
      QueryClientProvider,
      { client: query },
      createElement(RepositoryMemorySettings, {
        client,
        repo: { id: repoId, default_target_branch: 'main' } as Repo,
      })
    )
  );
}

describe('repository memory settings', () => {
  it('keeps the workflow owner active between child AgentRuns', () => {
    const markup = render('a', 'initializing', false, true);
    expect(markup).toContain('reviewing');
    expect(markup).toContain('disabled=""');
    expect(markup).toContain('independent read-only review');
  });
  it('distinguishes initialisation and sync and exposes errors without claiming current', () => {
    expect(render('a', 'uninitialized')).toContain('Initialize Wiki');
    const error = render('a', 'error');
    expect(error).toContain('Sync Wiki');
    expect(error).toContain('OpenWiki finalisation failed');
    expect(error).toContain('failed semantic events remain pending');
    expect(error).not.toContain('Status: current');
  });
  it('disables conflicting operations during maintenance and isolates repositories', () => {
    expect(render('a', 'reconciling', true)).toContain('disabled=""');
    expect(render('b', 'current')).toContain('maintenance-b');
    expect(render('b', 'current')).not.toContain('maintenance-a');
  });
});
