import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import {
  AgentRuntimeToolStatus,
  type AgentEventEnvelope,
  type RunState,
} from 'shared/types';
import { ToolSummaryEntry } from '@/features/workspace-chat/ui/ToolSummaryEntry';
import {
  emptyCanonicalAgentTimeline,
  mergeAgentLiveEvent,
  mergeCanonicalAgentTimeline,
} from '@/features/agent-runtime/model/canonicalAgentTimeline';
import { buildCanonicalAgentSessionTimeline } from '@/features/agent-runtime/model/canonicalAgentSessionTimeline';
import { projectCanonicalAgentConversation } from '@/features/agent-runtime/model/canonicalAgentConversation';

const state: RunState = {
  state_schema_version: 1,
  reducer_version: 1,
  session_id: 'shell-test',
  agent_run_id: 'shell-test',
  turn_id: 'shell-test',
  status: 'running',
  projection_status: 'current',
  last_run_attempt_id: 'shell-test',
  last_run_attempt_number: 1,
  last_event_sequence: 1n,
  last_event_id: null,
  provider_session: null,
  terminal_output: null,
  last_error: null,
  unknown_event_count: 0n,
  updated_at: '2026-10-08T00:00:00Z',
};
const command = 'printf first; sleep 1; printf second';
const started: AgentEventEnvelope = {
  schema_version: 1,
  payload_version: 1,
  event_id: 'shell-start',
  session_id: 'shell-test',
  agent_run_id: 'shell-test',
  turn_id: 'shell-test',
  run_attempt_id: 'shell-test',
  run_attempt_number: 1,
  sequence: 1n,
  correlation_id: 'shell-test',
  timestamp: state.updated_at,
  native_refs: [],
  payload: {
    type: 'tool_call',
    data: {
      tool_call_id: 'shell-command',
      tool_name: 'Shell',
      status: AgentRuntimeToolStatus.running,
      arguments: { command },
      result: null,
    },
  },
};

function ShellOutputHarness() {
  const [timeline, setTimeline] = useState(() =>
    mergeCanonicalAgentTimeline(emptyCanonicalAgentTimeline(), [started], state)
  );
  const projection = projectCanonicalAgentConversation(
    buildCanonicalAgentSessionTimeline(
      'shell-test',
      [
        {
          agent_run_id: 'shell-test',
          session_id: 'shell-test',
          turn_id: 'shell-test',
          state,
          created_at: state.updated_at,
          updated_at: state.updated_at,
        },
      ],
      new Map([['shell-test', timeline]])
    )
  );
  const entry = projection.entries[0];
  if (
    entry.type !== 'NORMALIZED_ENTRY' ||
    entry.content.entry_type.type !== 'tool_use'
  )
    throw new Error('Missing shell');
  const tool = entry.content.entry_type;
  if (tool.action_type.action !== 'command_run')
    throw new Error('Shell command was not projected');
  const action = tool.action_type;
  const append = (sequence: number, delta: string) =>
    setTimeline((previous) =>
      mergeAgentLiveEvent(previous, {
        schema_version: 1,
        event_id: `shell-delta-${sequence}`,
        session_id: 'shell-test',
        agent_run_id: 'shell-test',
        turn_id: 'shell-test',
        run_attempt_id: 'shell-test',
        run_attempt_number: 1,
        native_sequence: sequence,
        timestamp: state.updated_at,
        payload: {
          type: 'tool_output_delta',
          data: { provider_item_id: 'shell-command', delta },
        },
      })
    );
  return (
    <section aria-label="Shell regression">
      <button onClick={() => append(2, 'first\n')}>First output</button>
      <button onClick={() => append(3, 'second\n')}>Second output</button>
      <button
        onClick={() =>
          setTimeline((previous) =>
            mergeCanonicalAgentTimeline(previous, [
              {
                ...started,
                event_id: 'shell-complete',
                sequence: 4n,
                payload: {
                  type: 'tool_call',
                  data: {
                    tool_call_id: 'shell-command',
                    tool_name: 'Shell',
                    status: AgentRuntimeToolStatus.succeeded,
                    arguments: { command },
                    result: {
                      aggregatedOutput: 'first\nsecond\n',
                      exitCode: 0,
                    },
                  },
                },
              },
            ])
          )
        }
      >
        Complete shell
      </button>
      <ToolSummaryEntry
        summary={action.command}
        expansionKey={entry.patchKey}
        status={tool.status}
        active={entry.canonical?.active ?? false}
        content={action.result?.output ?? ''}
        toolName={tool.tool_name}
        command={action.command}
        actionType={action.action}
        startedAt={null}
        endedAt={null}
      />
    </section>
  );
}

// Loaded only by the development E2E test; never registered as an app route.
export function mount() {
  const host = document.createElement('div');
  host.className = 'new-design';
  host.style.cssText =
    'position:fixed;inset:40px;z-index:99999;background:white;padding:24px';
  document.body.append(host);
  const root = createRoot(host);
  root.render(<ShellOutputHarness />);
  return () => {
    root.unmount();
    host.remove();
  };
}
