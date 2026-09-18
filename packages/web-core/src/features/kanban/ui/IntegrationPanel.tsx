import { useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Button } from '@vibe/ui/components/Button';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@vibe/ui/components/KeyboardDialog';
import { useProjectContext } from '@/shared/hooks/useProjectContext';
import { useAppNavigation } from '@/shared/hooks/useAppNavigation';
import { repoApi } from '@/shared/lib/api';
import { useUserSystem } from '@/shared/hooks/useUserSystem';
import { useExecutorConfig } from '@/shared/hooks/useExecutorConfig';
import { useAgentProviderOptions } from '@/shared/hooks/useAgentProviderPolicy';
import { ModelSelectorContainer } from '@/shared/components/ModelSelectorContainer';
import { SettingsDialog } from '@/shared/dialogs/settings/SettingsDialog';
import {
  AgentProviderCapability,
  ExecutionMode,
  PermissionPolicy,
} from 'shared/types';
import {
  integrationApi,
  integrationCanCancel,
  integrationDoneLabel,
  integrationSelectionIsCurrent,
} from '@/shared/lib/integrationApi';
import type {
  CreateIntegrationRequest,
  IntegrationSelection,
} from 'shared/types';

/** Local Board only. Selection is deliberate, including the adopted Workspace;
 * network retries retain their request ID and cannot substitute another set. */
