// REQ-FE-003: Portable selected space preferences with local fallback
// REQ-FE-044: Portable locale preferences with local fallback
// REQ-FE-059: Portable color mode preferences with local fallback
import { afterEach, describe, expect, it, vi } from "vitest";

describe("preferences-local", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
  });

  it("returns empty preferences when browser storage is unavailable", async () => {
    vi.stubGlobal("window", undefined);
    vi.resetModules();

    const {
      emptyUserPreferences,
      readLocalPreferences,
      writeLocalPreferences,
    } = await import(
      "./preferences-local"
    );

    expect(readLocalPreferences()).toEqual(emptyUserPreferences());
    expect(() =>
      writeLocalPreferences({
        selected_space_id: "space-a",
        locale: "ja",
      })
    ).not.toThrow();
  });

  it("returns empty preferences when localStorage is malformed", async () => {
    const invalidStorage = {};
    vi.stubGlobal("window", { localStorage: invalidStorage });
    vi.stubGlobal("localStorage", invalidStorage);
    vi.resetModules();

    const { emptyUserPreferences, readLocalPreferences } = await import(
      "./preferences-local"
    );

    expect(readLocalPreferences()).toEqual(emptyUserPreferences());
  });
});
