import { useQuery } from '@tanstack/react-query';
import { workflowApi } from '@/shared/lib/workflowApi';

export const workspaceRepositoryWorkflowKey = (
  workspaceId?: string,
  hostId: string | null = null
) => ['workspace-repository-workflow', hostId, workspaceId] as const;

// Workflow transport is currently local-only. Never resolve a remote machine's
// workspace against the local database, even if an identifier happens to match.
export function useWorkspaceRepositoryWorkflow(
  workspaceId: string | undefined,
  hostId: string | null
) {
  return useQuery({
    queryKey: workspaceRepositoryWorkflowKey(workspaceId, hostId),
    queryFn: () => workflowApi.getRepositoryRunForWorkspace(workspaceId!),
    enabled: !!workspaceId && hostId === null,
    refetchInterval: (query) =>
      !query.state.data ||
      [
        'pending',
        'running',
        'cancelling',
        'awaiting_human',
        'awaiting_arena',
      ].includes(query.state.data.status)
        ? 5000
        : false,
  });
}
