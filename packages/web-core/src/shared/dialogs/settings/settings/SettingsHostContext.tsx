import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { listRelayHosts } from '@/shared/lib/remoteApi';
import { useAppRuntime } from '@/shared/hooks/useAppRuntime';
import {
  initialSettingsHost,
  canEditSettingsHost,
} from '@/shared/lib/settingsHostPolicy';
import { useAuth } from '@/shared/hooks/auth/useAuth';
import { useHostId } from '@/shared/providers/HostIdProvider';
import {
  createMachineClient,
  type MachineClient,
  type MachineTarget,
} from '@/shared/lib/machineClient';
import {
  useRemoteCloudHostsState,
  type RemoteCloudHost,
} from '@/shared/hooks/useRemoteCloudHosts';
import { listPairedRelayHosts } from '@/shared/lib/relayPairingStorage';

export type SettingsHostTargetId = 'local' | string;

export type SettingsHostTarget = MachineTarget & {
  description?: string;
  status?: 'online' | 'offline';
};

interface SettingsHostContextValue {
  availableHosts: SettingsHostTarget[];
  hostsResolved: boolean;
  selectedHostId: SettingsHostTargetId | null;
  selectedHost: SettingsHostTarget | null;
  canEdit: boolean;
  discoveryFailed: boolean;
  setSelectedHostId: (hostId: SettingsHostTargetId) => void;
}

const SettingsHostContext = createContext<SettingsHostContextValue | null>(
  null
);

function toLocalRuntimeTargets(
  remoteHosts: RemoteCloudHost[],
  getLabel: (key: string, defaultValue: string) => string
): SettingsHostTarget[] {
  return [
    {
      id: 'local',
      apiHostId: null,
      label: getLabel('settings.hostPicker.thisMachine', 'This machine'),
      description: getLabel('settings.hostPicker.localHost', 'Local host'),
      kind: 'local',
    },
    ...remoteHosts.map((host) => ({
      id: host.id,
      apiHostId: host.id,
      label: host.name,
      description: getLabel('settings.hostPicker.remoteHost', 'Remote host'),
      status:
        host.status === 'online' ? ('online' as const) : ('offline' as const),
      kind: 'remote' as const,
    })),
  ];
}

export function SettingsHostProvider({
  initialHostId,
  children,
}: {
  initialHostId?: SettingsHostTargetId;
  children: ReactNode;
}) {
  const { t } = useTranslation('settings');
  const runtime = useAppRuntime();
  const routeHostId = useHostId();
  const { isSignedIn } = useAuth();
  const { data: localRemoteHosts, isError: localRemoteHostsError } =
    useRemoteCloudHostsState();
  const {
    data: relayHosts = [],
    isLoading: relayHostsLoading,
    isError: relayHostsError,
  } = useQuery({
    queryKey: ['settings-dialog', 'relay-hosts'],
    queryFn: listRelayHosts,
    enabled: runtime === 'remote' && isSignedIn,
    staleTime: 30_000,
  });
  const {
    data: pairedRelayHosts = [],
    isLoading: pairedRelayHostsLoading,
    isError: pairedRelayHostsError,
  } = useQuery({
    queryKey: ['settings-dialog', 'paired-relay-hosts'],
    queryFn: listPairedRelayHosts,
    enabled: runtime === 'remote' && isSignedIn,
    staleTime: 5_000,
  });
  const hostsResolved = useMemo(() => {
    if (runtime === 'local') {
      return true;
    }

    if (!isSignedIn) {
      return true;
    }

    return !relayHostsLoading && !pairedRelayHostsLoading;
  }, [isSignedIn, pairedRelayHostsLoading, relayHostsLoading, runtime]);

  const availableHosts = useMemo<SettingsHostTarget[]>(() => {
    if (runtime === 'local') {
      return toLocalRuntimeTargets(localRemoteHosts?.hosts ?? [], t);
    }

    const pairedHostIds = new Set(pairedRelayHosts.map((host) => host.host_id));
    return relayHosts
      .filter((host) => pairedHostIds.has(host.id))
      .map((host) => ({
        id: host.id,
        apiHostId: host.id,
        label: host.name,
        description: t('settings.hostPicker.remoteHost', 'Remote host'),
        status:
          host.status === 'online' ? ('online' as const) : ('offline' as const),
        kind: 'remote',
      }));
  }, [localRemoteHosts?.hosts, pairedRelayHosts, relayHosts, runtime, t]);

  const [selectedHostId, setSelectedHostId] =
    useState<SettingsHostTargetId | null>(() =>
      initialSettingsHost([], runtime, routeHostId, initialHostId)
    );

  useEffect(() => {
    const nextHostId = initialSettingsHost(
      availableHosts,
      runtime,
      routeHostId,
      initialHostId
    );

    setSelectedHostId((current) => current ?? nextHostId);
  }, [availableHosts, initialHostId, routeHostId, runtime]);

  const selectedHost = useMemo(
    () => availableHosts.find((host) => host.id === selectedHostId) ?? null,
    [availableHosts, selectedHostId]
  );
  const discoveryFailed =
    runtime === 'local'
      ? localRemoteHostsError || !!localRemoteHosts?.discoveryFailed
      : relayHostsError || pairedRelayHostsError || !isSignedIn;
  const canEdit = canEditSettingsHost(
    selectedHost,
    hostsResolved,
    discoveryFailed
  );

  const value = useMemo<SettingsHostContextValue>(
    () => ({
      availableHosts,
      hostsResolved,
      selectedHostId,
      selectedHost,
      canEdit,
      discoveryFailed,
      setSelectedHostId,
    }),
    [
      availableHosts,
      hostsResolved,
      selectedHost,
      selectedHostId,
      canEdit,
      discoveryFailed,
    ]
  );

  return (
    <SettingsHostContext.Provider value={value}>
      {children}
    </SettingsHostContext.Provider>
  );
}

export function useSettingsHost() {
  const context = useContext(SettingsHostContext);
  if (!context) {
    throw new Error(
      'useSettingsHost must be used within a SettingsHostProvider'
    );
  }
  return context;
}

export function useSettingsMachineClient(): MachineClient | null {
  const runtime = useAppRuntime();
  const { selectedHost, canEdit } = useSettingsHost();
  const current = useRef({ selectedHost, canEdit });
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  current.current = { selectedHost, canEdit };
  const targetId = selectedHost?.id;

  return useMemo(() => {
    const host = current.current.selectedHost;
    if (!host) {
      return null;
    }

    return createMachineClient(
      runtime,
      host,
      () =>
        mounted.current &&
        current.current.canEdit &&
        current.current.selectedHost?.id === targetId
    );
    // Machine identity is stable across status refreshes. The live guard above
    // still rejects writes from stale callbacks after a switch or disconnect.
  }, [runtime, targetId]);
}
