import { describe, expect, it } from 'vitest';
import { preserveVisibleTailStart } from './conversation-row-model';

describe('tail virtualization while reading history', () => {
  it('defers moving a completed long turn into estimated rows until returning to bottom', () => {
    const streamingStart = 10;
    const completedStart = 180;
    const browsingStart = preserveVisibleTailStart(
      streamingStart,
      completedStart,
      false
    );
    expect(browsingStart).toBe(streamingStart);
    expect(preserveVisibleTailStart(browsingStart, completedStart, true)).toBe(
      completedStart
    );
  });

  it('allows a shorter/reset conversation and a fresh scope', () => {
    expect(preserveVisibleTailStart(180, 3, false)).toBe(3);
    expect(preserveVisibleTailStart(null, 20, false)).toBe(20);
  });
});
