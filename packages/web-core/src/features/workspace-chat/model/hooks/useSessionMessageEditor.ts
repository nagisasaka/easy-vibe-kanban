import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ScratchType,
  type DraftFollowUpData,
  type ExecutorConfig,
  type UpdateScratch,
} from 'shared/types';
import { useScratch } from '@/shared/hooks/useScratch';
import { useDebouncedCallback } from '@/shared/hooks/useDebouncedCallback';
import { useHostId } from '@/shared/providers/HostIdProvider';
import { useAppRuntime } from '@/shared/hooks/useAppRuntime';
import {
  acknowledgePendingSessionDraft,
  readPendingSessionDraft,
  writePendingSessionDraft,
} from '../pendingSessionDraft';

interface UseSessionMessageEditorOptions {
  /** Scratch ID (workspaceId for new session, sessionId for existing) */
  scratchId: string | undefined;
}

interface UseSessionMessageEditorResult {
  /** Current message value */
  localMessage: string;
  /** Set local message directly */
  setLocalMessage: (value: string) => void;
  /** Scratch data (message and variant) */
  scratchData: DraftFollowUpData | undefined;
  /** Whether scratch is loading */
  isScratchLoading: boolean;
  /** Whether the initial value has been applied from scratch */
  hasInitialValue: boolean;
  /** Save message and executor config to scratch */
  saveToScratch: (
    message: string,
    executorConfig: ExecutorConfig
  ) => Promise<void>;
  /** Delete the draft scratch */
  clearDraft: (expectedPayload?: UpdateScratch['payload']) => Promise<void>;
  getDraftRevision: () => number;
  /** Cancel pending debounced save */
  cancelDebouncedSave: () => void;
  /** Handle message change with debounced save */
  handleMessageChange: (value: string, executorConfig: ExecutorConfig) => void;
}

/**
 * Hook to manage message editing with draft persistence.
 * Handles local state, debounced saves to scratch, and sync on load.
 */
export function useSessionMessageEditor({
  scratchId,
}: UseSessionMessageEditorOptions): UseSessionMessageEditorResult {
  const hostId = useHostId();
  const runtime = useAppRuntime();
  const scope = JSON.stringify([runtime, hostId, scratchId]);
  const {
    scratch,
    updateScratch,
    deleteScratch,
    isLoading: isServerScratchLoading,
  } = useScratch(ScratchType.DRAFT_FOLLOW_UP, scratchId ?? '');

  const pendingLocal = scratchId ? readPendingSessionDraft(scope) : null;
  const scratchData: DraftFollowUpData | undefined =
    pendingLocal?.data ??
    (scratch?.payload?.type === 'DRAFT_FOLLOW_UP'
      ? scratch.payload.data
      : undefined);
  const isScratchLoading = isServerScratchLoading && !pendingLocal;

  const [localMessage, setMessage] = useState('');
  const hasLoadedRef = useRef(false);
  const revision = useRef(0);
  const getDraftRevision = useCallback(() => revision.current, []);
  const setLocalMessage = useCallback((value: string) => {
    revision.current++;
    hasLoadedRef.current = true;
    setHasInitialValue(true);
    setMessage(value);
  }, []);
  const [hasInitialValue, setHasInitialValue] = useState(false);

  const saveToScratch = useCallback(
    async (message: string, executorConfig: ExecutorConfig) => {
      if (!scratchId) return;
      const data = { message, executor_config: executorConfig };
      const backup = writePendingSessionDraft(scope, data);
      try {
        await updateScratch({
          payload: {
            type: 'DRAFT_FOLLOW_UP',
            data,
          },
        });
        acknowledgePendingSessionDraft(scope, backup);
      } catch (e) {
        console.error('Failed to save follow-up draft', e);
      }
    },
    [scope, scratchId, updateScratch]
  );

  const clearDraft = useCallback(
    async (expectedPayload?: UpdateScratch['payload']) => {
      const backup = readPendingSessionDraft(scope);
      await deleteScratch(expectedPayload);
      if (
        !expectedPayload ||
        (expectedPayload.type === 'DRAFT_FOLLOW_UP' &&
          JSON.stringify(backup?.data) === JSON.stringify(expectedPayload.data))
      )
        acknowledgePendingSessionDraft(scope, backup?.raw);
    },
    [deleteScratch, scope]
  );

  const pendingSave = useRef<(() => Promise<void>) | null>(null);
  const { debounced: debouncedSave, cancel } = useDebouncedCallback(() => {
    const operation = pendingSave.current;
    pendingSave.current = null;
    void operation?.();
  }, 500);
  const cancelDebouncedSave = useCallback(() => {
    cancel();
    pendingSave.current = null;
  }, [cancel]);

  // Flush to the captured identity when navigating/unmounting before debounce.
  // A late write must never acquire the next Session's storage callback.
  useEffect(
    () => () => {
      cancel();
      const operation = pendingSave.current;
      pendingSave.current = null;
      void operation?.();
    },
    [scope, cancel]
  );

  // Reset load state and clear message when scratchId changes (e.g., switching to approval mode)
  useEffect(() => {
    cancelDebouncedSave();
    hasLoadedRef.current = false;
    setHasInitialValue(false);
    revision.current++;
    setMessage('');
  }, [scope, cancelDebouncedSave]);

  // Sync local message from scratch only on initial load
  useEffect(() => {
    if (isScratchLoading) return;
    if (hasLoadedRef.current) return;
    hasLoadedRef.current = true;
    setMessage(scratchData?.message ?? '');
    setHasInitialValue(true);
    // A browser reload does not run React unmount cleanup. Replay only this
    // tab/Host/Session's unacknowledged edit, never another scope's draft.
    const backup = readPendingSessionDraft(scope);
    if (backup)
      void saveToScratch(backup.data.message, backup.data.executor_config);
  }, [isScratchLoading, scratchData?.message, scope, saveToScratch]);

  // Handle message change with debounced save
  // Pass executor profile at call-time to avoid stale closure
  const handleMessageChange = useCallback(
    (value: string, executorConfig: ExecutorConfig) => {
      if (scratchId)
        writePendingSessionDraft(scope, {
          message: value,
          executor_config: executorConfig,
        });
      setLocalMessage(value);
      // Capture the storage identity at edit time, not when the timer fires.
      pendingSave.current = () => saveToScratch(value, executorConfig);
      debouncedSave();
    },
    [debouncedSave, saveToScratch, setLocalMessage, scope, scratchId]
  );

  return {
    localMessage,
    setLocalMessage,
    scratchData,
    isScratchLoading,
    hasInitialValue,
    saveToScratch,
    clearDraft,
    getDraftRevision,
    cancelDebouncedSave,
    handleMessageChange,
  };
}
