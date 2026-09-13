import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { Repo } from 'shared/types';
import type { MachineClient } from '@/shared/lib/machineClient';
import { PrimaryButton } from '@vibe/ui/components/PrimaryButton';
import {
  SettingsCard,
  SettingsField,
  SettingsInput,
  SettingsCheckbox,
} from './SettingsComponents';

export function RepositoryMemorySettings({
  repo,
  client,
}: {
  repo: Repo;
  client: MachineClient;
}) {
  const query = useQuery({
    queryKey: [...client.queryScopeKey, 'repository-memory', repo.id],
    queryFn: () => client.getRepositoryMemory(repo.id),
    refetchInterval: 5000,
  });
  const [enabled, setEnabled] = useState(false);
  const [branch, setBranch] = useState(repo.default_target_branch ?? 'main');
  const [language, setLanguage] = useState('en');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const state = query.data;
  const savedEnabled = state?.enabled;
  const savedBranch = state?.target_branch;
  const savedLanguage = state?.output_language;
  useEffect(() => {
    if (savedEnabled !== undefined && savedLanguage !== undefined) {
      setEnabled(savedEnabled);
      setBranch(savedBranch ?? repo.default_target_branch ?? 'main');
      setLanguage(savedLanguage);
    }
  }, [
    savedEnabled,
    savedBranch,
    savedLanguage,
    repo.id,
    repo.default_target_branch,
  ]);

  const perform = async (sync: boolean) => {
    setBusy(true);
    setError(null);
    try {
      if (sync) await client.syncRepositoryMemory(repo.id);
      else
        await client.configureRepositoryMemory(repo.id, {
          enabled,
          target_branch: branch,
          output_language: language,
        });
      await query.refetch();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      await query.refetch();
    } finally {
      setBusy(false);
    }
  };
  const active = state?.active_run_id != null || state?.bootstrap != null;
  return (
    <SettingsCard
      title="OpenWiki repository memory"
      description="Canonical openwiki/ is maintained against the integrated branch in a dedicated workspace. Initial generation receives an independent read-only review and optional refinement before publication. Coding workspaces only read it and keep their own shared Workspace Memory."
    >
      <p className="text-sm text-normal" role="status">
        Status: {state?.status ?? 'Loading…'}
        {state?.bootstrap && ` · ${state.bootstrap.phase}`}
      </p>
      <SettingsCheckbox
        id={`openwiki-enabled-${repo.id}`}
        label="Enable repository memory"
        checked={enabled}
        onChange={setEnabled}
        disabled={busy || active}
      />
      <SettingsField label="Integrated local branch">
        <SettingsInput
          value={branch}
          onChange={setBranch}
          disabled={busy || active}
        />
      </SettingsField>
      <SettingsField label="Output language (BCP 47)">
        <SettingsInput
          value={language}
          onChange={setLanguage}
          placeholder="en, ja, fr, zh-Hans"
          disabled={busy || active}
        />
      </SettingsField>
      <div className="flex gap-base">
        <PrimaryButton
          value="Save settings"
          disabled={busy || active || !state}
          onClick={() => void perform(false)}
        />
        <PrimaryButton
          value={state?.last_success ? 'Sync Wiki' : 'Initialize Wiki'}
          disabled={busy || active || !state?.enabled}
          onClick={() => void perform(true)}
        />
      </div>
      <p className="text-sm text-low">
        Last successful reconciliation:{' '}
        {state?.last_success
          ? new Date(state.last_success).toLocaleString()
          : 'None'}
      </p>
      {state?.source_commit && (
        <p className="break-all text-sm text-low">
          Integrated source: {state.source_commit}
        </p>
      )}
      {state?.maintenance_workspace_id && (
        <p className="break-all text-sm text-low">
          Maintenance workspace: {state.maintenance_workspace_id}. Open it in
          Workspaces to inspect Codex output or stop the run.
        </p>
      )}
      {(error || query.error || state?.error) && (
        <p className="whitespace-pre-wrap text-sm text-error" role="alert">
          {error ?? query.error?.message ?? state?.error}
        </p>
      )}
      {state?.coding_errors?.map((message) => (
        <p
          key={message}
          className="whitespace-pre-wrap text-sm text-error"
          role="alert"
        >
          Source checkpoint needs attention: {message}. Source files are
          retained; fix the draft or unexpected Wiki edits before merging or
          pushing.
        </p>
      ))}
      {state?.status === 'error' && (
        <p className="text-sm text-low">
          Source changes are retained. Inspect the error and retry Sync Wiki
          after resolving it; failed semantic events remain pending.
        </p>
      )}
    </SettingsCard>
  );
}
