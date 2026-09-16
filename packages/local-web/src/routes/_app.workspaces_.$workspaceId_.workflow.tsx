import { createFileRoute } from '@tanstack/react-router';
import { WorkspaceWorkflowPage } from '@/features/workflow/ui/WorkspaceWorkflowPage';

export const Route = createFileRoute(
  '/_app/workspaces_/$workspaceId_/workflow'
)({
  component: WorkspaceWorkflowRoute,
});

function WorkspaceWorkflowRoute() {
  const { workspaceId } = Route.useParams();
  return <WorkspaceWorkflowPage workspaceId={workspaceId} />;
}