export function IntegrationPanel({
  focusedRunId,
  onClearFocus,
}: {
  focusedRunId: string | null;
  onClearFocus: () => void;
}) {
  const { projectId, issues } = useProjectContext();
  const navigation = useAppNavigation();
  const client = useQueryClient();
  const [panelOpen, setOpen] = useState(false);
  const open = panelOpen || focusedRunId !== null;
  const [repositoryId, setRepositoryId] = useState('');
  const [target, setTarget] = useState('');
  const [selected, setSelected] = useState<
    Record<string, IntegrationSelection>
  >({});
  const [error, setError] = useState<string | null>(null);
  const pendingRequest = useRef<CreateIntegrationRequest | null>(null);
  const { profiles, config } = useUserSystem();
  const agent = useExecutorConfig({
    profiles,
    lastUsedConfig: null,
    configExecutorProfile: config?.executor_profile,
    hiddenAgents: config?.hidden_agents,
    onPersist: () => {
      pendingRequest.current = null;
    },
  });
  const { options: providerOptions } = useAgentProviderOptions({
    executors: agent.executorOptions,
    preserveExecutors: [agent.effectiveExecutor],
    requiredCapabilities: [
      AgentProviderCapability.INITIAL_RUN,
      AgentProviderCapability.WORKFLOW_AGENT_STEP,
    ],
  });
  const agentReady =
    !!agent.executorConfig &&
    providerOptions.some(
      (p) => p.executor === agent.effectiveExecutor && p.enabled
    ) &&
    (!agent.executorConfig.execution_mode ||
      agent.executorConfig.execution_mode === ExecutionMode.code) &&
    agent.executorConfig.permission_policy !== PermissionPolicy.PLAN;
  const queryKey = ['integrations', projectId];
  const repos = useQuery({
    queryKey: ['integration-repos', projectId],
    queryFn: () => integrationApi.repositories(projectId),
    enabled: open,
  });
  const branches = useQuery({
    queryKey: ['integration-branches', repositoryId],
    queryFn: () => repoApi.getBranches(repositoryId),
    enabled: open && !!repositoryId,
  });
  const activities = useQuery({
    queryKey: ['integration-activities', repositoryId],
    queryFn: () => integrationApi.activities(repositoryId),
    enabled: open && !!repositoryId,
    refetchInterval: open ? 3000 : false,
  });
  const runs = useQuery({
    queryKey,
    queryFn: () => integrationApi.list(projectId),
    enabled: open,
    refetchInterval: open ? 1500 : false,
  });
  const refresh = () => client.invalidateQueries({ queryKey });
  const create = useMutation({
    mutationFn: integrationApi.create,
    onSuccess: () => {
      pendingRequest.current = null;
      setSelected({});
      setError(null);
      void refresh();
    },
    onError: (error: Error) => setError(error.message),
  });
  const action = useMutation({
    mutationFn: ({ id, recover }: { id: string; recover: boolean }) =>
      recover ? integrationApi.recover(id) : integrationApi.cancel(id),
    onSuccess: () => {
      setError(null);
      void refresh();
    },
    onError: (error: Error) => setError(error.message),
  });
  const resetRequest = () => {
    pendingRequest.current = null;
    setError(null);
  };
  const start = () => {
    const request = pendingRequest.current ?? {
      request_id: crypto.randomUUID(),
      project_id: projectId,
      repository_id: repositoryId,
      target_branch: target,
      selections: Object.values(selected),
      executor_config: agent.executorConfig,
    };
    pendingRequest.current = request;
    create.mutate(request);
  };
  const validSelection = integrationSelectionIsCurrent(
    Object.values(selected),
    activities.data ?? []
  );
  const dataError =
    error ??
    repos.error?.message ??
    branches.error?.message ??
    activities.error?.message ??
    runs.error?.message;

  return (
    <>
      <Button size="sm" onClick={() => setOpen(true)}>
        Integrate / Auto Merge
      </Button>
      <Dialog
        open={open}
        onOpenChange={(value) => {
          setOpen(value);
          if (!value) onClearFocus();
        }}
      >
        <DialogContent className="max-w-4xl max-h-[90vh] flex flex-col overflow-hidden bg-primary text-normal">
          <DialogHeader>
            <DialogTitle>Formal Integration</DialogTitle>
            <DialogDescription>
              Select Cards and one adopted Workspace each. EVK creates one
              integration Workspace, verifies its final commit, then updates the
              local target and conditionally marks the Cards Done. No push or
              automatic PR.
            </DialogDescription>
          </DialogHeader>
          <div className="overflow-y-auto space-y-base min-h-0">
            {dataError && (
              <p role="alert" className="text-error whitespace-pre-wrap">
                {dataError}
              </p>
            )}
            <p className="text-low">
              Sources are frozen on submission. The target base is captured when
              this queued run starts. Cooperative local execution is not
              OS-level isolation. Wiki reconciliation is separate from code
              publication.
            </p>
            <div className="flex gap-base">
              <label className="flex-1">
                Repository
                <select
                  aria-label="Integration repository"
                  className="block w-full bg-secondary border p-half"
                  value={repositoryId}
                  disabled={create.isPending}
                  onChange={(e) => {
                    setRepositoryId(e.target.value);
                    setTarget('');
                    setSelected({});
                    resetRequest();
                  }}
                >
                  <option value="">Select repository</option>
                  {repos.data?.map((repo) => (
                    <option key={repo.id} value={repo.id}>
                      {repo.display_name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="flex-1">
                Local target branch
                <select
                  aria-label="Integration target branch"
                  className="block w-full bg-secondary border p-half"
                  value={target}
                  disabled={create.isPending}
                  onChange={(e) => {
                    setTarget(e.target.value);
                    resetRequest();
                  }}
                >
                  <option value="">Select target explicitly</option>
                  {branches.data
                    ?.filter((b) => !b.is_remote)
                    .map((branch) => (
                      <option key={branch.name} value={branch.name}>
                        {branch.name}
                      </option>
                    ))}
                </select>
              </label>
            </div>
            <fieldset
              disabled={create.isPending}
              className="flex gap-base items-center"
            >
              <label>
                Integration agent
                <select
                  aria-label="Integration agent"
                  className="block bg-secondary border p-half"
                  value={agent.effectiveExecutor ?? ''}
                  onChange={(e) => {
                    const option = providerOptions.find(
                      (p) => p.executor === e.target.value
                    );
                    if (option?.enabled) agent.setExecutor(option.executor);
                    resetRequest();
                  }}
                >
                  <option value="">Select agent</option>
                  {providerOptions.map((option) => (
                    <option
                      key={option.executor}
                      value={option.executor}
                      disabled={!option.enabled}
                    >
                      {option.executor}
                    </option>
                  ))}
                </select>
              </label>
              <ModelSelectorContainer
                agent={agent.effectiveExecutor}
                workspaceId={undefined}
                onAdvancedSettings={() =>
                  SettingsDialog.show({ initialSection: 'agents' })
                }
                presets={agent.variantOptions}
                selectedPreset={agent.selectedVariant}
                onPresetSelect={(value) => {
                  agent.setVariant(value);
                  resetRequest();
                }}
                onOverrideChange={(value) => {
                  agent.setOverrides(value);
                  resetRequest();
                }}
                executorConfig={agent.executorConfig}
                presetOptions={agent.presetOptions}
              />
              {!agentReady && (
                <span className="text-low">
                  Choose an available agent in Code mode. Goal and Plan are not
                  formal Integration modes.
                </span>
              )}
            </fieldset>
            <fieldset disabled={create.isPending} className="space-y-half">
              <legend>Cards and adopted Workspaces</legend>
              {issues.map((issue) => {
                const candidates =
                  activities.data?.filter(
                    (a) => a.current_issue_id === issue.id
                  ) ?? [];
                if (!candidates.length) return null;
                return (
                  <div
                    key={issue.id}
                    className="border p-half flex items-center gap-base"
                  >
                    <label className="flex-1">
                      <input
                        type="checkbox"
                        checked={issue.id in selected}
                        onChange={(e) => {
                          setSelected((old) => {
                            const next = { ...old };
                            if (e.target.checked)
                              next[issue.id] = {
                                card_id: issue.id,
                                workspace_id: '',
                                expected_commit: '',
                              };
                            else delete next[issue.id];
                            return next;
                          });
                          resetRequest();
                        }}
                      />{' '}
                      {issue.simple_id}: {issue.title}
                    </label>
                    <select
                      aria-label={`Adopted Workspace for ${issue.title}`}
                      className="max-w-[55%] bg-secondary border p-half"
                      disabled={!(issue.id in selected)}
                      value={selected[issue.id]?.workspace_id ?? ''}
                      onChange={(e) => {
                        setSelected((old) => ({
                          ...old,
                          [issue.id]: {
                            card_id: issue.id,
                            workspace_id: e.target.value,
                            expected_commit:
                              candidates.find(
                                (a) => a.workspace_id === e.target.value
                              )?.observed_head_oid ?? '',
                          },
                        }));
                        resetRequest();
                      }}
                    >
                      <option value="">Choose Workspace</option>
                      {candidates.map((ws) => (
                        <option
                          key={ws.workspace_id}
                          value={ws.workspace_id}
                          disabled={
                            !ws.observed_head_oid ||
                            Number(ws.active_agent_runs) > 0 ||
                            Number(ws.active_scripts) > 0
                          }
                        >
                          {ws.name ?? ws.branch} ·{' '}
                          {ws.observed_head_oid?.slice(0, 10) ?? 'unknown HEAD'}
                          {Number(ws.active_agent_runs) ||
                          Number(ws.active_scripts)
                            ? ' (busy)'
                            : ''}
                        </option>
                      ))}
                    </select>
                    {selected[issue.id]?.expected_commit &&
                      !integrationSelectionIsCurrent(
                        [selected[issue.id]],
                        candidates
                      ) && (
                        <span className="text-error">
                          Selection changed or busy; reselect the Workspace.
                        </span>
                      )}
                  </div>
                );
              })}
            </fieldset>
            <Button
              onClick={start}
              disabled={
                !repositoryId ||
                !target ||
                !validSelection ||
                !agentReady ||
                create.isPending
              }
            >
              {create.isPending ? 'Reserving…' : 'Start selected Integration'}
            </Button>
            <h3 className="text-lg">Integration runs</h3>
            {[...(runs.data ?? [])]
              .sort(
                (a, b) =>
                  Number(b.id === focusedRunId) - Number(a.id === focusedRunId)
              )
              .map((run) => (
                <section
                  key={run.id}
                  className="border bg-panel p-base space-y-half"
                  aria-label={`Integration ${run.id}`}
                >
                  <div className="flex items-center gap-base">
                    <strong>{run.status}</strong>
                    <code>{run.target_ref}</code>
                    <span className="text-low">{run.created_at}</span>
                  </div>
                  <p className="text-low break-all">Run {run.id}</p>
                  {run.error && (
                    <p role="alert" className="text-error whitespace-pre-wrap">
                      {run.error}
                    </p>
                  )}
                  {run.payload.semantic_summary && (
                    <div>
                      <p className="text-low">
                        Agent proposal (before host validation)
                      </p>
                      <p>{run.payload.semantic_summary}</p>
                    </div>
                  )}
                  <p className="break-all">
                    B: {run.payload.base_commit ?? 'waiting for target'} → R:{' '}
                    {run.payload.result_commit ?? 'not validated'}
                  </p>
                  <p>
                    Git:{' '}
                    {run.payload.published
                      ? 'published'
                      : run.payload.publication_intent
                        ? 'reconciling publication'
                        : 'not published'}{' '}
                    · Wiki: {run.payload.wiki_result ?? 'not scheduled'}
                  </p>
                  <ul>
                    {run.payload.sources.map((source) => (
                      <li key={source.selection.card_id}>
                        {source.title}: {source.commit.slice(0, 10)} ·{' '}
                        {integrationDoneLabel(
                          run.status,
                          run.payload.published,
                          source.done_result
                        )}
                      </li>
                    ))}
                  </ul>
                  {run.payload.validation.length > 0 && (
                    <details>
                      <summary>Host validation evidence</summary>
                      <ul>
                        {run.payload.validation.map((v, i) => (
                          <li key={i} className="break-all">
                            {v.command} ({v.cwd}) —{' '}
                            {v.required ? 'required' : 'optional'}:{' '}
                            {v.result ?? 'not executed'}, exit{' '}
                            {v.exit_code == null ? '—' : String(v.exit_code)},
                            process {v.execution_process_id ?? '—'};{' '}
                            {v.evidence}
                          </li>
                        ))}
                      </ul>
                    </details>
                  )}
                  <div className="flex gap-base">
                    {run.workspace_id && (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => {
                          setOpen(false);
                          navigation.goToWorkspace(run.workspace_id!);
                        }}
                      >
                        Open integration Workspace
                      </Button>
                    )}
                    {integrationCanCancel(run.status) && (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={action.isPending || run.cancel_requested}
                        onClick={() =>
                          action.mutate({ id: run.id, recover: false })
                        }
                      >
                        {run.cancel_requested
                          ? 'Stopping…'
                          : 'Cancel Integration'}
                      </Button>
                    )}
                    {['recovery_required', 'post_processing'].includes(
                      run.status
                    ) && (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={action.isPending}
                        onClick={() =>
                          action.mutate({ id: run.id, recover: true })
                        }
                      >
                        Reconcile publication (no remerge)
                      </Button>
                    )}
                  </div>
                </section>
              ))}
            {!runs.data?.length && (
              <p className="text-low">No formal Integration runs.</p>
            )}
          </div>
        </DialogContent>
      </Dialog>
    </>
  );
}
