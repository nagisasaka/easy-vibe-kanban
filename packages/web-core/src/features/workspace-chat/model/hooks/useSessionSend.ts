import { useCallback, useEffect, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import type { ExecutorConfig, SelectedSkill } from 'shared/types';
import { sessionsApi } from '@/shared/lib/api';
import { useCreateSession } from './useCreateSession';
import { goalPromptError } from '../goalValidation';
import { useHostId } from '@/shared/providers/HostIdProvider';

interface UseSessionSendOptions {
  /** Session ID for existing sessions */
  sessionId: string | undefined;
  /** Workspace ID for creating new sessions */
  workspaceId: string | undefined;
  /** Whether in new session mode */
  isNewSessionMode: boolean;
  /** Callback when session is selected (to exit new session mode) */
  onSelectSession?: (sessionId: string) => void;
  /** Unified executor config (executor + variant + overrides) */
  executorConfig?: ExecutorConfig | null;
}

interface UseSessionSendResult {
  /** Send a message; the caller decides whether to select a newly created Session. */
  send: (
    message: string,
    selectedSkills?: SelectedSkill[],
    options?: {
      resumeSessionId?: string | null;
      resumeScopePath?: string | null;
      executorConfig?: ExecutorConfig;
    }
  ) => Promise<{ createdSessionId?: string } | null>;
  /** Whether a send operation is in progress */
  isSending: boolean;
  /** Error message if send failed */
  error: string | null;
  /** Clear the error */
  clearError: () => void;
}

/**
 * Hook for sending messages in SessionChatBoxContainer.
 * Handles both new session creation and existing session follow-up.
 *
 * Unlike useFollowUpSend, this hook:
 * - Takes message/variant as parameters to send() (not captured in closure)
 * - Returns the created identity on success (caller handles scoped cleanup)
 * - Has no prompt composition (no conflict/review/clicked markdown)
 */
export function useSessionSend({
  sessionId,
  workspaceId,
  isNewSessionMode,
  executorConfig,
}: UseSessionSendOptions): UseSessionSendResult {
  const hostId = useHostId();
  const queryClient = useQueryClient();
  const scope = JSON.stringify([
    hostId,
    workspaceId,
    sessionId,
    isNewSessionMode,
  ]);
  const currentScope = useRef(scope);
  currentScope.current = scope;
  const { mutateAsync: createSession, isPending: isCreatingSession } =
    useCreateSession();
  const [isSendingFollowUp, setIsSendingFollowUp] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setError(null);
  }, [scope]);

  const send = useCallback(
    async (
      message: string,
      selectedSkills: SelectedSkill[] = [],
      options: {
        resumeSessionId?: string | null;
        resumeScopePath?: string | null;
        executorConfig?: ExecutorConfig;
      } = {}
    ): Promise<{ createdSessionId?: string } | null> => {
      const trimmed = message.trim();
      const effectiveConfig = options.executorConfig ?? executorConfig;
      if (!trimmed) return null;
      if (!effectiveConfig) {
        setError('No executor selected');
        return null;
      }

      setError(null);
      const validationError = goalPromptError(trimmed, effectiveConfig);
      if (validationError) {
        setError(validationError);
        return null;
      }

      if (isNewSessionMode) {
        // New session flow
        if (!workspaceId) {
          setError('No workspace selected');
          return null;
        }
        try {
          const session = await createSession({
            hostId,
            workspaceId,
            prompt: trimmed,
            selectedSkills,
            executorConfig: effectiveConfig,
            resumeSessionId: options.resumeSessionId,
            resumeScopePath: options.resumeScopePath,
          });
          return { createdSessionId: session.id };
        } catch (e: unknown) {
          const err = e as { message?: string };
          if (currentScope.current === scope)
            setError(
              `Failed to create session: ${err.message ?? 'Unknown error'}`
            );
          return null;
        }
      } else {
        // Existing session flow
        if (!sessionId) return null;
        setIsSendingFollowUp(true);
        try {
          await sessionsApi.followUp(
            sessionId,
            {
              prompt: trimmed,
              selected_skills: selectedSkills,
              executor_config: effectiveConfig,
              resume_session_id: options.resumeSessionId || undefined,
              resume_scope_path: options.resumeScopePath || undefined,
            },
            hostId
          );
          void queryClient.invalidateQueries({
            queryKey: ['session-executor-config', hostId, sessionId],
          });
          return {};
        } catch (e: unknown) {
          const err = e as { message?: string };
          if (currentScope.current === scope)
            setError(`Failed to send: ${err.message ?? 'Unknown error'}`);
          return null;
        } finally {
          setIsSendingFollowUp(false);
        }
      }
    },
    [
      sessionId,
      workspaceId,
      isNewSessionMode,
      createSession,
      scope,
      hostId,
      queryClient,
      executorConfig,
    ]
  );

  const clearError = useCallback(() => setError(null), []);

  return {
    send,
    isSending: isSendingFollowUp || isCreatingSession,
    error,
    clearError,
  };
}
