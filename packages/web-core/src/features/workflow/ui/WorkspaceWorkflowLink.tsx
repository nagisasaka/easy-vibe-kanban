import { useWorkspaceRepositoryWorkflow } from '@/shared/hooks/useWorkspaceRepositoryWorkflow';

export function WorkspaceWorkflowLink({
  workspaceId,
  hostId,
}: {
  workspaceId: string;
  hostId: string | null;
}) {
  const { data: run, error } = useWorkspaceRepositoryWorkflow(
    workspaceId,
    hostId
  );
  if (hostId !== null) return null;
  if (error) {
    return (
      <p className="p-half text-xs text-error" role="alert">
        Workflow status unavailable: {error.message}
      </p>
    );
  }
  if (!run) return null;
  return (
    <a
      href={`/workspaces/${workspaceId}/workflow`}
      className="block border-b border-secondary bg-panel p-half text-sm text-brand underline"
    >
      View Workflow · {run.status}
    </a>
  );
}
