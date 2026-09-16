import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import type { WorkflowNodeExecutionResponse } from 'shared/types';
import { WorkflowNodeSessionHeader } from './WorkflowNodeSessionPanel';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock('@/features/workspace-files', () => ({}));
vi.mock(
  '@/features/agent-runtime/model/useAgentRunCanonicalStream',
  () => ({})
);

describe('workflow cockpit header', () => {
  it.each(['generate', 'review', 'refine'])(
    'keeps %s identity above wrapping controls',
    (node) => {
      const execution: WorkflowNodeExecutionResponse = {
        id: 'node',
        run_id: 'run',
        node_id: node,
        node_type: 'agent',
        iteration: 0n,
        status: 'running',
        input_text: null,
        output_text: null,
        session_id: 'session',
        orchestration_node_execution_id: null,
        agent_run_id: 'agent',
        projection_status: 'current',
        execution_process_id: null,
        arena_group_id: null,
        tokens_used: null,
        cost_estimate: null,
        started_at: null,
        finished_at: null,
        error_text: null,
        created_at: '',
        updated_at: '',
      };
      const html = renderToStaticMarkup(
        createElement(WorkflowNodeSessionHeader, {
          execution,
          nodeTitle: node,
          sessionHref: '/workspaces/workspace?session_id=session',
          workspaceHref: null,
        })
      );
      expect(html).toContain(`>${node}</h2>`);
      expect(html).toContain('class="flex min-w-0 flex-col gap-base"');
      expect(html).toContain('class="flex min-w-0 flex-wrap gap-half"');
      expect(html).toContain('href="/workspaces/workspace?session_id=session"');
      expect(html).toContain('disabled=""');
    }
  );
});
