import { useEffect, useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import type { Session, Workspace } from 'shared/types';
import { PrimaryButton } from '@vibe/ui/components/PrimaryButton';
import { useWorkspaceOwnerView } from '@/shared/hooks/useWorkspaceOwner';
import { useWorkspacePendingApproval } from '@/features/workspace-chat/model/hooks/useWorkspacePendingApproval';
import { canonicalAgentControls } from '@/features/workspace-chat/model/canonicalAgentControls';

/** No composer, draft/queue side effects, executor selector or session creation.
 * Existing pending controls remain responses to the owner's current request. */
export function ExecutionInspectionPanel({
  workspace,
  sessions,
  selectedSessionId,
  onSelectSession,
}: {
  workspace: Workspace;
  sessions: Session[];
  selectedSessionId?: string;
  onSelectSession: (id: string) => void;
}) {
  const execution = useWorkspaceOwnerView(workspace.id);
  const pending = useWorkspacePendingApproval();
  const [answer, setAnswer] = useState('');
  useEffect(() => {
    setAnswer('');
  }, [pending?.controlId]);
  const response = useMutation({
    mutationFn: async (approved: boolean) => {
      if (!pending) return;
      if (pending.kind === 'approval') {
        if (approved)
          await canonicalAgentControls.approve(
            pending.agentRunId,
            pending.approvalId
          );
        else
          await canonicalAgentControls.deny(
            pending.agentRunId,
            pending.approvalId,
            answer || undefined
          );
      } else {
        await canonicalAgentControls.submitInput(
          pending.agentRunId,
          pending.inputId,
          answer
        );
      }
      setAnswer('');
    },
  });
  const run = execution.data;
  return (
    <section
      className="border-t border-border bg-secondary p-base flex flex-col gap-half text-sm"
      aria-label="Execution inspection"
    >
      <div className="flex items-center justify-between gap-base">
        <strong className="text-high">Execution-only · Inspection</strong>
        <PrimaryButton
          variant="secondary"
          disabled={!run?.can_stop || execution.stop.isPending}
          onClick={() => execution.stop.mutate()}
        >
          {run?.stop_requested || execution.stop.isPending
            ? 'Stopping…'
            : 'Stop execution'}
        </PrimaryButton>
      </div>
      <p className="text-low">
        The owner controls this environment. Chat, new sessions and development
        actions are disabled. Files are shown from this execution workspace.
      </p>
      <p className="text-normal">
        {run?.status ?? 'Loading execution…'} ·{' '}
        {run?.owner?.kind ?? workspace.execution_owner?.kind ?? 'Unknown owner'}
      </p>
      {run?.owner?.run_id && (
        <p className="font-mono text-xs text-low break-all">
          Run: {run.owner.run_id}
        </p>
      )}
      {run && (
        <p className="text-low">
          {run.owner?.result?.no_op && run.status === 'succeeded'
            ? 'No changes required (successful reconciliation)'
            : run.published
              ? 'Published result'
              : 'Publication not confirmed; files may be unpublished'}
        </p>
      )}
      {run && !run.files_available && (
        <p className="text-warning">
          Files are missing or unprepared. Inspection will not recreate the
          worktree. Saved logs remain available.
        </p>
      )}
      {[
        run?.error,
        execution.error?.message,
        execution.stop.error?.message,
        response.error?.message,
      ]
        .filter(Boolean)
        .map((error) => (
          <p key={error} role="alert" className="text-error break-words">
            {error}
          </p>
        ))}
      <label className="flex items-center gap-base text-normal">
        Session
        <select
          aria-label="Execution session"
          className="bg-primary border border-border rounded-sm p-half min-w-0 flex-1"
          value={selectedSessionId ?? ''}
          onChange={(event) => onSelectSession(event.target.value)}
        >
          {!selectedSessionId && <option value="">Select saved session</option>}
          {sessions.map((session) => (
            <option key={session.id} value={session.id}>
              {session.name ?? session.id}
            </option>
          ))}
        </select>
      </label>
      {pending && (
        <div className="border border-border p-base flex flex-col gap-half">
          <p>
            {pending.kind === 'input'
              ? pending.prompt
              : `Approval requested: ${pending.toolName}`}
          </p>
          <textarea
            key={pending.controlId}
            aria-label="Owner request response"
            className="bg-primary border border-border rounded-sm p-half"
            value={answer}
            onChange={(event) => setAnswer(event.target.value)}
          />
          <div className="flex gap-half">
            <PrimaryButton
              disabled={
                response.isPending ||
                !run ||
                run.status === 'unknown' ||
                (pending.kind === 'input' && !answer.trim())
              }
              onClick={() => response.mutate(true)}
            >
              {pending.kind === 'input' ? 'Respond to request' : 'Approve'}
            </PrimaryButton>
            {pending.kind === 'approval' && (
              <PrimaryButton
                variant="secondary"
                disabled={
                  response.isPending || !run || run.status === 'unknown'
                }
                onClick={() => response.mutate(false)}
              >
                Deny
              </PrimaryButton>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
