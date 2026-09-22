import { useCallback } from 'react';
import { useJsonPatchWsStream } from '@/shared/hooks/useJsonPatchWsStream';
import { useAppRuntime } from '@/shared/hooks/useAppRuntime';
import { useLocalStorageScratch } from '@/shared/hooks/useLocalStorageScratch';
import { scratchApi } from '@/shared/lib/api';
import { ScratchType, type Scratch, type UpdateScratch } from 'shared/types';
import { useHostId } from '@/shared/providers/HostIdProvider';
import { enqueueScratchMutation } from '@/shared/lib/scratchMutationQueue';

type ScratchState = {
  scratch: Scratch | null;
};

export interface UseScratchResult {
  scratch: Scratch | null;
  isLoading: boolean;
  isConnected: boolean;
  error: string | null;
  updateScratch: (update: UpdateScratch) => Promise<void>;
  deleteScratch: (expectedPayload?: UpdateScratch['payload']) => Promise<void>;
}

interface UseScratchOptions {
  /** Whether to enable the scratch connection. Defaults to true. */
  enabled?: boolean;
}

/**
 * Runtime-aware scratch storage hook.
 *
 * - Local runtime: streams a single scratch item via WebSocket (JSON Patch)
 *   backed by the server-side SQLite scratch table.
 * - Remote runtime: persists scratch data in localStorage for the stable
 *   cloud domain (cloud.vibekanban.com).
 */
export const useScratch = (
  scratchType: ScratchType,
  id: string,
  options?: UseScratchOptions
): UseScratchResult => {
  const runtime = useAppRuntime();
  const hostId = useHostId();
  const mutationKey = JSON.stringify([runtime, hostId, scratchType, id]);
  const isRemote = runtime === 'remote';

  // --- localStorage path (remote-web) ---
  const localResult = useLocalStorageScratch(scratchType, id, {
    enabled: isRemote && (options?.enabled ?? true),
  });

  // --- WebSocket/API path (local-web) ---
  const serverEnabled =
    !isRemote && (options?.enabled ?? true) && id.length > 0;
  const endpoint = serverEnabled
    ? `${hostId ? `/api/host/${hostId}` : '/api'}/scratch/${scratchType}/${id}/stream/ws`
    : undefined;

  const initialData = useCallback((): ScratchState => ({ scratch: null }), []);

  const { data, isConnected, isInitialized, error } =
    useJsonPatchWsStream<ScratchState>(endpoint, serverEnabled, initialData);

  // Treat deleted scratches as null
  const rawScratch = data?.scratch as (Scratch & { deleted?: boolean }) | null;
  const scratch = rawScratch?.deleted ? null : rawScratch;

  const updateScratch = useCallback(
    async (update: UpdateScratch) => {
      await enqueueScratchMutation(mutationKey, () =>
        scratchApi.update(scratchType, id, update, hostId)
      );
    },
    [scratchType, id, hostId, mutationKey]
  );

  const deleteScratch = useCallback(
    async (expectedPayload?: UpdateScratch['payload']) => {
      await enqueueScratchMutation(mutationKey, () =>
        scratchApi.delete(scratchType, id, hostId, expectedPayload)
      );
    },
    [scratchType, id, hostId, mutationKey]
  );

  // A failed connection is not proof that a draft is empty. Keep waiting for
  // the first authoritative snapshot (the editor still permits local typing).
  const isLoading = serverEnabled && !isInitialized;

  const serverResult: UseScratchResult = {
    scratch,
    isLoading,
    isConnected,
    error,
    updateScratch,
    deleteScratch,
  };

  return isRemote ? localResult : serverResult;
};
