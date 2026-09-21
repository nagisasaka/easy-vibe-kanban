import { describe, expect, it } from 'vitest';
import { resolveSessionSelection as select } from './workspaceSessionSelection';

describe('saved session inspection selection', () => {
  const sessions = [{ id: 'latest' }, { id: 'older' }];
  it('keeps a chosen log and new-session draft across refetches', () => {
    expect(
      select(sessions, { mode: 'existing', sessionId: 'older' }, null, false)
    ).toEqual({ mode: 'existing', sessionId: 'older' });
    expect(select(sessions, { mode: 'new' }, null, false)).toEqual({
      mode: 'new',
    });
    expect(
      select(
        sessions,
        { mode: 'existing', sessionId: 'older' },
        'latest',
        false
      )
    ).toEqual({
      mode: 'existing',
      sessionId: 'older',
    });
  });
  it('uses latest on a new workspace/host and honours explicit session links', () => {
    expect(select(sessions, { mode: 'new' }, null, true)).toEqual({
      mode: 'existing',
      sessionId: 'latest',
    });
    expect(select(sessions, undefined, 'older', true)).toEqual({
      mode: 'existing',
      sessionId: 'older',
    });
    expect(select([], undefined, null, true)).toBeUndefined();
  });
});
