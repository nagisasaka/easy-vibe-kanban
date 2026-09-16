import { useWorkspaceRepositoryWorkflow } from '@/shared/hooks/useWorkspaceRepositoryWorkflow';
import { WorkflowRunPage } from './WorkflowRunPage';

export function WorkspaceWorkflowPage({
  workspaceId,
}: {
  workspaceId: string;
}) {
  const {
    data: run,
    isPending,
    error,
    refetch,
  } = useWorkspaceRepositoryWorkflow(workspaceId, null);

  if (isPending) {
    return (
      <p className="p-base text-normal" role="status">
        Loading workflow…
      </p>
    );
  }
  if (error || !run) {
    return (
      <div className="space-y-base p-base">
        <p className="text-error" role="alert">
          {error?.message ??
            'No repository workflow exists for this workspace.'}
        </p>
        <button className="text-brand underline" onClick={() => void refetch()}>
          Reload
        </button>
        <a
          className="block text-brand underline"
          href={`/workspaces/${workspaceId}`}
        >
          Open workspace
        </a>
      </div>
    );
  }
  return <WorkflowRunPage key={run.id} runId={run.id} />;
}
