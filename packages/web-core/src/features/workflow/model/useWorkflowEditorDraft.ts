import { useEffect, useRef, useState } from 'react';
import type { WorkflowTemplateResponse } from 'shared/types';
import {
  acknowledgeWorkflowSave,
  captureWorkflowSave,
  createWorkflowEditorDraft,
  editWorkflowDraft,
  encodeWorkflowDraft,
  moveWorkflowHistory,
  parseWorkflowEditorDocument,
  restoreWorkflowDraft,
  workflowDraftIsDirty,
  type WorkflowEditorDocument,
  type WorkflowEditorDraft,
  type WorkflowSaveSnapshot,
} from './workflowEditorDraft';

const storageKey = (scope: string) => `vibe.workflowEditorDraft.v1.${scope}`;

/** One editor/tab, scoped to the actual API host, project and workflow. */
export function useWorkflowEditorDraft(
  scope: string,
  template: WorkflowTemplateResponse | undefined,
  readOnly: boolean
) {
  const [state, setState] = useState<WorkflowEditorDraft | null>(null);
  const stateRef = useRef(state);
  const mounted = useRef(true);
  const writable = useRef(!readOnly);
  writable.current = !readOnly;
  const [parseError, setParseError] = useState(false);
  const [storageError, setStorageError] = useState(false);
  const [remoteRevision, setRemoteRevision] = useState<number | null>(null);
  const pendingSave = useRef<WorkflowSaveSnapshot | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  const persistDraft = (next: WorkflowEditorDraft) => {
    try {
      if (workflowDraftIsDirty(next))
        window.sessionStorage.setItem(
          storageKey(scope),
          encodeWorkflowDraft(next)
        );
      else window.sessionStorage.removeItem(storageKey(scope));
      setStorageError(false);
    } catch {
      setStorageError(true);
    }
  };
  const publish = (next: WorkflowEditorDraft) => {
    if (!mounted.current || next.scope !== scope) return;
    stateRef.current = next;
    // A save can immediately navigate away before effects run. Persist its
    // acknowledgement synchronously so an obsolete draft cannot reappear.
    persistDraft(next);
    setState(next);
  };

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!template) return;
    try {
      const value = parseWorkflowEditorDocument(
        template.name,
        template.description,
        template.graph_json
      );
      setParseError(false);
      setRemoteRevision(template.revision);
      const current = stateRef.current;
      if (!current || current.scope !== scope) {
        let restored: WorkflowEditorDraft | null = null;
        if (!readOnly) {
          try {
            const raw = window.sessionStorage.getItem(storageKey(scope));
            restored = raw ? restoreWorkflowDraft(raw, scope) : null;
          } catch {
            setStorageError(true);
          }
        }
        const next =
          restored && workflowDraftIsDirty(restored)
            ? restored
            : createWorkflowEditorDraft(scope, value, template.revision);
        stateRef.current = next;
        setState(next);
      } else if (
        !pendingSave.current &&
        !workflowDraftIsDirty(current) &&
        template.revision > current.revision
      ) {
        const next = createWorkflowEditorDraft(scope, value, template.revision);
        stateRef.current = next;
        setState(next);
      }
      // Dirty drafts retain their ORIGINAL revision. A background refresh must
      // never authorise overwriting a different editor's changes.
    } catch {
      setParseError(true);
    }
  }, [scope, template, readOnly]);

  useEffect(() => {
    if (!state || state.scope !== scope || readOnly) return;
    try {
      if (workflowDraftIsDirty(state))
        window.sessionStorage.setItem(
          storageKey(scope),
          encodeWorkflowDraft(state)
        );
      else window.sessionStorage.removeItem(storageKey(scope));
      setStorageError(false);
    } catch {
      setStorageError(true);
    }
  }, [state, scope, readOnly]);

  const edit = (
    update: (current: WorkflowEditorDocument) => WorkflowEditorDocument
  ) => {
    const current = stateRef.current;
    if (!current || current.scope !== scope || !writable.current) return;
    publish(editWorkflowDraft(current, update(current.value)));
  };
  const beginSave = (): WorkflowSaveSnapshot | null => {
    const current = stateRef.current;
    if (
      !mounted.current ||
      !writable.current ||
      !current ||
      pendingSave.current ||
      current.scope !== scope
    )
      return null;
    const snapshot = captureWorkflowSave(current);
    pendingSave.current = snapshot;
    setIsSaving(true);
    return snapshot;
  };
  const acknowledge = (
    snapshot: WorkflowSaveSnapshot,
    persisted: WorkflowEditorDocument,
    revision: number
  ) => {
    if (
      !mounted.current ||
      pendingSave.current !== snapshot ||
      !stateRef.current
    )
      return false;
    const next = acknowledgeWorkflowSave(
      stateRef.current,
      snapshot,
      persisted,
      revision
    );
    publish(next);
    return !workflowDraftIsDirty(next);
  };
  const finishSave = (snapshot: WorkflowSaveSnapshot) => {
    if (pendingSave.current !== snapshot) return;
    pendingSave.current = null;
    if (mounted.current) setIsSaving(false);
  };
  const discard = () => {
    if (!mounted.current || !template || pendingSave.current) return false;
    try {
      const next = createWorkflowEditorDraft(
        scope,
        parseWorkflowEditorDocument(
          template.name,
          template.description,
          template.graph_json
        ),
        template.revision
      );
      window.sessionStorage.removeItem(storageKey(scope));
      publish(next);
      return true;
    } catch {
      setStorageError(true);
      return false;
    }
  };
  const moveHistory = (direction: 'undo' | 'redo') => {
    if (stateRef.current && writable.current)
      publish(moveWorkflowHistory(stateRef.current, direction));
  };

  return {
    state,
    edit,
    beginSave,
    acknowledge,
    finishSave,
    discard,
    moveHistory,
    isSaving,
    parseError,
    storageError,
    dirty: !!state && workflowDraftIsDirty(state),
    isDirty: () => !!stateRef.current && workflowDraftIsDirty(stateRef.current),
    hasPendingChanges: () =>
      pendingSave.current !== null ||
      (!!stateRef.current && workflowDraftIsDirty(stateRef.current)),
    isCurrent: () => mounted.current && stateRef.current?.scope === scope,
    isVersionCurrent: (version: number | undefined) =>
      mounted.current && stateRef.current?.editVersion === version,
    conflict:
      !!state && remoteRevision !== null && remoteRevision > state.revision,
  };
}
