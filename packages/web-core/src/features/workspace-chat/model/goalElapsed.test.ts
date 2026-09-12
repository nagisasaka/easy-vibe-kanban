import { afterEach, describe, expect, it, vi } from 'vitest';
import { watchGoalElapsed } from './goalElapsed';

afterEach(() => vi.useRealTimers());

describe('Goal elapsed display', () => {
  it('never advances a saved active goal when its run has ended', () => {
    vi.useFakeTimers();
    const update = vi.fn();
    watchGoalElapsed(0, true, false, update);
    vi.advanceTimersByTime(60_000);
    expect(update.mock.calls).toEqual([[0]]);
    expect(vi.getTimerCount()).toBe(0);
  });
  it('stops interpolation on termination and restores reported time', () => {
    vi.useFakeTimers();
    const update = vi.fn();
    const stop = watchGoalElapsed(10, true, true, update);
    vi.advanceTimersByTime(3000);
    expect(update).toHaveBeenLastCalledWith(13);
    stop?.();
    watchGoalElapsed(12, true, false, update);
    vi.advanceTimersByTime(10_000);
    expect(update).toHaveBeenLastCalledWith(12);
    expect(vi.getTimerCount()).toBe(0);
  });
  it('does not advance paused or completed goals even with an active run', () => {
    vi.useFakeTimers();
    const update = vi.fn();
    watchGoalElapsed(20, false, true, update);
    vi.advanceTimersByTime(5000);
    expect(update.mock.calls).toEqual([[20]]);
  });
});
