import { describe, expect, it } from 'vitest';
import { enqueueScratchMutation } from './scratchMutationQueue';

describe('scratch mutation ordering', () => {
  it('orders delayed save, acknowledgement, and a newer draft without crossing scopes', async () => {
    let release!: () => void;
    const blocked = new Promise<void>((resolve) => {
      release = resolve;
    });
    const operations: string[] = [];
    const first = enqueueScratchMutation('host-a/session', async () => {
      await blocked;
      operations.push('save old');
    });
    const clear = enqueueScratchMutation('host-a/session', async () => {
      operations.push('ack old');
    });
    const next = enqueueScratchMutation('host-a/session', async () => {
      operations.push('save new');
    });
    await enqueueScratchMutation('host-b/session', async () => {
      operations.push('other host');
    });
    expect(operations).toEqual(['other host']);
    release();
    await Promise.all([first, clear, next]);
    expect(operations).toEqual([
      'other host',
      'save old',
      'ack old',
      'save new',
    ]);
  });
  it('does not poison later writes after a storage error', async () => {
    await expect(
      enqueueScratchMutation('failed', async () => {
        throw new Error('storage unavailable');
      })
    ).rejects.toThrow('storage unavailable');
    await expect(
      enqueueScratchMutation('failed', async () => 'saved')
    ).resolves.toBe('saved');
  });
});
