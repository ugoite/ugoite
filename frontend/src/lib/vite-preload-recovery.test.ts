import { describe, expect, it, vi } from "vitest";
import {
  recoverFromVitePreloadError,
  type VitePreloadErrorEvent,
} from "./vite-preload-recovery";

const createEvent = (
  message = "Failed to fetch a route chunk",
): VitePreloadErrorEvent => {
  const event = new Event("vite:preloadError", { cancelable: true });
  Object.assign(event, { payload: new Error(message) });
  return event as VitePreloadErrorEvent;
};

const createSession = () => {
  const values = new Map<string, string>();
  return {
    getItem: vi.fn((key: string) => values.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => values.set(key, value)),
  };
};

describe("Vite dynamic import recovery", () => {
  it("reloads the current page once after a failed dynamic import", () => {
    const event = createEvent();
    const session = createSession();
    const reload = vi.fn();

    expect(
      recoverFromVitePreloadError(
        event,
        session,
        reload,
        "/spaces/one/forms",
        1_000,
      ),
    ).toBe(true);
    expect(event.defaultPrevented).toBe(true);
    expect(session.setItem).toHaveBeenCalledWith(
      expect.stringMatching(/^ugoite:vite-preload-recovery-at:/),
      "1000",
    );
    expect(reload).toHaveBeenCalledOnce();
  });

  it("lets a repeated failure reach the app boundary instead of reloading again", () => {
    const session = createSession();
    const firstReload = vi.fn();
    const repeatedReload = vi.fn();
    recoverFromVitePreloadError(
      createEvent(),
      session,
      firstReload,
      "/spaces/one/forms",
      1_000,
    );
    const repeatedEvent = createEvent();

    expect(
      recoverFromVitePreloadError(
        repeatedEvent,
        session,
        repeatedReload,
        "/spaces/one/forms",
        1_001,
      ),
    ).toBe(false);
    expect(repeatedEvent.defaultPrevented).toBe(false);
    expect(firstReload).toHaveBeenCalledOnce();
    expect(repeatedReload).not.toHaveBeenCalled();
  });

  it("recovers a different failed chunk independently", () => {
    const session = createSession();
    const reload = vi.fn();
    recoverFromVitePreloadError(
      createEvent("Failed to fetch route chunk A"),
      session,
      reload,
      "/spaces/one/forms",
      1_000,
    );
    const nextEvent = createEvent("Failed to fetch route chunk B");

    expect(
      recoverFromVitePreloadError(
        nextEvent,
        session,
        reload,
        "/spaces/one/forms",
        1_001,
      ),
    ).toBe(true);
    expect(nextEvent.defaultPrevented).toBe(true);
    expect(reload).toHaveBeenCalledTimes(2);
  });

  it("recovers separate routes when browsers use the same generic message", () => {
    const session = createSession();
    const reload = vi.fn();
    const genericMessage = "Importing a module script failed.";
    recoverFromVitePreloadError(
      createEvent(genericMessage),
      session,
      reload,
      "/spaces/one/forms",
      1_000,
    );
    const otherRouteEvent = createEvent(genericMessage);

    expect(
      recoverFromVitePreloadError(
        otherRouteEvent,
        session,
        reload,
        "/spaces/one/entries",
        1_001,
      ),
    ).toBe(true);
    expect(otherRouteEvent.defaultPrevented).toBe(true);
    expect(reload).toHaveBeenCalledTimes(2);
  });

  it("allows a later failure to recover after the loop guard expires", () => {
    const session = createSession();
    const reload = vi.fn();
    recoverFromVitePreloadError(
      createEvent(),
      session,
      reload,
      "/spaces/one/forms",
      1_000,
    );
    const nextEvent = createEvent();

    expect(
      recoverFromVitePreloadError(
        nextEvent,
        session,
        reload,
        "/spaces/one/forms",
        31_000,
      ),
    ).toBe(true);
    expect(nextEvent.defaultPrevented).toBe(true);
    expect(reload).toHaveBeenCalledTimes(2);
  });

  it("keeps Vite's normal error when session storage cannot be read", () => {
    const event = createEvent();
    const reload = vi.fn();
    const session = {
      getItem: () => {
        throw new Error("Storage is disabled");
      },
      setItem: () => undefined,
    };

    expect(
      recoverFromVitePreloadError(
        event,
        session,
        reload,
        "/spaces/one/forms",
        1_000,
      ),
    ).toBe(false);
    expect(event.defaultPrevented).toBe(false);
    expect(reload).not.toHaveBeenCalled();
  });
});
