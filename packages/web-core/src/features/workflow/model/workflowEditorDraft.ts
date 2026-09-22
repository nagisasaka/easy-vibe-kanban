import { migrateWorkflowGraph, type WorkflowGraph } from './workflowGraph';

export interface WorkflowEditorDocument {
  graph: WorkflowGraph;
  name: string;
  description: string;
}

export interface WorkflowEditorDraft {
  scope: string;
  value: WorkflowEditorDocument;
  saved: WorkflowEditorDocument;
  revision: number;
  editVersion: number;
  undo: string[];
  redo: string[];
}

export interface WorkflowSaveSnapshot {
  scope: string;
  value: WorkflowEditorDocument;
  revision: number;
  editVersion: number;
}

export const WORKFLOW_HISTORY_LIMIT = 64;
// Bound retained history, not the editable graph or the provider prompt.
export const WORKFLOW_HISTORY_CODE_UNITS = 2 * 1024 * 1024;
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const equal = (left: unknown, right: unknown) =>
  JSON.stringify(left) === JSON.stringify(right);

export function workflowEditorScope(
  host: string,
  project: string,
  workflow: string
) {
  return JSON.stringify([host, project, workflow]);
}

export function createWorkflowEditorDraft(
  scope: string,
  value: WorkflowEditorDocument,
  revision: number
): WorkflowEditorDraft {
  return {
    scope,
    value: clone(value),
    saved: clone(value),
    revision,
    editVersion: 0,
    undo: [],
    redo: [],
  };
}

export function workflowDraftIsDirty(state: WorkflowEditorDraft) {
  return !equal(state.value, state.saved);
}

function boundedHistory(entries: string[]) {
  let size = 0;
  const result: string[] = [];
  for (
    let index = entries.length - 1;
    index >= 0 && result.length < WORKFLOW_HISTORY_LIMIT;
    index--
  ) {
    size += entries[index].length;
    if (size > WORKFLOW_HISTORY_CODE_UNITS) break;
    result.unshift(entries[index]);
  }
  return result;
}

export function editWorkflowDraft(
  state: WorkflowEditorDraft,
  value: WorkflowEditorDocument
): WorkflowEditorDraft {
  if (equal(value, state.value)) return state;
  return {
    ...state,
    value: clone(value),
    editVersion: state.editVersion + 1,
    undo: boundedHistory([...state.undo, JSON.stringify(state.value)]),
    redo: [],
  };
}

export function moveWorkflowHistory(
  state: WorkflowEditorDraft,
  direction: 'undo' | 'redo'
): WorkflowEditorDraft {
  const stack = state[direction];
  if (!stack.length) return state;
  const reverse = direction === 'undo' ? 'redo' : 'undo';
  return {
    ...state,
    value: JSON.parse(stack.at(-1)!) as WorkflowEditorDocument,
    editVersion: state.editVersion + 1,
    [direction]: stack.slice(0, -1),
    [reverse]: boundedHistory([...state[reverse], JSON.stringify(state.value)]),
  };
}

export function captureWorkflowSave(
  state: WorkflowEditorDraft
): WorkflowSaveSnapshot {
  return {
    scope: state.scope,
    value: clone(state.value),
    revision: state.revision,
    editVersion: state.editVersion,
  };
}

function applyAssignedSessions(
  value: WorkflowEditorDocument,
  submitted: WorkflowEditorDocument,
  persisted: WorkflowEditorDocument
): WorkflowEditorDocument {
  // The server may create node Sessions while saving. Merge only these assigned
  // identities into newer edits/history, never a whole ACK graph over the draft.
  return {
    ...value,
    graph: {
      ...value.graph,
      nodes: value.graph.nodes.map((node) => {
        const before = submitted.graph.nodes.find(
          (item) => item.id === node.id && item.type === node.type
        );
        const after = persisted.graph.nodes.find(
          (item) => item.id === node.id && item.type === node.type
        );
        if (
          !before ||
          !after?.data.session_id ||
          node.data.session_id !== before.data.session_id
        )
          return node;
        return {
          ...node,
          data: { ...node.data, session_id: after.data.session_id },
        };
      }),
    },
  };
}

export function acknowledgeWorkflowSave(
  state: WorkflowEditorDraft,
  snapshot: WorkflowSaveSnapshot,
  persisted: WorkflowEditorDocument,
  revision: number
): WorkflowEditorDraft {
  if (
    snapshot.scope !== state.scope ||
    snapshot.revision !== state.revision ||
    revision < state.revision
  )
    return state;
  const merge = (value: WorkflowEditorDocument) =>
    applyAssignedSessions(value, snapshot.value, persisted);
  return {
    ...state,
    revision,
    saved: clone(persisted),
    value:
      state.editVersion === snapshot.editVersion
        ? clone(persisted)
        : merge(state.value),
    undo: boundedHistory(
      state.undo.map((entry) => JSON.stringify(merge(JSON.parse(entry))))
    ),
    redo: boundedHistory(
      state.redo.map((entry) => JSON.stringify(merge(JSON.parse(entry))))
    ),
  };
}

export function parseWorkflowEditorDocument(
  name: string,
  description: string | null,
  graphJson: string
): WorkflowEditorDocument {
  const graph = JSON.parse(graphJson) as WorkflowGraph;
  if (
    !graph ||
    !graph.version ||
    !Array.isArray(graph.nodes) ||
    !Array.isArray(graph.edges)
  )
    throw new Error('Invalid workflow graph');
  return {
    name,
    description: description ?? '',
    graph: migrateWorkflowGraph(graph),
  };
}

export function encodeWorkflowDraft(state: WorkflowEditorDraft) {
  // History is bounded in memory and never persisted. Only this tab's draft and
  // its original revision survive a reload; no cross-editor blind overwrite.
  return JSON.stringify({
    version: 1,
    scope: state.scope,
    value: state.value,
    saved: state.saved,
    revision: state.revision,
  });
}

export function restoreWorkflowDraft(
  raw: string,
  scope: string
): WorkflowEditorDraft | null {
  try {
    const parsed = JSON.parse(raw);
    if (
      parsed.version !== 1 ||
      parsed.scope !== scope ||
      !Number.isSafeInteger(parsed.revision) ||
      parsed.revision < 0
    )
      return null;
    const read = (value: WorkflowEditorDocument) => {
      if (
        !value ||
        typeof value.name !== 'string' ||
        typeof value.description !== 'string'
      )
        throw new Error('Invalid draft');
      return parseWorkflowEditorDocument(
        value.name,
        value.description,
        JSON.stringify(value.graph)
      );
    };
    const state = createWorkflowEditorDraft(
      scope,
      read(parsed.value),
      parsed.revision
    );
    return { ...state, saved: read(parsed.saved) };
  } catch {
    return null;
  }
}
