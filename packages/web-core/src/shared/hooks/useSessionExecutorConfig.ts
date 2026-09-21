import { useQuery } from '@tanstack/react-query';
import { sessionsApi } from '@/shared/lib/api';
import { useHostId } from '@/shared/providers/HostIdProvider';

/** Immutable launch settings of this session, never legacy script processes. */
export function useSessionExecutorConfig(sessionId: string | undefined) {
  const hostId = useHostId();
  return useQuery({
    queryKey: ['session-executor-config', hostId, sessionId],
    queryFn: () => sessionsApi.getExecutorConfig(sessionId!, hostId),
    enabled: !!sessionId,
    staleTime: 0,
  });
}
