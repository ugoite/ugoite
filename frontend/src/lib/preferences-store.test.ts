// REQ-FE-003: Portable selected space preferences with local fallback
// REQ-FE-044: Portable locale preferences with local fallback
// REQ-FE-059: Portable color mode preferences with local fallback
import { beforeEach, describe, expect, it } from "vitest";
import {
  getPreferencePatches,
  resetMockData,
  seedPreferences,
} from "~/test/mocks/handlers";
import { testApiUrl } from "~/test/http-origin";

const resetUiState = async () => {
  const { setLocale } = await import("./i18n");
  const { setColorMode } = await import("./color-mode");
  const { resetPortablePreferencesState } = await import("./preferences-store");
  setLocale("en");
  setColorMode("light");
  localStorage.clear();
  resetPortablePreferencesState();
  document.documentElement.lang = "";
  document.documentElement.dataset.locale = "";
  delete document.documentElement.dataset.colorMode;
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

  it("REQ-FE-059: initializes color mode from portable storage and updates local fallback", async () => {
    localStorage.setItem("ugoite-color-mode", "light");
    seedPreferences({
      color_mode: "dark",
    });

    const { initializePortablePreferences } = await import(
      "./preferences-store"
    );
    const { colorMode } = await import("./color-mode");
    await initializePortablePreferences();

    expect(colorMode()).toBe("dark");
    expect(document.documentElement.dataset.colorMode).toBe("dark");
    expect(localStorage.getItem("ugoite-color-mode")).toBe("dark");
  });

  it("REQ-FE-059: migrates missing color mode from local fallback", async () => {
    localStorage.setItem("ugoite-color-mode", "dark");
    seedPreferences({
      color_mode: null,
    });

    const { initializePortablePreferences } = await import(
      "./preferences-store"
    );
    await initializePortablePreferences();

    const expectedPatch = {} as import("./types").UserPreferencesPatchPayload;
    expectedPatch.color_mode = "dark";
    expect(getPreferencePatches()).toContainEqual(
      expect.objectContaining(expectedPatch),
    );
  });

  it("persists the desktop content width locally and remotely", async () => {
    const { setContentWidthPreference, portablePreferences } = await import(
      "./preferences-store"
    );

    await setContentWidthPreference("wide");

    expect(portablePreferences().content_width).toBe("wide");
    expect(localStorage.getItem("ugoite-content-width")).toBe("wide");
    expect(getPreferencePatches()).toContainEqual({ content_width: "wide" });
  });

  it("REQ-FE-059: migrates missing locale and color mode from local fallback", async () => {
    localStorage.setItem("ugoite-locale", "ja");
    localStorage.setItem("ugoite-color-mode", "dark");
    seedPreferences({
      locale: null,
      color_mode: null,
    });

    const { initializePortablePreferences } = await import(
      "./preferences-store"
    );
    await initializePortablePreferences();

    const expectedPatch = {} as import("./types").UserPreferencesPatchPayload;
    expectedPatch.locale = "ja";
    expectedPatch.color_mode = "dark";
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

  it("REQ-FE-059: public routes apply local color mode without remote fetch", async () => {
    let requestCount = 0;
    const { server } = await import("~/test/mocks/server");
    const { http, HttpResponse } = await import("msw");
    server.use(
      http.get(testApiUrl("/preferences/me"), () => {
        requestCount += 1;
        return HttpResponse.json({
          color_mode: "dark",
        });
      }),
    );
    localStorage.setItem("ugoite-color-mode", "dark");

    const { initializePortablePreferencesForPath } = await import(
      "./preferences-store"
    );
    const { colorMode } = await import("./color-mode");
    for (const pathname of ["/about/", "/does-not-exist"]) {
      await initializePortablePreferencesForPath(pathname);
    }

    expect(requestCount).toBe(0);
    expect(colorMode()).toBe("dark");
    expect(document.documentElement.dataset.colorMode).toBe("dark");
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
