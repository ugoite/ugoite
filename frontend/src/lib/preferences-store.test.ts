// REQ-FE-003: Portable selected space preferences with local fallback
// REQ-FE-044: Portable locale preferences with local fallback
import { beforeEach, describe, expect, it } from "vitest";
import {
  getPreferencePatches,
  resetMockData,
  seedPreferences,
} from "~/test/mocks/handlers";
import { testApiUrl } from "~/test/http-origin";

const resetUiState = async () => {
  const { setLocale } = await import("./i18n");
  const { resetPortablePreferencesState } = await import("./preferences-store");
  setLocale("en");
  localStorage.clear();
  resetPortablePreferencesState();
  document.documentElement.lang = "";
  document.documentElement.dataset.locale = "";
};

describe("preferencesStore", () => {
  beforeEach(async () => {
    resetMockData();
    await resetUiState();
  });

  it("REQ-FE-003: initializes selected space from local fallback and migrates portable storage", async () => {
    localStorage.setItem("ugoite-selected-space", "space-local");

    const { initializePortablePreferences, portablePreferences } = await import(
      "./preferences-store"
    );
    await initializePortablePreferences();

    expect(portablePreferences().selected_space_id).toBe("space-local");
    expect(localStorage.getItem("ugoite-selected-space")).toBe("space-local");
    const expectedPatch = {} as import("./types").UserPreferencesPatchPayload;
    expectedPatch.selected_space_id = "space-local";
    expect(getPreferencePatches()).toContainEqual(
      expect.objectContaining(expectedPatch),
    );
  });

  it("REQ-FE-044: initializes locale from portable preferences and refreshes local fallback", async () => {
    localStorage.setItem("ugoite-locale", "en");
    seedPreferences({ locale: "ja" });

    const { initializePortablePreferences } = await import(
      "./preferences-store"
    );
    const { locale, t } = await import("./i18n");
    await initializePortablePreferences();

    expect(locale()).toBe("ja");
    expect(t("themeMenu.language")).toBe("言語");
    expect(document.documentElement.lang).toBe("ja");
    expect(localStorage.getItem("ugoite-locale")).toBe("ja");
  });

  it("REQ-FE-044: migrates a missing locale from local fallback", async () => {
    localStorage.setItem("ugoite-locale", "ja");
    seedPreferences({
      locale: null,
    });

    const { initializePortablePreferences } = await import(
      "./preferences-store"
    );
    await initializePortablePreferences();

    const expectedPatch = {} as import("./types").UserPreferencesPatchPayload;
    expectedPatch.locale = "ja";
    expect(getPreferencePatches()).toContainEqual(
      expect.objectContaining(expectedPatch),
    );
  });

  it("REQ-FE-044: public routes apply local locale without remote preference fetch", async () => {
    let requestCount = 0;
    const { server } = await import("~/test/mocks/server");
    const { http, HttpResponse } = await import("msw");
    server.use(
      http.get(testApiUrl("/preferences/me"), () => {
        requestCount += 1;
        return HttpResponse.json({ locale: "ja" });
      }),
    );
    localStorage.setItem("ugoite-locale", "ja");

    const { initializePortablePreferencesForPath } = await import(
      "./preferences-store"
    );
    const { locale, t } = await import("./i18n");
    await initializePortablePreferencesForPath("/");

    expect(requestCount).toBe(0);
    expect(locale()).toBe("ja");
    expect(t("themeMenu.language")).toBe("言語");
    expect(document.documentElement.lang).toBe("ja");
  });

  it("REQ-FE-003: authenticated routes hydrate portable preferences through route-aware initialization", async () => {
    seedPreferences({ selected_space_id: "space-remote" });

    const { initializePortablePreferencesForPath, portablePreferences } =
      await import(
        "./preferences-store"
      );
    await initializePortablePreferencesForPath("/spaces");

    expect(portablePreferences().selected_space_id).toBe("space-remote");
  });

  it("REQ-FE-003: public routes keep initialized remote portable preferences", async () => {
    localStorage.setItem("ugoite-selected-space", "space-local");
    seedPreferences({ selected_space_id: "space-remote" });

    const { initializePortablePreferencesForPath, portablePreferences } =
      await import(
        "./preferences-store"
      );
    await initializePortablePreferencesForPath("/spaces");
    localStorage.removeItem("ugoite-selected-space");

    await initializePortablePreferencesForPath("/");

    expect(portablePreferences().selected_space_id).toBe("space-remote");
  });

  it("REQ-FE-003: reuses an in-flight portable preferences initialization", async () => {
    let requestCount = 0;
    const { server } = await import("~/test/mocks/server");
    const { http, HttpResponse, delay } = await import("msw");
    server.use(
      http.get(testApiUrl("/preferences/me"), async () => {
        requestCount += 1;
        await delay(50);
        return HttpResponse.json({ selected_space_id: "space-remote" });
      }),
    );

    const { initializePortablePreferences, portablePreferences } = await import(
      "./preferences-store"
    );
    await Promise.all([
      initializePortablePreferences(),
      initializePortablePreferences(),
    ]);

    expect(requestCount).toBe(1);
    expect(portablePreferences().selected_space_id).toBe("space-remote");
  });

  it("REQ-FE-003: returns cached portable preferences after initialization", async () => {
    seedPreferences({ selected_space_id: "space-remote" });

    const { initializePortablePreferences, portablePreferences } = await import(
      "./preferences-store"
    );
    await initializePortablePreferences();
    const preferences = await initializePortablePreferences();

    expect(preferences).toEqual(portablePreferences());
    expect(preferences.selected_space_id).toBe("space-remote");
  });

  it("REQ-FE-003: skips empty portable preference patches", async () => {
    const {
      initializePortablePreferences,
      patchPortablePreferences,
      portablePreferences,
    } = await import("./preferences-store");
    await initializePortablePreferences();

    const preferences = await patchPortablePreferences({});

    expect(preferences).toEqual(portablePreferences());
    expect(getPreferencePatches()).toEqual([]);
  });
});
