import type { SessionSelection } from './useWorkspaceSessions';

/** Refetching a run's sessions must not move a reader away from a chosen log
 * or discard an ordinary new-session draft. New explicit links take priority;
 * a URL used to enter the view must not override subsequent manual selection. */
export function resolveSessionSelection(
  sessions: { id: string }[],
  previous: SessionSelection | undefined,
  requestedId: string | null,
  scopeChanged: boolean
): SessionSelection | undefined {
  if (
    !scopeChanged &&
    previous &&
    (previous.mode === 'new' ||
      sessions.some((session) => session.id === previous.sessionId))
  )
    return previous;
  if (requestedId && sessions.some((session) => session.id === requestedId)) {
    return { mode: 'existing', sessionId: requestedId };
  }
  return sessions[0]
    ? { mode: 'existing', sessionId: sessions[0].id }
    : undefined;
}
