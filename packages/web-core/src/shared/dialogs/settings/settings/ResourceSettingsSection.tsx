import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import type { RegisterResource, ResourceOperation } from 'shared/types';
import { PrimaryButton } from '@vibe/ui/components/PrimaryButton';
import { resourceOperationCanCancel } from '@/shared/lib/resourceCoordinationApi';
import { SettingsCard, SettingsInput } from './SettingsComponents';
import {
  useSettingsHost,
  useSettingsMachineClient,
} from './SettingsHostContext';

const emptyResource: RegisterResource = {
  resource_key: '',
  name: '',
  description: '',
  state: '',
};

function mediationExplanation(result: string) {
  try {
    const value: unknown = JSON.parse(result);
    if (
      value &&
      typeof value === 'object' &&
      'explanation' in value &&
      typeof value.explanation === 'string'
    )
      return value.explanation;
  } catch {
    // Failed assessments contain a human-readable error, not a JSON decision.
  }
  return result;
}

export function ResourceSettingsSection() {
  const host = useSettingsHost();
  return <ResourceSettingsContent key={host.selectedHostId ?? 'none'} />;
}

function ResourceSettingsContent() {
  const { t } = useTranslation('settings');
  const client = useSettingsMachineClient();
  const host = useSettingsHost();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState(emptyResource);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [recovery, setRecovery] = useState<ResourceOperation | null>(null);
  const [evidence, setEvidence] = useState('');
  const [states, setStates] = useState<Record<string, string>>({});
  const [revisions, setRevisions] = useState<Record<string, number>>({});
  const queryKey = [
    ...(client?.queryScopeKey ?? ['machine', 'none']),
    'resources',
  ];
  const query = useQuery({
    queryKey,
    enabled: !!client,
    refetchInterval: 3000,
    queryFn: async () => {
      if (!client) throw new Error('Host unavailable');
      const [snapshot, operations, mediations] = await Promise.all([
        client.resources.snapshot(),
        client.resources.operations(),
        client.resources.mediations(),
      ]);
      return { snapshot, operations, mediations };
    },
  });
  async function act(action: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      await queryClient.invalidateQueries({ queryKey });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }
  const disabled = !client || !host.canEdit || busy;
  const data = query.data;
  return (
    <div className="space-y-6">
      <SettingsCard
        title={t('resources.title')}
        description={t('resources.description')}
      >
        <p className="text-sm text-low">{t('resources.scope')}</p>
        {(error || query.error) && (
          <p role="alert" className="text-sm text-error">
            {error ?? String(query.error)}
          </p>
        )}
        {query.isLoading && <p role="status">{t('resources.loading')}</p>}
        <div className="space-y-3">
          {data?.snapshot.resources.map((resource) => {
            const holder = data.snapshot.holders.find(
              (h) => h.resource_id === resource.id
            );
            return (
              <div
                key={resource.id}
                className="rounded-sm border border-border p-3 space-y-1"
              >
                <div className="flex justify-between gap-2">
                  <strong>{resource.name}</strong>
                  <span>
                    {t(`resources.health.${resource.health}`)}
                    {holder ? ` · ${t('resources.occupied')}` : ''}
                  </span>
                </div>
                <code className="text-sm break-all">
                  {resource.resource_key}
                </code>
                <p className="text-sm whitespace-pre-wrap">
                  {resource.description}
                </p>
                <p className="text-sm">
                  {t('resources.revision', {
                    revision: String(resource.revision),
                  })}
                  : {resource.state}
                </p>
                {holder && (
                  <p className="text-xs text-low break-all">
                    {t('resources.owner')}: {holder.operation_id}
                  </p>
                )}
              </div>
            );
          })}
          {data?.snapshot.resources.length === 0 && (
            <p className="text-sm text-low">{t('resources.empty')}</p>
          )}
        </div>
        <form
          className="space-y-3 mt-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (client && !disabled)
              void act(async () => {
                await client.resources.register(draft);
                setDraft(emptyResource);
              });
          }}
        >
          {(['resource_key', 'name', 'description', 'state'] as const).map(
            (field) => (
              <div key={field}>
                <label
                  htmlFor={`resource-${field}`}
                  className="text-sm font-medium"
                >
                  {t(`resources.fields.${field}`)}
                </label>
                <SettingsInput
                  id={`resource-${field}`}
                  value={draft[field]}
                  onChange={(value) =>
                    setDraft((d) => ({ ...d, [field]: value }))
                  }
                  disabled={disabled}
                />
              </div>
            )
          )}
          <PrimaryButton
            type="submit"
            value={t('resources.register')}
            disabled={disabled || Object.values(draft).some((v) => !v.trim())}
          />
        </form>
      </SettingsCard>
      <SettingsCard
        title={t('resources.operations')}
        description={t('resources.operationsHelp')}
      >
        <div className="space-y-3">
          {data?.operations.map((op) => (
            <div
              key={op.id}
              className="border border-border rounded-sm p-3 space-y-2"
            >
              <div className="flex justify-between gap-2">
                <strong className="break-words">{op.spec.purpose}</strong>
                <span>{t(`resources.status.${op.status}`)}</span>
              </div>
              <p className="text-xs text-low break-all">
                {t('resources.workspace')}: {op.workspace_id} · {op.id}
              </p>
              <p className="text-sm">
                {op.spec.claims
                  .map(
                    (c) =>
                      data.snapshot.resources.find(
                        (r) => r.id === c.resource_id
                      )?.name ?? c.resource_id
                  )
                  .join(', ')}
              </p>
              {op.message && (
                <p className="text-sm whitespace-pre-wrap">{op.message}</p>
              )}
              <details>
                <summary className="cursor-pointer text-sm">
                  {t('resources.command')}
                </summary>
                <pre className="text-xs overflow-auto whitespace-pre-wrap">
                  {op.spec.script}
                </pre>
                <p className="text-sm mt-2">{t('resources.verification')}</p>
                <pre className="text-xs overflow-auto whitespace-pre-wrap">
                  {op.spec.verification_script}
                </pre>
                {op.process_id && (
                  <p className="text-xs">
                    {t('resources.process')}: {op.process_id}
                  </p>
                )}
              </details>
              <div className="flex flex-wrap gap-2">
                {resourceOperationCanCancel(op.status) && (
                  <PrimaryButton
                    variant="tertiary"
                    value={t('resources.cancel')}
                    disabled={disabled}
                    onClick={() => {
                      if (client)
                        void act(() => client.resources.cancel(op.id));
                    }}
                  />
                )}
                {['queued', 'blocked', 'recovery_required'].includes(
                  op.status
                ) && (
                  <PrimaryButton
                    variant="tertiary"
                    value={t('resources.mediate')}
                    disabled={
                      disabled ||
                      data.mediations.some((m) =>
                        ['pending', 'preparing', 'running'].includes(m.status)
                      )
                    }
                    onClick={() => {
                      if (client)
                        void act(() => client.resources.mediate(op.id));
                    }}
                  />
                )}
                {op.status === 'recovery_required' && (
                  <PrimaryButton
                    variant="tertiary"
                    value={t('resources.inspectRecovery')}
                    disabled={disabled}
                    onClick={() => {
                      setRecovery(op);
                      setRevisions(
                        Object.fromEntries(
                          data.snapshot.resources.map((r) => [r.id, r.revision])
                        )
                      );
                      setStates(
                        Object.fromEntries(
                          data.snapshot.resources
                            .filter((r) =>
                              op.spec.claims.some((c) => c.resource_id === r.id)
                            )
                            .map((r) => [r.id, r.state])
                        )
                      );
                      setEvidence('');
                    }}
                  />
                )}
              </div>
              {recovery?.id === op.id && (
                <div className="space-y-3 border-t border-border pt-3">
                  <p className="text-sm">{t('resources.recoveryHelp')}</p>
                  {op.spec.claims.map((claim) => (
                    <div key={claim.resource_id}>
                      <label
                        htmlFor={`recover-${claim.resource_id}`}
                        className="text-sm"
                      >
                        {
                          data.snapshot.resources.find(
                            (r) => r.id === claim.resource_id
                          )?.name
                        }
                      </label>
                      <SettingsInput
                        id={`recover-${claim.resource_id}`}
                        value={states[claim.resource_id] ?? ''}
                        onChange={(value) =>
                          setStates((s) => ({
                            ...s,
                            [claim.resource_id]: value,
                          }))
                        }
                      />
                    </div>
                  ))}
                  <label
                    htmlFor="resource-recovery-evidence"
                    className="text-sm"
                  >
                    {t('resources.evidence')}
                  </label>
                  <SettingsInput
                    id="resource-recovery-evidence"
                    value={evidence}
                    onChange={setEvidence}
                  />
                  <PrimaryButton
                    value={t('resources.confirmRecovery')}
                    disabled={
                      disabled ||
                      !evidence.trim() ||
                      Object.values(states).some((s) => !s.trim())
                    }
                    onClick={() => {
                      if (client)
                        void act(async () => {
                          await client.resources.recover(op.id, {
                            evidence,
                            claims: op.spec.claims.map((c) => ({
                              resource_id: c.resource_id,
                              expected_revision: revisions[c.resource_id],
                              resulting_state: states[c.resource_id],
                            })),
                          });
                          setRecovery(null);
                        });
                    }}
                  />
                </div>
              )}
            </div>
          ))}
          {data?.operations.length === 0 && (
            <p className="text-sm text-low">{t('resources.noOperations')}</p>
          )}
        </div>
      </SettingsCard>
      <SettingsCard
        title={t('resources.mediations')}
        description={t('resources.mediationsHelp')}
      >
        {data?.mediations.map((m) => (
          <div key={m.id} className="border-b border-border py-3">
            <p className="text-sm">
              {t(`resources.mediationStatus.${m.status}`)} · {m.created_at}
            </p>
            <p className="text-sm whitespace-pre-wrap break-words">
              {m.result
                ? mediationExplanation(m.result)
                : t('resources.assessing')}
            </p>
            {['pending', 'preparing', 'running'].includes(m.status) && (
              <PrimaryButton
                variant="tertiary"
                value={t('resources.stopMediation')}
                disabled={disabled}
                onClick={() => {
                  if (client)
                    void act(() => client.resources.stopMediation(m.id));
                }}
              />
            )}
          </div>
        ))}
      </SettingsCard>
    </div>
  );
}
