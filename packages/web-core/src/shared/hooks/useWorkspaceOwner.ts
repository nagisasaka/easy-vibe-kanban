import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useHostId } from '@/shared/providers/HostIdProvider';
import { makeLocalApiRequest } from '@/shared/lib/localApiTransport';
import type { ApiResponse, WorkspaceExecutionView } from 'shared/types';

export async function executionRequest<T>(
  hostId: string | null,
  path: string,
  method = 'GET'
): Promise<T> {
  const response = await makeLocalApiRequest(`/api/workspaces${path}`, {
    method,
    hostScope: 'explicit',
    hostId,
  });
  const result: ApiResponse<T> = await response.json();
  if (
    !response.ok ||
    !result.success ||
    (method === 'GET' && result.data === null)
  ) {
    throw new Error(result.message || 'Execution details unavailable');
  }
  return result.data as T;
}

export function useWorkspaceExecutions() {
  const host = useHostId();
  return useQuery({
    queryKey: ['workspace-executions', host],
    queryFn: () =>
      executionRequest<WorkspaceExecutionView[]>(host, '/executions'),
    refetchInterval: 5000,
  });
}

export function useWorkspaceOwnerView(workspaceId: string) {
  const host = useHostId();
  const client = useQueryClient();
  const query = useQuery({
    queryKey: ['workspace-execution', host, workspaceId],
    queryFn: () =>
      executionRequest<WorkspaceExecutionView>(host, `/${workspaceId}/usage`),
    refetchInterval: 3000,
  });
  const stop = useMutation({
    mutationFn: () =>
      executionRequest<null>(host, `/${workspaceId}/execution/stop`, 'POST'),
    onSettled: () => {
      void client.invalidateQueries({
        queryKey: ['workspace-execution', host, workspaceId],
      });
      void client.invalidateQueries({
        queryKey: ['workspace-executions', host],
      });
    },
  });
  return { ...query, stop };
}
