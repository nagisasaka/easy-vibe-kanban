import { afterEach, describe, expect, it, vi } from 'vitest';
import { setLocalApiTransport } from '@/shared/lib/localApiTransport';
import { executionRequest } from './useWorkspaceOwner';

vi.mock('@/shared/providers/HostIdProvider', () => ({
  useHostId: () => 'new-host',
  getCurrentHostId: () => 'new-host',
}));

afterEach(() => setLocalApiTransport(null));

describe('execution inspection host identity', () => {
  it('pins reads and Stop to the captured host even after navigation', async () => {
    const request = vi.fn(
      async (_path: string) =>
        new Response(JSON.stringify({ success: true, data: {} }))
    );
    setLocalApiTransport({ request, openWebSocket: vi.fn() });
    await executionRequest(null, '/local/usage');
    await executionRequest('old-host', '/old/execution/stop', 'POST');
    expect(request.mock.calls.map(([path]) => path)).toEqual([
      '/api/workspaces/local/usage',
      '/api/host/old-host/workspaces/old/execution/stop',
    ]);
  });
});
