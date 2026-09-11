import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  CheckIcon,
  FastForwardIcon,
  GearIcon,
  HandIcon,
  ListBulletsIcon,
  SlidersHorizontalIcon,
  TargetIcon,
  type Icon,
} from '@phosphor-icons/react';
import type { ExecutorConfig, ModelInfo } from 'shared/types';
import { BaseCodingAgent, ExecutionMode, PermissionPolicy } from 'shared/types';
import { toPrettyCase } from '@/shared/lib/string';
import {
  getModelKey,
  getRecentModelEntries,
  touchRecentModel,
  updateRecentModelEntries,
  setRecentReasoning,
} from '@/shared/lib/recentModels';
import {
  getReasoningLabel,
  getSelectedModel,
  escapeAttributeValue,
  parseModelId,
  appendPresetModel,
  resolveDefaultModelId,
  isModelAvailable,
  buildModelSelectionOverride,
  findModelForSelection,
  resolveReasoningOverrideState,
} from '@/shared/lib/modelSelector';
import { profilesApi } from '@/shared/lib/api';
import { useUserSystem } from '@/shared/hooks/useUserSystem';
import { getResolvedTheme, useTheme } from '@/shared/hooks/useTheme';
import { useModelSelectorConfig } from '@/shared/hooks/useExecutorDiscovery';
import { ModelSelectorPopover } from '@vibe/ui/components/ModelSelectorPopover';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTriggerButton,
} from '@vibe/ui/components/Dropdown';

interface ModelSelectorContainerProps {
  agent: BaseCodingAgent | null;
  workspaceId: string | undefined;
  sessionId?: string;
  onAdvancedSettings: () => void;
  presets: string[];
  selectedPreset: string | null;
  onPresetSelect: (presetId: string | null) => void;
  onOverrideChange: (partial: Partial<ExecutorConfig>) => void;
  executorConfig: ExecutorConfig | null;
  presetOptions: ExecutorConfig | null | undefined;
}

const EMPTY_REASONING_OPTIONS: ModelInfo['reasoning_options'] = [];

type PendingReasoningSelection = {
  reasoningId: string | null;
};

