import { useCallback } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { queueApi } from '@/shared/lib/api';
import type { ExecutorConfig, QueueStatus, SelectedSkill } from 'shared/types';
import { useHostId } from '@/shared/providers/HostIdProvider';

type QueueOperation = Parameters<typeof queueApi.edit>[1];

export function useSessionQueueInteraction({
  sessionId,
}: {
  sessionId: string | undefined;
}) {
  const queryClient = useQueryClient();
  const hostId = useHostId();
  const { data: status = { status: 'empty' as const }, refetch } =
    useQuery<QueueStatus>({
      queryKey: ['queue-status', hostId, sessionId],
      queryFn: () => queueApi.getStatus(sessionId!, hostId),
      enabled: !!sessionId,
      refetchInterval: 2000,
    });
  // Capture scope in mutation variables, including when a user switches sessions.
  const mutation = useMutation({
    mutationFn: async ({
      sessionId,
      hostId,
      action,
    }: {
      sessionId: string;
      hostId: string | null;
      action:
        | {
            type: 'append';
            message: string;
            executorConfig: ExecutorConfig;
            selectedSkills?: SelectedSkill[];
          }
        | { type: 'resume' }
        | { type: 'cancel' }
        | QueueOperation;
    }) => {
      if (action.type === 'append')
        return queueApi.queue(
          sessionId,
          {
            message: action.message,
            executor_config: action.executorConfig,
            selected_skills: action.selectedSkills,
          },
          hostId
        );
      if (action.type === 'resume') return queueApi.resume(sessionId, hostId);
      if (action.type === 'cancel') return queueApi.cancel(sessionId, hostId);
      return queueApi.edit(sessionId, action, hostId);
    },
    onSuccess: (status, { sessionId, hostId }) =>
      queryClient.setQueryData(['queue-status', hostId, sessionId], status),
    onSettled: (_data, _error, { sessionId, hostId }) => {
      void queryClient.invalidateQueries({
        queryKey: ['queue-status', hostId, sessionId],
      });
    },
  });
  const perform = async (
    action: Parameters<typeof mutation.mutateAsync>[0]['action']
  ) => {
    if (!sessionId) return;
    await mutation.mutateAsync({ sessionId, hostId, action });
  };
  const refreshQueueStatus = useCallback(async () => {
    await refetch();
  }, [refetch]);
  return {
    isQueued: status.status === 'queued',
    messages: status.status === 'queued' ? status.messages : [],
    paused: status.status === 'queued' && status.paused,
    isQueueLoading: mutation.isPending,
    queueError: mutation.error?.message,
    queueMessage: (
      message: string,
      executorConfig: ExecutorConfig,
      selectedSkills?: SelectedSkill[]
    ) => perform({ type: 'append', message, executorConfig, selectedSkills }),
    cancelQueue: () => perform({ type: 'cancel' }),
    editQueue: (operation: QueueOperation) => perform(operation),
    resumeQueue: () => perform({ type: 'resume' }),
    refreshQueueStatus,
  };
}
