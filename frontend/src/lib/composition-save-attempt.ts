/**
 * Browser-memory state for retrying an uncertain Composition save.
 * These attempts survive route unmounts in the current tab, but are not
 * persisted to browser storage or shared across server requests.
 */
export interface PendingCompositionSaveAttempt {
  spaceId: string;
  /** Authenticated owner of this in-memory attempt; never an auth credential. */
  principalId: string;
  /** Saved-SQL route identity. EntryQuery saves identify by routePath only. */
  sqlId?: string;
  routePath: string;
  saveTarget: CompositionSaveTarget;
  name: string;
  yaml: string;
  idempotencyKey: string;
}

export type CompositionSaveTarget =
  | { kind: "create" }
  | { kind: "update"; compositionId: string; baseRevisionId: string };

export interface StoredCompositionSaveAttempt {
  attempt: PendingCompositionSaveAttempt;
  state: "in_flight" | "uncertain";
}

export type CompositionSaveRouteIdentity = Pick<
  PendingCompositionSaveAttempt,
  "spaceId" | "sqlId" | "routePath" | "saveTarget"
>;
export type CompositionSaveAttemptIdentity =
  & CompositionSaveRouteIdentity
  & Pick<PendingCompositionSaveAttempt, "principalId">;
type CompositionSaveAttemptEvent =
  | { type: "pending"; stored: StoredCompositionSaveAttempt }
  | {
    type: "cleared";
    attemptName: string;
    idempotencyKey: string;
    outcome: "completed" | "rejected";
  };
type Listener = (event: CompositionSaveAttemptEvent) => void;

const pendingAttempts = typeof window === "undefined"
  ? undefined
  : new Map<string, StoredCompositionSaveAttempt>();
const listeners = typeof window === "undefined"
  ? undefined
  : new Map<string, Set<Listener>>();
const identityKey = ({
  spaceId,
  principalId,
  sqlId,
  routePath,
  saveTarget,
}: CompositionSaveAttemptIdentity) =>
  JSON.stringify([spaceId, principalId, sqlId, routePath, saveTarget]);

export function sameCompositionSaveIdentity(
  left: CompositionSaveAttemptIdentity,
  right: CompositionSaveAttemptIdentity,
): boolean {
  return identityKey(left) === identityKey(right);
}

export function getPendingCompositionSaveAttempt(
  identity: CompositionSaveAttemptIdentity,
): StoredCompositionSaveAttempt | undefined {
  return pendingAttempts?.get(identityKey(identity));
}

export function stagePendingCompositionSaveAttempt(
  attempt: PendingCompositionSaveAttempt,
): void {
  const stored = {
    attempt,
    state: "in_flight",
  } as const;
  const key = identityKey(attempt);
  pendingAttempts?.set(key, stored);
  for (const listener of listeners?.get(key) ?? []) {
    listener({ type: "pending", stored });
  }
}

export function markPendingCompositionSaveAttemptUncertain(
  attempt: PendingCompositionSaveAttempt,
): void {
  const key = identityKey(attempt);
  const existing = pendingAttempts?.get(key);
  if (
    !existing ||
    existing.attempt.idempotencyKey !== attempt.idempotencyKey
  ) return;
  const stored = { attempt, state: "uncertain" as const };
  pendingAttempts?.set(key, stored);
  for (const listener of listeners?.get(key) ?? []) {
    listener({ type: "pending", stored });
  }
}

export function clearPendingCompositionSaveAttempt(
  identity: CompositionSaveAttemptIdentity & { idempotencyKey?: string },
  outcome?: "completed" | "rejected",
): void {
  const key = identityKey(identity);
  const existing = pendingAttempts?.get(key);
  if (
    identity.idempotencyKey &&
    existing?.attempt.idempotencyKey !== identity.idempotencyKey
  ) return;
  pendingAttempts?.delete(key);
  if (!existing || !outcome) return;
  const event: CompositionSaveAttemptEvent = {
    type: "cleared",
    attemptName: existing.attempt.name,
    idempotencyKey: existing.attempt.idempotencyKey,
    outcome,
  };
  for (const listener of listeners?.get(key) ?? []) listener(event);
}

export function subscribeToPendingCompositionSaveAttempt(
  identity: CompositionSaveAttemptIdentity,
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
