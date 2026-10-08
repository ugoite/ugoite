const recoveryKeyPrefix = "ugoite:vite-preload-recovery-at";
const recoveryWindowMs = 30_000;

export type VitePreloadErrorEvent = Event & { payload?: Error };

type SessionStore = Pick<Storage, "getItem" | "setItem">;

const recoveryKeyFor = (
  event: VitePreloadErrorEvent,
  routeIdentity: string,
): string => {
  const message = event.payload?.message ?? "";
  const identity = `${routeIdentity}\0${message}`;
  let hash = 2_166_136_261;
  for (let index = 0; index < identity.length; index += 1) {
    hash = Math.imul(hash ^ identity.charCodeAt(index), 16_777_619);
  }
  return `${recoveryKeyPrefix}:${(hash >>> 0).toString(36)}`;
};

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
  try {
    const key = recoveryKeyFor(event, routeIdentity);
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
    // If session storage is unavailable, preserve Vite's normal error path.
    return false;
  }

  event.preventDefault();
  reload();
  return true;
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
