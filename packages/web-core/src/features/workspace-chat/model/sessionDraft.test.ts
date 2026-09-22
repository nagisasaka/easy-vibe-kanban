import { describe, expect, it } from 'vitest';
import { isSessionDraftSubmissionCurrent } from './sessionDraft';

describe('composer submission identity', () => {
  const sent = {
    scope: 'host/workspace/session',
    revision: 1,
    content: '{"text":"old","skills":[]}',
  };
  it('acknowledges only the unchanged submission', () => {
    expect(isSessionDraftSubmissionCurrent({ ...sent }, sent)).toBe(true);
    for (const current of [
      { ...sent, scope: 'other-host/workspace/session' },
      { ...sent, scope: 'host/other-workspace/session' },
      { ...sent, scope: 'host/workspace/other-session' },
      { ...sent, revision: 2 }, // text edited away and back again
      { ...sent, content: '{"text":"new","skills":[]}' },
      { ...sent, content: '{"text":"old","skills":["review"]}' },
      { ...sent, content: '{"text":"old","attachments":["new"]}' },
      { ...sent, content: '{"text":"old","config":{"reasoning_id":"max"}}' },
    ])
      expect(isSessionDraftSubmissionCurrent(current, sent)).toBe(false);
  });
  it('does not consume the newly created Session composer', () => {
    expect(
      isSessionDraftSubmissionCurrent(
        { ...sent, scope: 'created-session' },
        { ...sent, scope: 'new-session' }
      )
    ).toBe(false);
  });
});
