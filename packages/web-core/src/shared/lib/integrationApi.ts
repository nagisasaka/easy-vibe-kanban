import type {
  CreateIntegrationRequest,
  IntegrationRun,
  IntegrationSelection,
  ParallelContextPage,
  Repo,
  WorkspaceActivity,
} from 'shared/types';
import { handleApiResponse } from '@/shared/lib/api';
import { makeLocalApiRequest } from '@/shared/lib/localApiTransport';

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  return handleApiResponse<T>(
    await makeLocalApiRequest(path, {
      ...init,
      headers: { 'Content-Type': 'application/json', ...init?.headers },
    })
  );
}

export const integrationApi = {
  repositories: (projectId: string) =>
    request<Repo[]>(
      `/api/integrations/repositories?project_id=${encodeURIComponent(projectId)}`
    ),
  list: (projectId: string) =>
    request<IntegrationRun[]>(
      `/api/integrations?project_id=${encodeURIComponent(projectId)}`
    ),
  create: (body: CreateIntegrationRequest) =>
    request<IntegrationRun>('/api/integrations', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  cancel: (id: string) =>
    request<IntegrationRun>(`/api/integrations/${id}/cancel`, {
      method: 'POST',
    }),
  recover: (id: string) =>
    request<IntegrationRun>(`/api/integrations/${id}/recover`, {
      method: 'POST',
    }),
  activities: async (repositoryId: string): Promise<WorkspaceActivity[]> => {
    const activities: WorkspaceActivity[] = [];
    let after: string | null = null;
    do {
      const page: ParallelContextPage = await request(
        `/api/repos/${repositoryId}/parallel-context?limit=100${after ? `&after_workspace=${after}` : ''}`
      );
      activities.push(...page.activities);
      after = page.next_workspace;
    } while (after);
    return activities;
  },
};

export function integrationCanCancel(status: string): boolean {
  return ['queued', 'preparing', 'integrating', 'validating'].includes(status);
}

export function integrationDoneLabel(
  status: string,
  published: boolean,
  doneResult: string | null
): string {
  if (doneResult) return doneResult;
  if (!published && ['cancelled', 'blocked', 'failed'].includes(status))
    return 'Done not applied';
  if (status === 'recovery_required') return 'Done awaiting reconciliation';
  return 'Done pending';
}

export function integrationSelectionIsCurrent(
  selected: IntegrationSelection[],
  activities: WorkspaceActivity[]
): boolean {
  return (
    selected.length > 0 &&
    selected.every(
      (s) =>
        !!s.expected_commit &&
        activities.some(
          (a) =>
            a.current_issue_id === s.card_id &&
            a.workspace_id === s.workspace_id &&
            a.observed_head_oid === s.expected_commit &&
            Number(a.active_agent_runs) === 0 &&
            Number(a.active_scripts) === 0
        )
    )
  );
}
