// Serialise mutations of one scratch identity so a delayed save cannot resurrect
// an acknowledged draft. Different hosts/identities never block one another.
const pending = new Map<string, Promise<unknown>>();

export function enqueueScratchMutation<T>(
  key: string,
  operation: () => Promise<T>
): Promise<T> {
  const next = (pending.get(key) ?? Promise.resolve())
    .catch(() => undefined)
    .then(operation);
  pending.set(key, next);
  const release = () => {
    if (pending.get(key) === next) pending.delete(key);
  };
  void next.then(release, release);
  return next;
}
