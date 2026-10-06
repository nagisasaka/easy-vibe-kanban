import { describe, expect, it, vi } from 'vitest';
import {
  createResourceCoordinationClient,
  resourceOperationCanCancel,
} from './resourceCoordinationApi';
import type { ResourceRecovery } from 'shared/types';

vi.mock('./api', () => ({
  handleApiResponse: async (response: Response) => {
    if (!response.ok) throw new Error('Host rejected operation');
    return (await response.json()).data;
  },
}));

describe('resource ownership controls', () => {
  it('can request termination during recovery, but never offers cancellation as an undo of a completed operation', () => {
    for (const status of [
      'queued',
      'blocked',
      'launching',
      'running',
      'recovery_required',
    ])
      expect(resourceOperationCanCancel(status)).toBe(true);
    for (const status of ['succeeded', 'recovered', 'cancelled'])
      expect(resourceOperationCanCancel(status)).toBe(false);
  });
  it('sends recovery evidence and observed revisions to the selected host without silently retrying a rejected confirmation', async () => {
    const send = vi.fn().mockResolvedValue(new Response('{}', { status: 409 }));
    const client = createResourceCoordinationClient(send);
    const recovery = {
      evidence: 'Stopped old process and checked mock state',
      claims: [
        { resource_id: 'phone', expected_revision: 7, resulting_state: 'idle' },
      ],
    } as ResourceRecovery;
    await expect(client.recover('old-operation', recovery)).rejects.toThrow(
      'Host rejected'
    );
    expect(send).toHaveBeenCalledTimes(1);
    const [path, init] = send.mock.calls[0];
    expect(path).toBe(
      '/api/resource-coordination/operations/old-operation/recover'
    );
    expect(JSON.parse(init.body)).toEqual(recovery);
    expect(init.method).toBe('POST');
  });
  it('isolates each selected host and keeps observation separate from mutations', async () => {
    const a = vi
      .fn()
      .mockImplementation(async () => new Response('{"data":[]}'));
    const b = vi
      .fn()
      .mockImplementation(async () => new Response('{"data":[]}'));
    const one = createResourceCoordinationClient(a);
    const two = createResourceCoordinationClient(b);
    await one.snapshot();
    await two.cancel('operation-b');
    expect(a).toHaveBeenCalledWith('/api/resource-coordination/snapshot', {
      cache: 'no-store',
    });
    expect(b.mock.calls[0][0]).toBe(
      '/api/resource-coordination/operations/operation-b/cancel'
    );
    expect(a).toHaveBeenCalledTimes(1);
  });
});
