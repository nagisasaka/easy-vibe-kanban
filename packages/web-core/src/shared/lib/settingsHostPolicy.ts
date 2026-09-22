import type { AppRuntime } from '@/shared/hooks/useAppRuntime';

interface HostIdentity {
  id: string;
  kind: 'local' | 'remote';
  status?: 'online' | 'offline';
}

export function initialSettingsHost(
  hosts: HostIdentity[],
  runtime: AppRuntime,
  routeHostId: string | null,
  initialHostId?: string
): string | null {
  // An explicit but unavailable identity must never become another machine.
  if (initialHostId) return initialHostId;
  if (routeHostId) return routeHostId;
  if (runtime === 'local') return 'local';
  return (
    hosts.find((host) => host.status === 'online')?.id ?? hosts[0]?.id ?? null
  );
}

export function canEditSettingsHost(
  host: HostIdentity | null,
  resolved: boolean,
  discoveryFailed: boolean
): boolean {
  if (!host || !resolved) return false;
  // Optional remote discovery is not a dependency of local settings.
  return (
    host.kind === 'local' || (!discoveryFailed && host.status === 'online')
  );
}
