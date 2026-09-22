import { describe, expect, it } from 'vitest';
import {
  acknowledgeWorkflowSave,
  captureWorkflowSave,
  createWorkflowEditorDraft,
  editWorkflowDraft,
  encodeWorkflowDraft,
  moveWorkflowHistory,
  restoreWorkflowDraft,
  workflowDraftIsDirty,
  workflowEditorScope,
  WORKFLOW_HISTORY_CODE_UNITS,
  WORKFLOW_HISTORY_LIMIT,
} from './workflowEditorDraft';
import {
  createDefaultWorkflowGraph,
  type WorkflowGraph,
} from './workflowGraph';

function initial() {
  const graph = createDefaultWorkflowGraph();
  graph.nodes.push({
    id: 'agent',
    type: 'agent',
    position: { x: 1, y: 2 },
    data: {
      display_name: 'Agent',
      selected_skills: [{ name: 'review', path: '/test/review/SKILL.md' }],
      include_workflow_context: false,
      executor_config: { executor: 'CODEX', model_reasoning_effort: 'max' },
      output_schema: { type: 'object' },
    } as WorkflowGraph['nodes'][number]['data'],
  });
  return createWorkflowEditorDraft(
    workflowEditorScope('local', 'project', 'workflow'),
    { graph, name: 'Original', description: 'Keep me' },
    3
  );
}

describe('scoped Workflow draft and immutable save acknowledgement', () => {
  it('preserves edits after submit, while merging only assigned Session IDs', () => {
    let state = initial();
    state = editWorkflowDraft(state, { ...state.value, name: 'Submitted' });
    const snapshot = captureWorkflowSave(state);
    state = editWorkflowDraft(state, {
      ...state.value,
      name: 'Newer',
      description: 'Not sent yet',
    });
    const persisted = structuredClone(snapshot.value);
    persisted.graph.nodes.find((node) => node.id === 'agent')!.data.session_id =
      'assigned';
    state = acknowledgeWorkflowSave(state, snapshot, persisted, 4);
    expect(state.value.name).toBe('Newer');
    expect(state.value.description).toBe('Not sent yet');
    expect(state.saved.name).toBe('Submitted');
    expect(
      state.value.graph.nodes.find((node) => node.id === 'agent')!.data
        .session_id
    ).toBe('assigned');
    expect(workflowDraftIsDirty(state)).toBe(true);
    state = moveWorkflowHistory(state, 'undo');
    expect(workflowDraftIsDirty(state)).toBe(false);
    expect(state.value.graph).toEqual(persisted.graph);
    expect(moveWorkflowHistory(state, 'redo').value.name).toBe('Newer');
  });

  it('round-trips every graph field including LVK metadata, edges and positions', () => {
    const state = initial();
    const changed = structuredClone(state.value);
    changed.graph.nodes[0].position = { x: 500, y: 900 };
    changed.graph.edges = [];
    changed.graph.router_executor_config = {
      executor: 'codex',
      model_reasoning_effort: 'ultra',
    };
    let edited = editWorkflowDraft(state, changed);
    edited = moveWorkflowHistory(edited, 'undo');
    expect(edited.value).toEqual(state.value);
    expect(moveWorkflowHistory(edited, 'redo').value).toEqual(changed);
    expect(
      editWorkflowDraft(edited, { ...edited.value, name: 'Fork' }).redo
    ).toEqual([]);
  });

  it('rejects old ACKs and other Workflow/Host identities without clearing history', () => {
    const state = initial();
    const snapshot = captureWorkflowSave(state);
    for (const scope of [
      workflowEditorScope('remote', 'project', 'workflow'),
      workflowEditorScope('local', 'project', 'other'),
    ]) {
      expect(
        acknowledgeWorkflowSave({ ...state, scope }, snapshot, state.value, 4)
          .scope
      ).toBe(scope);
      expect(
        acknowledgeWorkflowSave({ ...state, scope }, snapshot, state.value, 4)
          .revision
      ).toBe(3);
    }
    expect(
      acknowledgeWorkflowSave(
        { ...state, revision: 5 },
        snapshot,
        state.value,
        4
      ).revision
    ).toBe(5);
  });

  it('restores a dirty draft with its original revision and rejects wrong scopes', () => {
    const state = initial();
    const changed = editWorkflowDraft(state, {
      ...state.value,
      name: 'Unsaved',
    });
    const restored = restoreWorkflowDraft(
      encodeWorkflowDraft(changed),
      state.scope
    )!;
    expect(restored.value).toEqual(changed.value);
    expect(restored.revision).toBe(3);
    expect(workflowDraftIsDirty(restored)).toBe(true);
    expect(restored.undo).toEqual([]);
    expect(
      restoreWorkflowDraft(encodeWorkflowDraft(changed), 'other')
    ).toBeNull();
    expect(restoreWorkflowDraft('{}', state.scope)).toBeNull();
  });

  it('bounds both history count and bytes without limiting editable contents', () => {
    let state = initial();
    for (let index = 0; index < 200; index++)
      state = editWorkflowDraft(state, { ...state.value, name: String(index) });
    expect(state.undo).toHaveLength(WORKFLOW_HISTORY_LIMIT);
    state = editWorkflowDraft(state, {
      ...state.value,
      description: 'x'.repeat(WORKFLOW_HISTORY_CODE_UNITS + 1),
    });
    state = editWorkflowDraft(state, { ...state.value, name: 'Large' });
    expect(
      state.undo.reduce((sum, value) => sum + value.length, 0)
    ).toBeLessThanOrEqual(WORKFLOW_HISTORY_CODE_UNITS);
    expect(state.value.description.length).toBe(
      WORKFLOW_HISTORY_CODE_UNITS + 1
    );
  });
});
