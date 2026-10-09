const recoveryKeyPrefix = "ugoite:vite-preload-recovery-at";
const recoveryWindowMs = 30_000;
const dynamicImportFailurePatterns = [
  /failed to fetch dynamically imported module/i,
  /error loading dynamically imported module/i,
  /importing a module script failed/i,
  /failed to load module script/i,
];

export type VitePreloadErrorEvent = Event & { payload?: Error };

type SessionStore = Pick<
  Storage,
  "getItem" | "setItem" | "removeItem" | "key" | "length"
>;

const recoveryKeyFor = (
  message: string,
  routeIdentity: string,
): string => {
  const identity = `${routeIdentity}\0${message}`;
  let hash = 2_166_136_261;
  for (let index = 0; index < identity.length; index += 1) {
    hash = Math.imul(hash ^ identity.charCodeAt(index), 16_777_619);
  }
  return `${recoveryKeyPrefix}:${(hash >>> 0).toString(36)}`;
};

const errorMessage = (error: unknown): string => {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    return typeof error.message === "string" ? error.message : "";
  }
  return "";
};

const recoverWithMessage = (
  message: string,
  session: SessionStore,
  reload: () => void,
  routeIdentity: string,
  now: number,
): boolean => {
  try {
    const key = recoveryKeyFor(message, routeIdentity);
    const previousAttempt = session.getItem(key);
    if (previousAttempt !== null) {
      const previousAt = Number(previousAttempt);
      const elapsed = now - previousAt;
      if (
        Number.isFinite(previousAt) && elapsed >= 0 &&
        elapsed < recoveryWindowMs
      ) {
        return false;
      }
    }
    session.setItem(key, String(now));
  } catch {
    // If session storage is unavailable, preserve the normal error path.
    return false;
  }

  reload();
  return true;
};

/**
 * A route change is a fresh navigation attempt. Drop markers left by earlier
 * transient chunk failures so identical browser error messages on a later
 * route visit do not inherit the previous retry budget. Same-path reloads do
 * not call this, so a persistently missing chunk still reaches the boundary
 * instead of creating a reload loop.
 */
export function clearVitePreloadRecoveryAttempts(session: SessionStore): void {
  for (let index = session.length - 1; index >= 0; index -= 1) {
    const key = session.key(index);
    if (key?.startsWith(`${recoveryKeyPrefix}:`)) session.removeItem(key);
  }
}

/**
 * Vite cannot retry a failed dynamic import in the current document. Retry it
 * through one same-URL reload for transient network failures, while keeping a
 * short session guard so a missing chunk cannot create a reload loop.
 */
export function recoverFromVitePreloadError(
  event: VitePreloadErrorEvent,
  session: SessionStore,
  reload: () => void,
  routeIdentity: string,
  now = Date.now(),
): boolean {
  const recovered = recoverWithMessage(
    event.payload?.message ?? "",
    session,
    reload,
    routeIdentity,
    now,
  );
  if (recovered) event.preventDefault();
  return recovered;
}

/**
 * Some router dynamic imports surface failed chunk fetches through the
 * application ErrorBoundary instead of Vite's `vite:preloadError` event.
 * Recover those browser-specific module-load errors through the same bounded
 * same-URL reload path.
 */
export function recoverFromRouteChunkFailure(
  error: unknown,
  session: SessionStore,
  reload: () => void,
  routeIdentity: string,
  now = Date.now(),
): boolean {
  const message = errorMessage(error);
  if (!dynamicImportFailurePatterns.some((pattern) => pattern.test(message))) {
    return false;
  }
  return recoverWithMessage(message, session, reload, routeIdentity, now);
}

/** Install before SolidStart begins resolving route modules on the client. */
export function installVitePreloadRecovery(): void {
  if (typeof window === "undefined") return;

  let session: Storage;
  try {
    session = window.sessionStorage;
  } catch {
    return;
  }

  window.addEventListener(
    "vite:preloadError",
    ((event: Event) => {
      recoverFromVitePreloadError(
        event as VitePreloadErrorEvent,
        session,
        () => window.location.reload(),
        window.location.pathname,
      );
    }) as EventListener,
  );
}
