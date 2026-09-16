import {
  useContext,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react';
import type { RepoWithTargetBranch, Workspace } from 'shared/types';
import { createHmrContext } from '@/shared/lib/hmrContext';
import { workspacesApi } from '@/shared/lib/api';
import {
  createWikiViewerController,
  type WikiViewerController,
} from '../model/wikiViewerController';
import {
  wikiRepositoryOptions,
  type WikiRepositoryOption,
} from '../model/wikiNavigation';

interface WikiContext {
  workspace: Workspace | undefined;
  repositoryOptions: WikiRepositoryOption[];
  controller: WikiViewerController;
}

const Context = createHmrContext<WikiContext | null>(
  'WorkspaceWikiContext',
  null
);

export interface WorkspaceWikiProviderProps {
  workspace: Workspace | undefined;
  repos: RepoWithTargetBranch[];
  enabled?: boolean;
  children: ReactNode;
}

export function WorkspaceWikiProvider(props: WorkspaceWikiProviderProps) {
  return (
    <WorkspaceWikiScope
      key={props.workspace?.id ?? 'no-workspace'}
      {...props}
    />
  );
}

function WorkspaceWikiScope({
  workspace,
  repos,
  enabled = true,
  children,
}: WorkspaceWikiProviderProps) {
  const repositoryOptions = useMemo(
    () => (workspace ? wikiRepositoryOptions(workspace, repos) : []),
    [workspace, repos]
  );
  const workspaceId = workspace?.id ?? '';
  const initialRepoId = repositoryOptions[0]?.id ?? '';
  // Keyed by workspace, not repository ordering. A changed source cannot flash
  // old content, while a refreshed repository list preserves valid selection.
  const [controller] = useState(() =>
    createWikiViewerController(workspaceId, initialRepoId, workspacesApi.wiki)
  );

  useEffect(() => {
    const current = controller.getSnapshot().repoId;
    if (!repositoryOptions.some((repo) => repo.id === current)) {
      controller.selectRepository(repositoryOptions[0]?.id ?? '');
    }
  }, [controller, repositoryOptions]);
  useEffect(() => {
    controller.setEnabled(enabled && Boolean(workspaceId));
    return () => controller.setEnabled(false);
  }, [controller, enabled, workspaceId]);
  const value = useMemo(
    () => ({ workspace, repositoryOptions, controller }),
    [workspace, repositoryOptions, controller]
  );
  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useWorkspaceWiki() {
  const context = useContext(Context);
  if (!context) throw new Error('Wiki surfaces require WorkspaceWikiProvider');
  const state = useSyncExternalStore(
    context.controller.subscribe,
    context.controller.getSnapshot,
    context.controller.getSnapshot
  );
  return { ...context, ...state };
}
