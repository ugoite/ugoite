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

export interface StoredCompositionSaveAttempt {
  attempt: PendingCompositionSaveAttempt;
  state: "in_flight" | "uncertain";
  routeVisitId: number | undefined;
}

type RouteIdentity = Pick<
  PendingCompositionSaveAttempt,
  "spaceId" | "sqlId" | "routePath"
>;
type CompositionSaveAttemptEvent =
  | { type: "pending"; stored: StoredCompositionSaveAttempt }
  | {
    type: "cleared";
    idempotencyKey: string;
    outcome: "completed" | "rejected";
    routeVisitId: number | undefined;
  };
type Listener = (event: CompositionSaveAttemptEvent) => void;

const pendingAttempts = typeof window === "undefined"
  ? undefined
  : new Map<string, StoredCompositionSaveAttempt>();
const listeners = typeof window === "undefined"
  ? undefined
  : new Map<string, Set<Listener>>();
let nextRouteVisitId = 0;
let activeRouteVisit: { key: string; id: number } | undefined;

const identityKey = ({ spaceId, sqlId, routePath }: RouteIdentity) =>
  JSON.stringify([spaceId, sqlId, routePath]);

export function getPendingCompositionSaveAttempt(
  identity: RouteIdentity,
): StoredCompositionSaveAttempt | undefined {
  return pendingAttempts?.get(identityKey(identity));
}

export function stagePendingCompositionSaveAttempt(
  attempt: PendingCompositionSaveAttempt,
  routeVisitId: number | undefined,
): void {
  pendingAttempts?.set(identityKey(attempt), {
    attempt,
    state: "in_flight",
    routeVisitId,
  });
}

export function markPendingCompositionSaveAttemptUncertain(
  attempt: PendingCompositionSaveAttempt,
  routeVisitId: number | undefined,
): void {
  const key = identityKey(attempt);
  const existing = pendingAttempts?.get(key);
  if (
    !existing ||
    existing.attempt.idempotencyKey !== attempt.idempotencyKey ||
    existing.routeVisitId !== routeVisitId
  ) return;
  const stored = { attempt, state: "uncertain" as const, routeVisitId };
  pendingAttempts?.set(key, stored);
  for (const listener of listeners?.get(key) ?? []) {
    listener({ type: "pending", stored });
  }
}

export function clearPendingCompositionSaveAttempt(
  identity: RouteIdentity & { idempotencyKey?: string },
  outcome?: "completed" | "rejected",
  routeVisitId?: number,
): void {
  const key = identityKey(identity);
  const existing = pendingAttempts?.get(key);
  if (
    identity.idempotencyKey &&
    existing?.attempt.idempotencyKey !== identity.idempotencyKey
  ) return;
  if (
    routeVisitId !== undefined &&
    existing?.routeVisitId !== routeVisitId
  ) return;
  pendingAttempts?.delete(key);
  if (!existing || !outcome) return;
  const event: CompositionSaveAttemptEvent = {
    type: "cleared",
    idempotencyKey: existing.attempt.idempotencyKey,
    outcome,
    routeVisitId: existing.routeVisitId,
  };
  for (const listener of listeners?.get(key) ?? []) listener(event);
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

export function beginCompositionSaveRouteVisit(
  identity: RouteIdentity,
): number | undefined {
  if (!pendingAttempts) return undefined;
  const id = ++nextRouteVisitId;
  activeRouteVisit = { key: identityKey(identity), id };
  return id;
}

export function isCurrentCompositionSaveRouteVisit(
  identity: RouteIdentity,
  visitId: number | undefined,
): boolean {
  if (visitId === undefined || !pendingAttempts) return true;
  return activeRouteVisit?.key === identityKey(identity) &&
    activeRouteVisit.id === visitId;
}
