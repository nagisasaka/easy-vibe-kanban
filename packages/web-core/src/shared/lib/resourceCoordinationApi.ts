import type {
  RegisterResource,
  ResourceEvent,
  ResourceMediation,
  ResourceOperation,
  ResourceRecovery,
  ResourceSnapshot,
  SharedResource,
} from 'shared/types';
import { handleApiResponse } from './api';

export function createResourceCoordinationClient(
  send: (path: string, init?: RequestInit) => Promise<Response>
) {
  async function request<T>(path: string, body?: unknown): Promise<T> {
    return handleApiResponse<T>(
      await send(
        `/api/resource-coordination${path}`,
        body === undefined
          ? { cache: 'no-store' }
          : {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify(body),
            }
      )
    );
  }
  return {
    snapshot: () => request<ResourceSnapshot>('/snapshot'),
    operations: () => request<ResourceOperation[]>('/operations'),
    events: (after = 0) => request<ResourceEvent[]>(`/events?after=${after}`),
    mediations: () => request<ResourceMediation[]>('/mediations'),
    stopMediation: (id: string) =>
      request<void>(`/mediations/${id}/cancel`, {}),
    register: (body: RegisterResource) =>
      request<SharedResource>('/resources', body),
    cancel: (id: string) =>
      request<ResourceOperation>(`/operations/${id}/cancel`, {}),
    mediate: (id: string) =>
      request<ResourceMediation | null>(`/operations/${id}/mediate`, {}),
    recover: (id: string, body: ResourceRecovery) =>
      request<ResourceOperation>(`/operations/${id}/recover`, body),
  };
}

export type ResourceCoordinationClient = ReturnType<
  typeof createResourceCoordinationClient
>;

export function resourceOperationCanCancel(status: string) {
  return [
    'queued',
    'blocked',
    'launching',
    'running',
    'recovery_required',
  ].includes(status);
}
