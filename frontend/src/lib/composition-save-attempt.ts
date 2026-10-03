/**
 * Browser-memory Work state for retrying an uncertain Composition save.
 * These attempts survive route unmounts in the current tab, but are not
 * persisted to browser storage or shared across server requests.
 */
export interface PendingCompositionSaveAttempt {
  spaceId: string;
  sqlId: string;
  routePath: string;
  name: string;
  yaml: string;
  idempotencyKey: string;
}

type RouteIdentity = Pick<
  PendingCompositionSaveAttempt,
  "spaceId" | "sqlId" | "routePath"
>;
type Listener = (attempt: PendingCompositionSaveAttempt) => void;

const pendingAttempts = typeof window === "undefined"
  ? undefined
  : new Map<string, PendingCompositionSaveAttempt>();
const listeners = typeof window === "undefined"
  ? undefined
  : new Map<string, Set<Listener>>();

const identityKey = ({ spaceId, sqlId, routePath }: RouteIdentity) =>
  JSON.stringify([spaceId, sqlId, routePath]);

export function getPendingCompositionSaveAttempt(
  identity: RouteIdentity,
): PendingCompositionSaveAttempt | undefined {
  return pendingAttempts?.get(identityKey(identity));
}

export function rememberPendingCompositionSaveAttempt(
  attempt: PendingCompositionSaveAttempt,
): void {
  const key = identityKey(attempt);
  pendingAttempts?.set(key, attempt);
  for (const listener of listeners?.get(key) ?? []) listener(attempt);
}

export function clearPendingCompositionSaveAttempt(
  identity: RouteIdentity,
): void {
  pendingAttempts?.delete(identityKey(identity));
}

export function subscribeToPendingCompositionSaveAttempt(
  identity: RouteIdentity,
  listener: Listener,
): () => void {
  const key = identityKey(identity);
  if (!listeners) return () => {};
  const routeListeners = listeners.get(key) ?? new Set<Listener>();
  routeListeners.add(listener);
  listeners.set(key, routeListeners);
  return () => {
    routeListeners.delete(listener);
    if (routeListeners.size === 0) listeners.delete(key);
  };
}
