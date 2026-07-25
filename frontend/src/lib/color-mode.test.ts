import "@testing-library/jest-dom/vitest";
import { afterEach, describe, expect, it, vi } from "vitest";
import { initializeColorMode } from "./color-mode";

describe("system color mode", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    delete document.documentElement.dataset.colorMode;
  });

  it("updates the document when the operating system preference changes", () => {
    const listeners = new Set<(event: MediaQueryListEvent) => void>();
    const mediaQuery = {
      matches: false,
      addEventListener: (
        type: string,
        listener: (event: MediaQueryListEvent) => void,
      ) => {
        if (type === "change") listeners.add(listener);
      },
    } as unknown as MediaQueryList;
    vi.stubGlobal("matchMedia", vi.fn(() => mediaQuery));

    initializeColorMode();
    expect(document.documentElement.dataset.colorMode).toBe("light");

    mediaQuery.matches = true;
    for (const listener of listeners) {
      listener({ matches: true } as MediaQueryListEvent);
    }

    expect(document.documentElement.dataset.colorMode).toBe("dark");
  });
});
