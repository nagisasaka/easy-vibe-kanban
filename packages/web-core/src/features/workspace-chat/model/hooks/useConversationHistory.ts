import { ExecutionProcess, PatchType } from 'shared/types';
import { useExecutionProcessesContext } from '@/shared/hooks/useExecutionProcessesContext';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { streamJsonPatchEntries } from '@/shared/lib/streamJsonPatchEntries';
import {
  getExecutionProcessLifecycle,
  isExecutionProcessActive,
} from '@/shared/lib/executionProcessRuntime';
import type {
  AddEntryType,
  ConversationTimelineSource,
  ExecutionProcessStateStore,
  PatchTypeWithKey,
  UseConversationHistoryParams,
} from '@/shared/hooks/useConversationHistory/types';
import { useCanonicalAgentSession } from '../contexts/EntriesContext';

// Result type for the new UI's conversation history hook
export interface UseConversationHistoryResult {
  /** Whether the conversation only has a single coding agent turn (no follow-ups) */
  isFirstTurn: boolean;
  /** Whether background batches are still loading older history entries */
  isLoadingHistory: boolean;
}
import {
  MIN_INITIAL_ENTRIES,
  REMAINING_BATCH_SIZE,
} from '@/shared/hooks/useConversationHistory/constants';

export const useConversationHistory = ({
  onTimelineUpdated,
  scopeKey,
}: UseConversationHistoryParams): UseConversationHistoryResult => {
  const {
    executionProcessesVisible: executionProcessesRaw,
    isLoading,
    isConnected,
  } = useExecutionProcessesContext();
  const canonicalSession = useCanonicalAgentSession();
  const executionProcesses = useRef<ExecutionProcess[]>(executionProcessesRaw);
  const displayedExecutionProcesses = useRef<ExecutionProcessStateStore>({});
  const loadedInitialEntries = useRef(false);
  const emittedEmptyInitialRef = useRef(false);
  const streamingProcessIdsRef = useRef<Set<string>>(new Set());
  const onTimelineUpdatedRef = useRef<
    UseConversationHistoryParams['onTimelineUpdated'] | null
  >(null);
  const previousActiveMapRef = useRef<Map<string, boolean>>(new Map());
  const [isLoadingHistoryState, setIsLoadingHistory] = useState(false);
  const generationRef = useRef(0);
  const controllersRef = useRef(new Set<{ close: () => void }>());

  // Derive whether this is the first turn (no follow-up processes exist)
  const isFirstTurn = useMemo(() => {
    return canonicalSession.conversation.runCount <= 1;
  }, [canonicalSession.conversation.runCount]);

  const mergeIntoDisplayed = (
    mutator: (state: ExecutionProcessStateStore) => void
  ) => {
    const state = displayedExecutionProcesses.current;
    mutator(state);
  };

  const canonicalRef = useRef(canonicalSession.conversation);
  canonicalRef.current = canonicalSession.conversation;
  const canonicalInitializedRef = useRef(false);

  // The hook owns transport, loading, and reconciliation.
  // It emits a source model that later derivation layers can transform further.

  const buildTimelineSource = useCallback(
    (
      executionProcessState: ExecutionProcessStateStore
    ): ConversationTimelineSource => ({
      executionProcessState,
      liveExecutionProcesses: executionProcesses.current,
      canonical: canonicalRef.current,
    }),
    []
  );

  useEffect(() => {
    onTimelineUpdatedRef.current = onTimelineUpdated;
  }, [onTimelineUpdated]);

  // Keep executionProcesses up to date
  useEffect(() => {
    executionProcesses.current = executionProcessesRaw.filter(
      (ep) => ep.executor_action.typ.type === 'ScriptRequest'
    );
  }, [executionProcessesRaw]);

  const loadEntriesForHistoricExecutionProcess = (
    executionProcess: ExecutionProcess
  ) => {
    const url = `/api/execution-processes/${executionProcess.id}/raw-logs/ws`;

    return new Promise<PatchType[]>((resolve) => {
      const controller = streamJsonPatchEntries<PatchType>(url, {
        onFinished: (allEntries) => {
          controllersRef.current.delete(controller);
          controller.close();
          resolve(allEntries);
        },
        onError: (err) => {
          console.warn(
            `Error loading entries for historic execution process ${executionProcess.id}`,
            err
          );
          controllersRef.current.delete(controller);
          controller.close();
          resolve([]);
        },
      });
      controllersRef.current.add(controller);
    });
  };

  const patchWithKey = (
    patch: PatchType,
    executionProcessId: string,
    index: number
  ) => {
    return {
      ...patch,
      patchKey: `${executionProcessId}:${index}`,
      executionProcessId,
    };
  };

  const flattenEntries = (
    executionProcessState: ExecutionProcessStateStore
  ): PatchTypeWithKey[] => {
    return Object.values(executionProcessState)
      .filter(
        (p) => p.executionProcess.executor_action.typ.type === 'ScriptRequest'
      )
      .sort(
        (a, b) =>
          new Date(
            a.executionProcess.created_at as unknown as string
          ).getTime() -
          new Date(b.executionProcess.created_at as unknown as string).getTime()
      )
      .flatMap((p) => p.entries);
  };

  const getActiveScriptProcesses = (): ExecutionProcess[] => {
    return (
      executionProcesses?.current.filter(
        (p) => isExecutionProcessActive(p) && p.run_reason !== 'devserver'
      ) ?? []
    );
  };

  const emitEntries = useCallback(
    (
      executionProcessState: ExecutionProcessStateStore,
      addEntryType: AddEntryType,
      loading: boolean
    ) => {
      const timelineSource = buildTimelineSource(executionProcessState);
      let modifiedAddEntryType = addEntryType;

      const latestEntry = Object.values(executionProcessState)
        .sort(
          (a, b) =>
            new Date(
              a.executionProcess.created_at as unknown as string
            ).getTime() -
            new Date(
              b.executionProcess.created_at as unknown as string
            ).getTime()
        )
        .flatMap((processState) => processState.entries)
        .at(-1);

      if (
        latestEntry?.type === 'NORMALIZED_ENTRY' &&
        latestEntry.content.entry_type.type === 'tool_use' &&
        latestEntry.content.entry_type.tool_name === 'ExitPlanMode'
      ) {
        modifiedAddEntryType = 'plan';
      }

      onTimelineUpdatedRef.current?.(
        timelineSource,
        modifiedAddEntryType,
        loading || canonicalRef.current.isLoading
      );
    },
    [buildTimelineSource]
  );

  // This emits its own events as they are streamed
  const loadRunningAndEmit = useCallback(
    (executionProcess: ExecutionProcess): Promise<void> => {
      const generation = generationRef.current;
      return new Promise((resolve, reject) => {
        const url = `/api/execution-processes/${executionProcess.id}/raw-logs/ws`;
        const controller = streamJsonPatchEntries<PatchType>(url, {
          onEntries(entries) {
            if (generation !== generationRef.current) return;
            const patchesWithKey = entries.map((entry, index) =>
              patchWithKey(entry, executionProcess.id, index)
            );
            mergeIntoDisplayed((state) => {
              state[executionProcess.id] = {
                executionProcess,
                entries: patchesWithKey,
              };
            });
            emitEntries(displayedExecutionProcesses.current, 'running', false);
          },
          onFinished: () => {
            if (generation !== generationRef.current) return;
            controllersRef.current.delete(controller);
            emitEntries(displayedExecutionProcesses.current, 'running', false);
            controller.close();
            resolve();
          },
          onError: () => {
            controllersRef.current.delete(controller);
            controller.close();
            reject();
          },
        });
        controllersRef.current.add(controller);
      });
    },
    [emitEntries]
  );

  // Sometimes it can take a few seconds for the stream to start, wrap the loadRunningAndEmit method
  const loadRunningAndEmitWithBackoff = useCallback(
    async (executionProcess: ExecutionProcess) => {
      const generation = generationRef.current;
      for (let i = 0; i < 20 && generation === generationRef.current; i++) {
        try {
          await loadRunningAndEmit(executionProcess);
          break;
        } catch (_) {
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
      }
    },
    [loadRunningAndEmit]
  );

  const loadHistoricEntries = useCallback(
    async (maxEntries?: number): Promise<ExecutionProcessStateStore> => {
      const localDisplayedExecutionProcesses: ExecutionProcessStateStore = {};
      const generation = generationRef.current;

      if (!executionProcesses?.current) return localDisplayedExecutionProcesses;

      for (const executionProcess of [
        ...executionProcesses.current,
      ].reverse()) {
        if (isExecutionProcessActive(executionProcess)) continue;

        const entries =
          await loadEntriesForHistoricExecutionProcess(executionProcess);
        if (generation !== generationRef.current) return {};
        const entriesWithKey = entries.map((e, idx) =>
          patchWithKey(e, executionProcess.id, idx)
        );

        localDisplayedExecutionProcesses[executionProcess.id] = {
          executionProcess,
          entries: entriesWithKey,
        };

        if (
          maxEntries != null &&
          flattenEntries(localDisplayedExecutionProcesses).length > maxEntries
        ) {
          break;
        }
      }

      return localDisplayedExecutionProcesses;
    },
    [executionProcesses]
  );

  const loadRemainingEntriesInBatches = useCallback(
    async (batchSize: number): Promise<boolean> => {
      if (!executionProcesses?.current) return false;

      const generation = generationRef.current;
      let anyUpdated = false;
      for (const executionProcess of [
        ...executionProcesses.current,
      ].reverse()) {
        const current = displayedExecutionProcesses.current;
        if (
          current[executionProcess.id] ||
          isExecutionProcessActive(executionProcess)
        )
          continue;

        const entries =
          await loadEntriesForHistoricExecutionProcess(executionProcess);
        if (generation !== generationRef.current) return false;
        const entriesWithKey = entries.map((e, idx) =>
          patchWithKey(e, executionProcess.id, idx)
        );

        mergeIntoDisplayed((state) => {
          state[executionProcess.id] = {
            executionProcess,
            entries: entriesWithKey,
          };
        });

        if (
          flattenEntries(displayedExecutionProcesses.current).length > batchSize
        ) {
          anyUpdated = true;
          break;
        }
        anyUpdated = true;
      }
      return anyUpdated;
    },
    [executionProcesses]
  );

  const ensureProcessVisible = useCallback((p: ExecutionProcess) => {
    mergeIntoDisplayed((state) => {
      if (!state[p.id]) {
        state[p.id] = {
          executionProcess: {
            id: p.id,
            created_at: p.created_at,
            updated_at: p.updated_at,
            executor_action: p.executor_action,
          },
          entries: [],
        };
      }
    });
  }, []);

  const idListKey = useMemo(
    () => executionProcessesRaw?.map((p) => p.id).join(','),
    [executionProcessesRaw]
  );

  const idStatusKey = useMemo(
    () =>
      executionProcessesRaw
        ?.map((p) => `${p.id}:${getExecutionProcessLifecycle(p)}`)
        .join(','),
    [executionProcessesRaw]
  );

  // Clean up entries for processes that have been removed (e.g., after reset)
  useEffect(() => {
    if (isLoading || !isConnected) return;
    const visibleProcessIds = new Set(executionProcessesRaw.map((p) => p.id));
    const displayedIds = Object.keys(displayedExecutionProcesses.current);
    let changed = false;

    for (const id of displayedIds) {
      if (!visibleProcessIds.has(id)) {
        delete displayedExecutionProcesses.current[id];
        changed = true;
      }
    }

    if (changed) {
      emitEntries(displayedExecutionProcesses.current, 'historic', false);
    }
  }, [idListKey, executionProcessesRaw, emitEntries, isLoading, isConnected]);

  useEffect(() => {
    displayedExecutionProcesses.current = {};
    loadedInitialEntries.current = false;
    emittedEmptyInitialRef.current = false;
    streamingProcessIdsRef.current.clear();
    previousActiveMapRef.current.clear();
    canonicalInitializedRef.current = false;
    setIsLoadingHistory(false);
    emitEntries(displayedExecutionProcesses.current, 'initial', true);
    const controllers = controllersRef.current;
    return () => {
      generationRef.current += 1;
      for (const controller of controllers) controller.close();
      controllers.clear();
    };
  }, [scopeKey, emitEntries]);

  // Canonical updates must not reset script loading or issue an initial scroll.
  // Publish the first complete session atomically, then follow only at bottom.
  useEffect(() => {
    if (canonicalSession.isLoading) return;
    const initial = !canonicalInitializedRef.current;
    canonicalInitializedRef.current = true;
    emitEntries(
      displayedExecutionProcesses.current,
      initial ? 'initial' : 'running',
      false
    );
  }, [canonicalSession.conversation, canonicalSession.isLoading, emitEntries]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (loadedInitialEntries.current) return;

      if (isLoading) return;

      if (executionProcesses.current.length === 0) {
        if (emittedEmptyInitialRef.current) return;
        emittedEmptyInitialRef.current = true;
        emitEntries(displayedExecutionProcesses.current, 'initial', false);
        return;
      }

      emittedEmptyInitialRef.current = false;

      const allInitialEntries = await loadHistoricEntries(MIN_INITIAL_ENTRIES);
      if (cancelled) return;
      loadedInitialEntries.current = true;
      mergeIntoDisplayed((state) => {
        Object.assign(state, allInitialEntries);
      });
      emitEntries(displayedExecutionProcesses.current, 'initial', false);

      setIsLoadingHistory(true);
      while (
        !cancelled &&
        (await loadRemainingEntriesInBatches(REMAINING_BATCH_SIZE))
      ) {
        if (cancelled) return;
        emitEntries(displayedExecutionProcesses.current, 'historic', false);
      }
      if (!cancelled) setIsLoadingHistory(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [
    scopeKey,
    idListKey,
    isLoading,
    loadHistoricEntries,
    loadRemainingEntriesInBatches,
    emitEntries,
  ]); // include idListKey so new processes trigger reload

  useEffect(() => {
    const activeProcesses = getActiveScriptProcesses();
    if (activeProcesses.length === 0) return;

    for (const activeProcess of activeProcesses) {
      if (!displayedExecutionProcesses.current[activeProcess.id]) {
        const runningOrInitial =
          Object.keys(displayedExecutionProcesses.current).length > 1
            ? 'running'
            : 'initial';
        ensureProcessVisible(activeProcess);
        emitEntries(
          displayedExecutionProcesses.current,
          runningOrInitial,
          false
        );
      }

      if (!streamingProcessIdsRef.current.has(activeProcess.id)) {
        streamingProcessIdsRef.current.add(activeProcess.id);
        const generation = generationRef.current;
        loadRunningAndEmitWithBackoff(activeProcess).finally(() => {
          if (generation === generationRef.current)
            streamingProcessIdsRef.current.delete(activeProcess.id);
        });
      }
    }
  }, [
    scopeKey,
    idStatusKey,
    emitEntries,
    ensureProcessVisible,
    loadRunningAndEmitWithBackoff,
  ]);

  useEffect(() => {
    // Ordinary coding-agent runs are projected from the canonical AgentRun
    // stream. Only standalone script processes own the legacy raw-log
    // endpoint; iterating the raw provider list here would re-introduce
    // provider transcripts into the canonical conversation after completion.
    if (executionProcesses.current.length === 0) return;

    const processesToReload: ExecutionProcess[] = [];

    for (const process of executionProcesses.current) {
      const previousActive = previousActiveMapRef.current.get(process.id);
      const currentActive = isExecutionProcessActive(process);

      if (
        previousActive === true &&
        !currentActive &&
        displayedExecutionProcesses.current[process.id]
      ) {
        processesToReload.push(process);
      }

      previousActiveMapRef.current.set(process.id, currentActive);
    }

    if (processesToReload.length === 0) return;

    const generation = generationRef.current;
    (async () => {
      let anyUpdated = false;

      for (const process of processesToReload) {
        const entries = await loadEntriesForHistoricExecutionProcess(process);
        if (generation !== generationRef.current) return;
        if (entries.length === 0) continue;

        const entriesWithKey = entries.map((e, idx) =>
          patchWithKey(e, process.id, idx)
        );

        mergeIntoDisplayed((state) => {
          state[process.id] = {
            executionProcess: process,
            entries: entriesWithKey,
          };
        });
        anyUpdated = true;
      }

      if (anyUpdated) {
        emitEntries(displayedExecutionProcesses.current, 'running', false);
      }
    })();
  }, [idStatusKey, executionProcessesRaw, emitEntries]);

  // If an execution process is removed, remove it from the state
  useEffect(() => {
    if (!executionProcessesRaw) return;

    const removedProcessIds = Object.keys(
      displayedExecutionProcesses.current
    ).filter((id) => !executionProcessesRaw.some((p) => p.id === id));

    if (removedProcessIds.length > 0) {
      mergeIntoDisplayed((state) => {
        removedProcessIds.forEach((id) => {
          delete state[id];
        });
      });
    }
  }, [scopeKey, idListKey, executionProcessesRaw]);

  return { isFirstTurn, isLoadingHistory: isLoadingHistoryState };
};
