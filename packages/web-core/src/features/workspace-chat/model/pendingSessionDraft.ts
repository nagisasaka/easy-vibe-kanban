import type { DraftFollowUpData } from 'shared/types';

const key = (scope: string) => `vibe.pendingSessionDraft.v1.${scope}`;

/** Tab-local write-ahead copy only until the scoped Scratch write is acknowledged. */
export function readPendingSessionDraft(scope: string) {
  try {
    const raw = window.sessionStorage.getItem(key(scope));
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (
      parsed.version !== 1 ||
      typeof parsed.nonce !== 'string' ||
      typeof parsed.data?.message !== 'string' ||
      typeof parsed.data?.executor_config?.executor !== 'string'
    )
      return null;
    return { raw, data: parsed.data as DraftFollowUpData };
  } catch {
    return null;
  }
}

export function writePendingSessionDraft(
  scope: string,
  data: DraftFollowUpData
): string | undefined {
  try {
    const raw = JSON.stringify({
      version: 1,
      nonce: crypto.randomUUID(),
      data,
    });
    window.sessionStorage.setItem(key(scope), raw);
    return raw;
  } catch (error) {
    // Storage can be disabled or full. Keep the editor and server persistence
    // usable; do not clear the in-memory draft as a consequence.
    console.error('Could not retain a pending Session draft for reload', error);
    return undefined;
  }
}

export function acknowledgePendingSessionDraft(
  scope: string,
  expected: string | undefined
) {
  if (!expected) return;
  try {
    if (window.sessionStorage.getItem(key(scope)) === expected)
      window.sessionStorage.removeItem(key(scope));
  } catch {
    // A retained backup is safer than clearing a different edit.
  }
}
