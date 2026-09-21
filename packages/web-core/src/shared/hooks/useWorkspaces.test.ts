import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { WorkspaceSummary, WorkspaceWithStatus } from 'shared/types';
import { useWorkspaces, type UseWorkspacesResult } from './useWorkspaces';

const fixture = vi.hoisted(() => ({
  summaries: new Map<string, WorkspaceSummary>(),
  workspace: {} as WorkspaceWithStatus,
}));

vi.mock('@/shared/providers/HostIdProvider', () => ({
  useHostId: () => null,
}));
vi.mock('@/shared/hooks/useJsonPatchWsStream', () => ({
  useJsonPatchWsStream: (endpoint: string) => ({
    data: {
      workspaces: endpoint.endsWith('archived=false')
        ? { [fixture.workspace.id]: fixture.workspace }
        : {},
    },
    isInitialized: true,
    isConnected: true,
    error: null,
  }),
}));
vi.mock('@tanstack/react-query', () => ({
  keepPreviousData: (data: unknown) => data,
  useQuery: () => ({ data: fixture.summaries }),
}));

function readWorkspaces() {
  let result: UseWorkspacesResult | undefined;
  function Probe() {
    result = useWorkspaces();
    return null;
  }
  renderToStaticMarkup(createElement(Probe));
  return result!;
}

describe('workspace sidebar runtime state', () => {
  beforeEach(() => {
    fixture.workspace = {
      id: 'workspace',
      name: 'OpenWiki maintenance',
      created_at: '2026-09-17T00:00:00Z',
      updated_at: '2026-09-17T00:00:00Z',
      is_running: true,
      archived: false,
      usage: 'interactive',
    } as WorkspaceWithStatus;
    fixture.summaries = new Map();
  });

  it('keeps human workspaces even when their name resembles an internal execution', () => {
    const result = readWorkspaces();
    expect(result.workspaces.map((workspace) => workspace.id)).toEqual([
      'workspace',
    ]);
    expect(result.executionWorkspaces).toEqual([]);
  });

  it('separates persistent execution usage even after a successful agent run', () => {
    fixture.workspace.usage = 'execution_only';
    fixture.workspace.is_running = false;
    const result = readWorkspaces();
    expect(result.workspaces).toEqual([]);
    expect(result.archivedWorkspaces).toEqual([]);
    expect(result.executionWorkspaces.map((workspace) => workspace.id)).toEqual(
      ['workspace']
    );
  });

  it('converges after a terminal AgentRun even when the legacy workspace stream stays running', () => {
    expect(readWorkspaces().workspaces[0].isRunning).toBe(true);
    fixture.summaries = new Map([
      [
        'workspace',
        {
          workspace_id: 'workspace',
          is_running: false,
          latest_process_status: 'succeeded',
        } as WorkspaceSummary,
      ],
    ]);
    expect(readWorkspaces().workspaces[0].isRunning).toBe(false);
  });

  it.each(['succeeded', null] as const)(
    'does not infer inactivity from the latest AgentRun (%s) when another agent or script is active',
    (latestStatus) => {
      fixture.workspace.is_running = false;
      fixture.summaries.set('workspace', {
        workspace_id: 'workspace',
        is_running: true,
        latest_process_status: latestStatus,
      } as WorkspaceSummary);
      expect(readWorkspaces().workspaces[0].isRunning).toBe(true);
    }
  );

  it('retains streaming status before summary fetch or with an older host response', () => {
    fixture.summaries.set('workspace', {
      workspace_id: 'workspace',
      latest_process_status: 'succeeded',
    } as WorkspaceSummary);
    expect(readWorkspaces().workspaces[0].isRunning).toBe(true);
  });
});
