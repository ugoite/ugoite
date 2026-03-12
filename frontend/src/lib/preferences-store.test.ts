// REQ-FE-003: Portable selected space preferences with local fallback
// REQ-FE-044: Portable locale preferences with local fallback
// REQ-FE-059: Portable theme preferences with local fallback
import { beforeEach, describe, expect, it } from "vitest";
import { getPreferencePatches, resetMockData, seedPreferences } from "~/test/mocks/handlers";

const resetUiState = async () => {
	const { setLocale } = await import("./i18n");
	const { setColorMode, setPrimaryColor, setUiTheme } = await import("./ui-theme");
	const { resetPortablePreferencesState } = await import("./preferences-store");
	setLocale("en");
	setUiTheme("materialize");
	setColorMode("light");
	setPrimaryColor("violet");
	localStorage.clear();
	resetPortablePreferencesState();
	document.documentElement.lang = "";
	document.documentElement.dataset.locale = "";
	delete document.documentElement.dataset.uiTheme;
	delete document.documentElement.dataset.colorMode;
	delete document.documentElement.dataset.primaryColor;
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
		expect(getPreferencePatches()).toContainEqual(expectedPatch);
	});

	it("REQ-FE-044: initializes locale from portable preferences and refreshes local fallback", async () => {
		localStorage.setItem("ugoite-locale", "en");
		seedPreferences({ locale: "ja" });

		const { initializePortablePreferences } = await import("./preferences-store");
		const { locale, t } = await import("./i18n");
		await initializePortablePreferences();

		expect(locale()).toBe("ja");
		expect(t("themeMenu.language")).toBe("言語");
		expect(document.documentElement.lang).toBe("ja");
		expect(localStorage.getItem("ugoite-locale")).toBe("ja");
	});

	it("REQ-FE-059: initializes theme preferences from portable storage and updates local fallback", async () => {
		localStorage.setItem("ugoite-ui-theme", "pop");
		localStorage.setItem("ugoite-color-mode", "light");
		localStorage.setItem("ugoite-primary-color", "amber");
		seedPreferences({
			ui_theme: "classic",
			color_mode: "dark",
			primary_color: "blue",
		});

		const { initializePortablePreferences } = await import("./preferences-store");
		const { colorMode, primaryColor, uiTheme } = await import("./ui-theme");
		await initializePortablePreferences();

		expect(uiTheme()).toBe("classic");
		expect(colorMode()).toBe("dark");
		expect(primaryColor()).toBe("blue");
		expect(document.documentElement.dataset.uiTheme).toBe("classic");
		expect(document.documentElement.dataset.colorMode).toBe("dark");
		expect(document.documentElement.dataset.primaryColor).toBe("blue");
		expect(localStorage.getItem("ugoite-ui-theme")).toBe("classic");
		expect(localStorage.getItem("ugoite-color-mode")).toBe("dark");
		expect(localStorage.getItem("ugoite-primary-color")).toBe("blue");
	});

	it("REQ-FE-059: migrates missing primary color from local fallback", async () => {
		localStorage.setItem("ugoite-ui-theme", "classic");
		localStorage.setItem("ugoite-color-mode", "dark");
		localStorage.setItem("ugoite-primary-color", "amber");
		seedPreferences({
			ui_theme: "classic",
			color_mode: "dark",
			primary_color: null,
		});

		const { initializePortablePreferences } = await import("./preferences-store");
		await initializePortablePreferences();

		const expectedPatch = {} as import("./types").UserPreferencesPatchPayload;
		expectedPatch.primary_color = "amber";
		expect(getPreferencePatches()).toContainEqual(expectedPatch);
	});

	it("REQ-FE-059: migrates missing locale and theme fields from local fallback", async () => {
		localStorage.setItem("ugoite-locale", "ja");
		localStorage.setItem("ugoite-ui-theme", "classic");
		localStorage.setItem("ugoite-color-mode", "dark");
		seedPreferences({
			locale: null,
			ui_theme: null,
			color_mode: null,
		});

		const { initializePortablePreferences } = await import("./preferences-store");
		await initializePortablePreferences();

		const expectedPatch = {} as import("./types").UserPreferencesPatchPayload;
		expectedPatch.locale = "ja";
		expectedPatch.ui_theme = "classic";
		expectedPatch.color_mode = "dark";
		expect(getPreferencePatches()).toContainEqual(expectedPatch);
	});

	it("REQ-FE-044: public routes apply local locale without remote preference fetch", async () => {
		let requestCount = 0;
		const { server } = await import("~/test/mocks/server");
		const { http, HttpResponse } = await import("msw");
		server.use(
			http.get("http://localhost:3000/api/preferences/me", () => {
				requestCount += 1;
				return HttpResponse.json({ locale: "ja" });
			}),
		);
		localStorage.setItem("ugoite-locale", "ja");

		const { initializePortablePreferencesForPath } = await import("./preferences-store");
		const { locale, t } = await import("./i18n");
		await initializePortablePreferencesForPath("/");

		expect(requestCount).toBe(0);
		expect(locale()).toBe("ja");
		expect(t("themeMenu.language")).toBe("言語");
		expect(document.documentElement.lang).toBe("ja");
	});

	it("REQ-FE-059: public routes apply local theme preferences without remote fetch", async () => {
		let requestCount = 0;
		const { server } = await import("~/test/mocks/server");
		const { http, HttpResponse } = await import("msw");
		server.use(
			http.get("http://localhost:3000/api/preferences/me", () => {
				requestCount += 1;
				return HttpResponse.json({
					ui_theme: "classic",
					color_mode: "dark",
					primary_color: "blue",
				});
			}),
		);
		localStorage.setItem("ugoite-ui-theme", "pop");
		localStorage.setItem("ugoite-color-mode", "dark");
		localStorage.setItem("ugoite-primary-color", "amber");

		const { initializePortablePreferencesForPath } = await import("./preferences-store");
		const { colorMode, primaryColor, uiTheme } = await import("./ui-theme");
		await initializePortablePreferencesForPath("/about/");

		expect(requestCount).toBe(0);
		expect(uiTheme()).toBe("pop");
		expect(colorMode()).toBe("dark");
		expect(primaryColor()).toBe("amber");
		expect(document.documentElement.dataset.uiTheme).toBe("pop");
		expect(document.documentElement.dataset.colorMode).toBe("dark");
		expect(document.documentElement.dataset.primaryColor).toBe("amber");
	});

	it("REQ-FE-003: authenticated routes hydrate portable preferences through route-aware initialization", async () => {
		seedPreferences({ selected_space_id: "space-remote" });

		const { initializePortablePreferencesForPath, portablePreferences } = await import(
			"./preferences-store"
		);
		await initializePortablePreferencesForPath("/spaces");

		expect(portablePreferences().selected_space_id).toBe("space-remote");
	});

	it("REQ-FE-003: reuses an in-flight portable preferences initialization", async () => {
		let requestCount = 0;
		const { server } = await import("~/test/mocks/server");
		const { http, HttpResponse, delay } = await import("msw");
		server.use(
			http.get("http://localhost:3000/api/preferences/me", async () => {
				requestCount += 1;
				await delay(50);
				return HttpResponse.json({ selected_space_id: "space-remote" });
			}),
		);

		const { initializePortablePreferences, portablePreferences } = await import(
			"./preferences-store"
		);
		await Promise.all([initializePortablePreferences(), initializePortablePreferences()]);

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
		const { initializePortablePreferences, patchPortablePreferences, portablePreferences } =
			await import("./preferences-store");
		await initializePortablePreferences();

		const preferences = await patchPortablePreferences({});

		expect(preferences).toEqual(portablePreferences());
		expect(getPreferencePatches()).toEqual([]);
	});
});