export function ModelSelectorContainer({
  agent,
  workspaceId,
  sessionId,
  onAdvancedSettings,
  presets,
  selectedPreset,
  onPresetSelect,
  onOverrideChange,
  executorConfig,
  presetOptions,
}: ModelSelectorContainerProps) {
  const { t } = useTranslation('common');
  const { theme } = useTheme();
  const resolvedTheme = getResolvedTheme(theme);
  const [isOpen, setIsOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [expandedProviderId, setExpandedProviderId] = useState('');
  const { profiles, setProfiles, reloadSystem } = useUserSystem();
  const defaultLabel = t('modelSelector.default');
  const loadingLabel = t('states.loading');

  const permissionMetaByPolicy: Record<
    PermissionPolicy,
    { label: string; icon: Icon }
  > = {
    [PermissionPolicy.AUTO]: {
      label: t('modelSelector.permissionAuto'),
      icon: FastForwardIcon,
    },
    [PermissionPolicy.SUPERVISED]: {
      label: t('modelSelector.permissionAsk'),
      icon: HandIcon,
    },
    [PermissionPolicy.PLAN]: {
      label: t('modelSelector.permissionPlan'),
      icon: ListBulletsIcon,
    },
  };

  const resolvedPreset =
    selectedPreset ??
    (presets.includes('DEFAULT') ? 'DEFAULT' : (presets[0] ?? null));

  const {
    config: streamConfig,
    loadingModels,
    error: streamError,
  } = useModelSelectorConfig(agent, {
    workspaceId: sessionId ? workspaceId : undefined,
    sessionId,
  });

  useEffect(() => {
    if (streamError) {
      console.error('Failed to fetch model config', streamError);
    }
  }, [streamError]);

  const baseConfig = streamConfig;
  const config = appendPresetModel(baseConfig, presetOptions?.model_id);

  const availableProviderIds = useMemo(
    () => config?.providers.map((item) => item.id) ?? [],
    [config?.providers]
  );
  const hasProviders = availableProviderIds.length > 0;
  const providerIdMap = useMemo(
    () => new Map(availableProviderIds.map((id) => [id.toLowerCase(), id])),
    [availableProviderIds]
  );
  const resolveProviderId = (value?: string | null) =>
    value ? (providerIdMap.get(value.toLowerCase()) ?? null) : null;

  const { providerId: configProviderId, modelId: configModelId } = useMemo(
    () => parseModelId(executorConfig?.model_id, hasProviders),
    [executorConfig?.model_id, hasProviders]
  );

  const fallbackProviderId = availableProviderIds[0] ?? null;
  const resolvedConfigProviderId = resolveProviderId(configProviderId);

  const { providerId: presetProviderId } = useMemo(
    () => parseModelId(presetOptions?.model_id, hasProviders),
    [presetOptions?.model_id, hasProviders]
  );
  const resolvedPresetProviderId = resolveProviderId(presetProviderId);

  const hasDefaultModel = Boolean(config?.default_model);
  const selectedProviderId =
    resolvedConfigProviderId ??
    resolvedPresetProviderId ??
    (hasDefaultModel ? fallbackProviderId : null);

  const defaultModelId = config
    ? resolveDefaultModelId(
        config.models,
        selectedProviderId,
        config.default_model,
        hasProviders
      )
    : null;

  const { modelId: presetModelId } = useMemo(
    () => parseModelId(presetOptions?.model_id, hasProviders),
    [presetOptions?.model_id, hasProviders]
  );

  const presetModelMatchesProvider =
    !selectedProviderId ||
    !resolvedPresetProviderId ||
    resolvedPresetProviderId === selectedProviderId;
  const resolvedPresetModelId = presetModelMatchesProvider
    ? presetModelId
    : null;

  const selectedModelId = (() => {
    const candidate = configModelId ?? resolvedPresetModelId ?? defaultModelId;
    if (!candidate || !config || !selectedProviderId) return candidate;
    const hasMatch = isModelAvailable(config, selectedProviderId, candidate);
    return hasMatch
      ? candidate
      : resolveDefaultModelId(
          config.models,
          selectedProviderId,
          config.default_model,
          hasProviders
        );
  })();

  const selectedModel = config
    ? getSelectedModel(config.models, selectedProviderId, selectedModelId)
    : null;

  const selectedReasoningOptions =
    selectedModel?.reasoning_options ?? EMPTY_REASONING_OPTIONS;
  const hasConfiguredReasoning = Boolean(
    executorConfig &&
      Object.prototype.hasOwnProperty.call(executorConfig, 'reasoning_id')
  );
  const configuredReasoningId = executorConfig?.reasoning_id;
  const reasoningOverrideState = useMemo(
    () =>
      resolveReasoningOverrideState(
        selectedReasoningOptions,
        configuredReasoningId,
        hasConfiguredReasoning
      ),
    [configuredReasoningId, hasConfiguredReasoning, selectedReasoningOptions]
  );
  const selectedReasoningId = reasoningOverrideState.selectedReasoningId;

  useEffect(() => {
    if (loadingModels) return;

    const repair = reasoningOverrideState.repair;
    if (repair) {
      onOverrideChange(repair);
    }
  }, [loadingModels, onOverrideChange, reasoningOverrideState]);

  const defaultAgentId =
    config?.agents.find((entry) => entry.is_default)?.id ?? null;

  const selectedAgentId =
    executorConfig?.agent_id !== undefined
      ? executorConfig.agent_id
      : (presetOptions?.agent_id ?? defaultAgentId);

  const supportsPermissions = (config?.permissions.length ?? 0) > 0;

  const basePermissionPolicy = supportsPermissions
    ? (presetOptions?.permission_policy ?? config?.permissions[0] ?? null)
    : null;
  const permissionPolicy = supportsPermissions
    ? (executorConfig?.permission_policy ?? basePermissionPolicy)
    : null;

  // LRU persistence (on popover close)

  const recentModelEntries = getRecentModelEntries(profiles, agent);
  const pendingModelRef = useRef<ModelInfo | null>(null);
  const pendingReasoningRef = useRef<PendingReasoningSelection | null>(null);

  const persistPendingSelections = useCallback(() => {
    if (!profiles || !agent) return;
    if (!pendingModelRef.current && pendingReasoningRef.current === null) {
      return;
    }

    let nextProfiles = profiles;

    const model = pendingModelRef.current;
    if (model) {
      pendingModelRef.current = null;
      const current = getRecentModelEntries(nextProfiles, agent);
      const nextEntries = touchRecentModel(current, model);
      nextProfiles = updateRecentModelEntries(nextProfiles, agent, nextEntries);
    }

    const reasoningModel =
      model ??
      (selectedModelId && config
        ? getSelectedModel(config.models, selectedProviderId, selectedModelId)
        : null);
    const pendingReasoning = pendingReasoningRef.current;
    if (pendingReasoning && reasoningModel) {
      nextProfiles = setRecentReasoning(
        nextProfiles,
        agent,
        reasoningModel,
        pendingReasoning.reasoningId
      );
      pendingReasoningRef.current = null;
    }

    if (nextProfiles !== profiles) {
      setProfiles(nextProfiles);
      void profilesApi
        .save(JSON.stringify({ executors: nextProfiles }, null, 2))
        .catch((error) => {
          console.error('Failed to save recent models', error);
          void reloadSystem();
        });
    }
  }, [
    agent,
    config,
    profiles,
    reloadSystem,
    selectedModelId,
    selectedProviderId,
    setProfiles,
  ]);

  const handleModelSelect = (modelId: string | null, providerId?: string) => {
    const modelOverride = config
      ? buildModelSelectionOverride(config.models, modelId, providerId)
      : {
          model_id: modelId
            ? providerId
              ? `${providerId}/${modelId}`
              : modelId
            : null,
        };
    onOverrideChange(modelOverride);

    pendingModelRef.current =
      modelId && config
        ? findModelForSelection(config.models, modelId, providerId)
        : null;
    pendingReasoningRef.current = null;
  };

  const handleReasoningSelect = (reasoningId: string | null) => {
    onOverrideChange({ reasoning_id: reasoningId });
    pendingReasoningRef.current = { reasoningId };
  };

  const handleAgentSelect = (id: string | null) => {
    onOverrideChange({ agent_id: id });
  };

  const handlePermissionPolicyChange = (policy: PermissionPolicy) => {
    if (!supportsPermissions) return;
    onOverrideChange({ permission_policy: policy });
  };

  const scrollRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setSearchQuery('');
  }, [selectedProviderId]);

  useEffect(() => {
    if (!isOpen) {
      setSearchQuery('');
      return;
    }
    requestAnimationFrame(() => {
      const node = scrollRef.current;
      if (!node) return;
      if (selectedModelId && config) {
        const selected = getSelectedModel(
          config.models,
          selectedProviderId,
          selectedModelId
        );
        if (selected) {
          const key = getModelKey(selected);
          const selector = `[data-model-key="${escapeAttributeValue(key)}"]`;
          const target = node.querySelector(selector);
          if (target instanceof HTMLElement) {
            target.scrollIntoView({ block: 'nearest' });
            return;
          }
        }
      }
      if (!selectedModelId) {
        node.scrollTop = node.scrollHeight;
      }
    });
  }, [config, isOpen, selectedModelId, selectedProviderId]);

  const handleOpenChange = (open: boolean) => {
    setIsOpen(open);
    if (open) {
      const selected =
        selectedModelId && config
          ? getSelectedModel(config.models, selectedProviderId, selectedModelId)
          : null;
      setExpandedProviderId(selected?.provider_id ?? selectedProviderId ?? '');
    } else {
      persistPendingSelections();
    }
  };

  useEffect(() => {
    if (isOpen) return;
    persistPendingSelections();
  }, [isOpen, persistPendingSelections]);

  const presetLabel = resolvedPreset
    ? toPrettyCase(resolvedPreset)
    : defaultLabel;

  if (!config) {
    return (
      <>
        <DropdownMenu>
          <DropdownMenuTriggerButton size="sm" label={loadingLabel} disabled />
        </DropdownMenu>
      </>
    );
  }

  const showModelSelector = loadingModels || config.models.length > 0;
  const showDefaultOption = !config.default_model && config.models.length > 0;
  const displaySelectedModel = showModelSelector
    ? getSelectedModel(config.models, selectedProviderId, selectedModelId)
    : null;
  const reasoningLabel = displaySelectedModel
    ? getReasoningLabel(
        displaySelectedModel.reasoning_options,
        selectedReasoningId
      )
    : null;
  const modelLabelBase = loadingModels
    ? loadingLabel
    : (displaySelectedModel?.name ?? selectedModelId ?? defaultLabel);
  const modelLabel = reasoningLabel
    ? `${modelLabelBase} · ${reasoningLabel}`
    : modelLabelBase;

  const agentLabel = selectedAgentId
    ? (config.agents.find((entry) => entry.id === selectedAgentId)?.label ??
      toPrettyCase(selectedAgentId))
    : defaultLabel;

  const permissionMeta = permissionPolicy
    ? (permissionMetaByPolicy[permissionPolicy] ?? null)
    : null;
  const permissionIcon = permissionMeta?.icon ?? HandIcon;
  const isCodex = agent === BaseCodingAgent.CODEX;
  const executionMode =
    executorConfig?.execution_mode ??
    presetOptions?.execution_mode ??
    (permissionPolicy === PermissionPolicy.PLAN
      ? ExecutionMode.plan
      : ExecutionMode.code);
  const isGoalMode =
    executionMode === ExecutionMode.goal ||
    executionMode === ExecutionMode.plan_with_goal;
  const parallelAgentLimit = executorConfig?.goal_max_concurrent_agents ?? 0;
  const executionModeOptions = [
    { value: ExecutionMode.code, label: 'Code' },
    { value: ExecutionMode.plan, label: 'Plan' },
    { value: ExecutionMode.goal, label: 'Goal' },
    { value: ExecutionMode.plan_with_goal, label: 'Plan with Goal' },
  ];
  const handleExecutionModeChange = (nextMode: ExecutionMode) => {
    const usesGoalOptions =
      nextMode === ExecutionMode.goal ||
      nextMode === ExecutionMode.plan_with_goal;
    onOverrideChange({
      execution_mode: nextMode,
      ...(permissionPolicy === PermissionPolicy.PLAN
        ? { permission_policy: PermissionPolicy.SUPERVISED }
        : {}),
      ...(!usesGoalOptions
        ? {
            goal_token_budget: null,
            goal_max_concurrent_agents: null,
          }
        : {}),
    });
  };
  const parsePositiveInteger = (value: string): number | null => {
    if (!value.trim()) return null;
    const parsed = Number(value);
    return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
  };

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTriggerButton
          size="sm"
          icon={SlidersHorizontalIcon}
          label={
            resolvedPreset?.toLowerCase() !== 'default'
              ? presetLabel
              : undefined
          }
          showCaret={false}
        />
        <DropdownMenuContent align="start">
          <DropdownMenuLabel>{t('modelSelector.preset')}</DropdownMenuLabel>
          {presets.length > 0 ? (
            presets.map((preset) => (
              <DropdownMenuItem
                key={preset}
                icon={preset === resolvedPreset ? CheckIcon : undefined}
                onClick={() => onPresetSelect?.(preset)}
              >
                {toPrettyCase(preset)}
              </DropdownMenuItem>
            ))
          ) : (
            <DropdownMenuItem disabled>{presetLabel}</DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem icon={GearIcon} onClick={onAdvancedSettings}>
            {t('modelSelector.custom')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      {showModelSelector && (
        <ModelSelectorPopover
          isOpen={isOpen}
          onOpenChange={handleOpenChange}
          trigger={
            <DropdownMenuTriggerButton
              size="sm"
              label={modelLabel}
              disabled={loadingModels}
            />
          }
          config={config}
          error={streamError}
          selectedProviderId={selectedProviderId}
          selectedModelId={selectedModelId}
          selectedReasoningId={selectedReasoningId}
          searchQuery={searchQuery}
          onSearchChange={setSearchQuery}
          onModelSelect={handleModelSelect}
          onReasoningSelect={handleReasoningSelect}
          recentModelEntries={recentModelEntries}
          showDefaultOption={showDefaultOption}
          onSelectDefault={() => handleModelSelect(null)}
          scrollRef={scrollRef}
          expandedProviderId={expandedProviderId}
          onExpandedProviderIdChange={setExpandedProviderId}
          resolvedTheme={resolvedTheme}
        />
      )}

      {isCodex && (
        <DropdownMenu>
          <DropdownMenuTriggerButton
            size="sm"
            icon={
              isGoalMode
                ? TargetIcon
                : executionMode === ExecutionMode.plan
                  ? ListBulletsIcon
                  : FastForwardIcon
            }
            label={
              executionModeOptions.find(({ value }) => value === executionMode)
                ?.label ?? 'Code'
            }
          />
          <DropdownMenuContent align="start">
            <DropdownMenuLabel>Execution mode</DropdownMenuLabel>
            {executionModeOptions.map(({ value, label }) => (
              <DropdownMenuItem
                key={value}
                icon={executionMode === value ? CheckIcon : undefined}
                onClick={() => handleExecutionModeChange(value)}
              >
                {label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      )}

      {isCodex && isGoalMode && (
        <DropdownMenu>
          <DropdownMenuTriggerButton
            size="sm"
            icon={TargetIcon}
            label="Goal settings"
          />
          <DropdownMenuContent align="start" className="w-72">
            <DropdownMenuLabel>Goal settings</DropdownMenuLabel>
            <div className="space-y-base px-base py-base">
              <label className="block space-y-half text-sm text-normal">
                <span>Token budget</span>
                <input
                  type="number"
                  min={1}
                  step={1}
                  value={executorConfig?.goal_token_budget ?? ''}
                  placeholder="Auto"
                  onChange={(event) =>
                    onOverrideChange({
                      goal_token_budget: parsePositiveInteger(
                        event.currentTarget.value
                      ),
                    })
                  }
                  className="border-border bg-primary text-normal w-full rounded-sm border px-base py-half"
                />
              </label>
              <label className="block space-y-half text-sm text-normal">
                <span>Parallel agents</span>
                <div className="flex gap-half">
                  <button
                    type="button"
                    className="border-border hover:bg-secondary rounded-sm border px-base py-half"
                    onClick={() =>
                      onOverrideChange({ goal_max_concurrent_agents: null })
                    }
                  >
                    Auto
                  </button>
                  <button
                    type="button"
                    className="border-border hover:bg-secondary rounded-sm border px-base py-half"
                    onClick={() =>
                      onOverrideChange({ goal_max_concurrent_agents: 0 })
                    }
                  >
                    Off
                  </button>
                  <input
                    type="number"
                    min={1}
                    step={1}
                    value={parallelAgentLimit > 0 ? parallelAgentLimit : ''}
                    placeholder="Max"
                    aria-label="Parallel agent limit"
                    onChange={(event) =>
                      onOverrideChange({
                        goal_max_concurrent_agents: parsePositiveInteger(
                          event.currentTarget.value
                        ),
                      })
                    }
                    className="border-border bg-primary text-normal min-w-0 flex-1 rounded-sm border px-base py-half"
                  />
                </div>
              </label>
              <p className="text-xs text-low">
                Parallel agents is a maximum for subagents, excluding the main
                agent.
              </p>
            </div>
          </DropdownMenuContent>
        </DropdownMenu>
      )}

      {permissionPolicy && config.permissions.length > 0 && (
        <DropdownMenu>
          <DropdownMenuTriggerButton
            size="sm"
            icon={permissionIcon}
            showCaret={false}
          />
          <DropdownMenuContent align="start">
            <DropdownMenuLabel>
              {t('modelSelector.permissions')}
            </DropdownMenuLabel>
            {config.permissions
              .filter((policy) => !isCodex || policy !== PermissionPolicy.PLAN)
              .map((policy) => {
                const meta = permissionMetaByPolicy[policy];
                return (
                  <DropdownMenuItem
                    key={policy}
                    icon={meta?.icon ?? HandIcon}
                    onClick={() => handlePermissionPolicyChange(policy)}
                  >
                    {meta?.label ?? toPrettyCase(policy)}
                  </DropdownMenuItem>
                );
              })}
          </DropdownMenuContent>
        </DropdownMenu>
      )}

      {config.agents.length > 0 && (
        <DropdownMenu>
          <DropdownMenuTriggerButton size="sm" label={agentLabel} />
          <DropdownMenuContent align="start">
            <DropdownMenuLabel>{t('modelSelector.agent')}</DropdownMenuLabel>
            <DropdownMenuItem
              icon={selectedAgentId === null ? CheckIcon : undefined}
              onClick={() => handleAgentSelect(null)}
            >
              {t('modelSelector.default')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            {config.agents.map((agentOption) => (
              <DropdownMenuItem
                key={agentOption.id}
                icon={
                  agentOption.id === selectedAgentId ? CheckIcon : undefined
                }
                onClick={() => handleAgentSelect(agentOption.id)}
              >
                {agentOption.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </>
  );
}
