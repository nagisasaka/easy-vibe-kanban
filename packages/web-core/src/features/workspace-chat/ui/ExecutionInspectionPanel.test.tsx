import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Session, Workspace, WorkspaceExecutionView } from 'shared/types';
import { ExecutionInspectionPanel } from './ExecutionInspectionPanel';

const fixture = vi.hoisted(() => ({
  data: undefined as WorkspaceExecutionView | undefined,
  pending: null as unknown,
  stop: vi.fn(),
  response: vi.fn(),
}));
vi.mock('@/shared/hooks/useWorkspaceOwner', () => ({
  useWorkspaceOwnerView: () => ({
    data: fixture.data,
    stop: { mutate: fixture.stop, isPending: false },
  }),
}));
vi.mock(
  '@/features/workspace-chat/model/hooks/useWorkspacePendingApproval',
  () => ({ useWorkspacePendingApproval: () => fixture.pending })
);
vi.mock('@/features/workspace-chat/model/canonicalAgentControls', () => ({
  canonicalAgentControls: {},
}));
vi.mock('@tanstack/react-query', () => ({
  useMutation: () => ({ mutate: fixture.response, isPending: false }),
}));
vi.mock('@vibe/ui/components/PrimaryButton', () => ({
  PrimaryButton: ({
    children,
    variant: _variant,
    ...props
  }: {
    children: string;
    variant?: string;
  }) => createElement('button', props, children),
}));
const workspace = {
  id: 'execution',
  usage: 'execution_only',
  execution_owner: { kind: 'future_owner' },
} as Workspace;
function render() {
  return renderToStaticMarkup(
    createElement(ExecutionInspectionPanel, {
      workspace,
      sessions: [{ id: 'session', name: 'Saved log' } as Session],
      selectedSessionId: 'session',
      onSelectSession: vi.fn(),
    })
  );
}

describe('execution inspection (no composer lifecycle)', () => {
  beforeEach(() => {
    fixture.data = undefined;
    fixture.pending = null;
    vi.clearAllMocks();
  });
  it('fails closed while ownership loads and offers existing sessions only', () => {
    const html = render();
    expect(html).toContain('Loading execution');
    expect(html).toContain('disabled=""');
    expect(html).toContain('Saved log');
    expect(html).not.toContain('<textarea');
    expect(html).not.toContain('New session');
    expect(fixture.stop).not.toHaveBeenCalled();
  });
  it('uses the owner outcome, not an Agent success, and never recreates missing files', () => {
    fixture.data = {
      status: 'finalizing',
      can_stop: false,
      terminal: false,
      published: false,
      files_available: false,
    } as WorkspaceExecutionView;
    expect(render()).toContain('Publication not confirmed');
    expect(render()).toContain('will not recreate the worktree');
    fixture.data = {
      ...fixture.data,
      status: 'succeeded',
      terminal: true,
      published: true,
    };
    expect(render()).toContain('Published result');
    expect(render()).toContain('disabled=""');
  });
  it('shows owner cancellation state without enabling arbitrary messages', () => {
    fixture.data = {
      status: 'running',
      can_stop: true,
      terminal: false,
      files_available: true,
    } as WorkspaceExecutionView;
    expect(render()).not.toContain('disabled=""');
    fixture.data = { ...fixture.data, can_stop: false, stop_requested: true };
    expect(render()).toContain('Stopping');
    expect(render()).not.toContain('<textarea');
  });
  it('offers a response only for an outstanding owner request', () => {
    fixture.data = {
      status: 'awaiting_input',
      can_stop: true,
    } as WorkspaceExecutionView;
    fixture.pending = {
      kind: 'input',
      controlId: 'request',
      agentRunId: 'run',
      inputId: 'input',
      prompt: 'Choose a destination',
    };
    expect(render()).toContain('Owner request response');
    expect(render()).toContain('Choose a destination');
    expect(render()).not.toContain('New session');
  });
});
