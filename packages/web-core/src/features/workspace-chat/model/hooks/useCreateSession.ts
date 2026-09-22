import { useMutation, useQueryClient } from '@tanstack/react-query';
import { sessionsApi } from '@/shared/lib/api';
import { workspaceSessionKeys } from '@/shared/hooks/workspaceSessionKeys';
import type {
  Session,
  CreateFollowUpAttempt,
  ExecutorConfig,
  SelectedSkill,
} from 'shared/types';

interface CreateSessionParams {
  hostId: string | null;
  workspaceId: string;
  prompt: string;
  selectedSkills?: SelectedSkill[];
  executorConfig: ExecutorConfig;
  resumeSessionId?: string | null;
  resumeScopePath?: string | null;
}

/**
 * Hook for creating a new session and sending the first message.
 * Uses TanStack Query mutation for proper cache management.
 */
export function useCreateSession() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({
      hostId,
      workspaceId,
      prompt,
      selectedSkills = [],
      executorConfig,
      resumeSessionId,
      resumeScopePath,
    }: CreateSessionParams): Promise<Session> => {
      const session = await sessionsApi.create(
        {
          workspace_id: workspaceId,
        },
        hostId
      );

      const body: CreateFollowUpAttempt = {
        prompt,
        selected_skills: selectedSkills,
        executor_config: executorConfig,
        resume_session_id: resumeSessionId || undefined,
        resume_scope_path: resumeScopePath || undefined,
      };
      await sessionsApi.followUp(session.id, body, hostId);

      return session;
    },
    onSuccess: (session, { hostId }) => {
      // Invalidate session queries to refresh the list
      queryClient.invalidateQueries({
        queryKey: workspaceSessionKeys.byWorkspace(
          session.workspace_id,
          hostId
        ),
      });
    },
  });
}
