import { useState } from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Link,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router';
import { ReactFlowProvider } from '@xyflow/react';
import {
  useWorkflowTemplate,
  useWorkflowTemplateMutations,
} from '@/shared/hooks/useWorkflowTemplates';
import { useWorkflowEditorDraft } from '@/features/workflow/model/useWorkflowEditorDraft';
import {
  parseWorkflowEditorDocument,
  workflowEditorScope,
} from '@/features/workflow/model/workflowEditorDraft';
import { WorkflowDraftGuard } from '@/features/workflow/ui/WorkflowDraftGuard';
import { WorkflowHistoryControls } from '@/features/workflow/ui/WorkflowHistoryControls';
import { WorkflowCanvas } from '@/features/workflow/ui/WorkflowCanvas';
import { useExecutorConfig } from '@/shared/hooks/useExecutorConfig';
import type { ExecutorConfig } from 'shared/types';

const client = new QueryClient({
  defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
});

function Editor({ id }: { id: string }) {
  const {
    data: template,
    error: fetchError,
    refetch,
  } = useWorkflowTemplate(id);
  const { updateTemplate } = useWorkflowTemplateMutations();
  const readOnly = !!fetchError || template?.source === 'system';
  const editor = useWorkflowEditorDraft(
    workflowEditorScope('local', 'project', id),
    template,
    readOnly
  );
  const [error, setError] = useState('');
  const executor = useExecutorConfig({
    profiles: null,
    lastUsedConfig: null,
    controlled: true,
    scratchConfig: editor.state?.value.graph.nodes.find(
      (node) => node.type === 'agent'
    )?.data.executor_config as ExecutorConfig | null,
    onPersist: (config) =>
      editor.edit((value) => ({
        ...value,
        graph: {
          ...value.graph,
          nodes: value.graph.nodes.map((node) =>
            node.type === 'agent'
              ? { ...node, data: { ...node.data, executor_config: config } }
              : node
          ),
        },
      })),
  });
  const save = async () => {
    const snapshot = editor.beginSave();
    if (!snapshot) return false;
    setError('');
    try {
      const result = await updateTemplate({
        workflowId: id,
        payload: {
          name: snapshot.value.name,
          description: snapshot.value.description,
          graph_json: JSON.stringify(snapshot.value.graph),
          expected_revision: snapshot.revision,
        },
      });
      return editor.acknowledge(
        snapshot,
        parseWorkflowEditorDocument(
          result.name,
          result.description,
          result.graph_json
        ),
        result.revision
      );
    } catch (reason) {
      if (editor.isCurrent()) setError(String(reason));
      return false;
    } finally {
      editor.finishSave(snapshot);
    }
  };
  return (
    <section>
      <button
        disabled={readOnly}
        onClick={() => executor.setOverrides({ reasoning_id: 'ultra' })}
      >
        Set effort ultra
      </button>
      <output data-testid="controlled-executor">
        {JSON.stringify(executor.executorConfig)}
      </output>
      <WorkflowDraftGuard
        dirty={editor.dirty}
        saving={editor.isSaving}
        canSave={!readOnly && !editor.conflict}
        errorMessage={error}
        onSave={save}
        onDiscard={editor.discard}
        hasPendingChanges={editor.hasPendingChanges}
      />
      <label>
        Name
        <input
          aria-label="Draft name"
          disabled={readOnly}
          value={editor.state?.value.name ?? ''}
          onChange={(event) =>
            editor.edit((value) => ({ ...value, name: event.target.value }))
          }
        />
      </label>
      <label>
        Description
        <textarea
          aria-label="Draft description"
          disabled={readOnly}
          value={editor.state?.value.description ?? ''}
          onChange={(event) =>
            editor.edit((value) => ({
              ...value,
              description: event.target.value,
            }))
          }
        />
      </label>
      <button
        disabled={readOnly || editor.isSaving || editor.conflict}
        onClick={() => void save()}
      >
        Save draft
      </button>
      <button onClick={() => void refetch()}>Refetch baseline</button>
      <button disabled={editor.isSaving} onClick={() => editor.discard()}>
        Discard draft
      </button>
      <WorkflowHistoryControls
        canUndo={!readOnly && !!editor.state?.undo.length}
        canRedo={!readOnly && !!editor.state?.redo.length}
        onMove={editor.moveHistory}
      />
      <button
        disabled={readOnly}
        onClick={() =>
          editor.edit((value) => ({
            ...value,
            graph: {
              ...value.graph,
              nodes: value.graph.nodes.map((node) =>
                node.type === 'agent'
                  ? {
                      ...node,
                      data: {
                        ...node.data,
                        prompt_template: 'Changed prompt',
                        include_workflow_context: false,
                        selected_skills: [
                          { name: 'retained', path: '/fixture/skill/SKILL.md' },
                        ],
                      },
                      position: { x: 550, y: 100 },
                    }
                  : node
              ),
            },
          }))
        }
      >
        Edit graph contract
      </button>
      <Link to="/other">Leave editor</Link>
      <output data-testid="draft-state">
        {JSON.stringify({
          id,
          revision: editor.state?.revision,
          dirty: editor.dirty,
          conflict: editor.conflict,
          saving: editor.isSaving,
          readOnly,
          value: editor.state?.value,
        })}
      </output>
      <output role="alert">{error}</output>
      {editor.state && (
        <div style={{ height: 480 }}>
          <ReactFlowProvider>
            <WorkflowCanvas
              graph={editor.state.value.graph}
              readOnly={readOnly}
              onChange={(graph) =>
                editor.edit((value) => ({ ...value, graph }))
              }
              onNodeDrop={() => {}}
            />
          </ReactFlowProvider>
        </div>
      )}
    </section>
  );
}

function EditorRoute() {
  const [id, setId] = useState('a');
  return (
    <div>
      <button onClick={() => setId(id === 'a' ? 'b' : 'a')}>
        Switch target
      </button>
      <Editor key={id} id={id} />
    </div>
  );
}
const root = createRootRoute({ component: Outlet });
const edit = createRoute({
  getParentRoute: () => root,
  path: '/edit',
  component: EditorRoute,
});
const other = createRoute({
  getParentRoute: () => root,
  path: '/other',
  component: () => <p>Other page</p>,
});
const router = createRouter({
  routeTree: root.addChildren([edit, other]),
  history: createMemoryHistory({ initialEntries: ['/edit'] }),
});

export function WorkflowDraftHarness() {
  return (
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
}
