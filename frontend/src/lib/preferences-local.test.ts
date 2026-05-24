import { afterEach, describe, expect, it, vi } from "vitest";

describe("preferences-local", () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.resetModules();
	});

	it("returns empty preferences when browser storage is unavailable", async () => {
		vi.stubGlobal("window", undefined);
		vi.resetModules();

		const { emptyUserPreferences, readLocalPreferences, writeLocalPreferences } = await import(
			"./preferences-local"
		);

		expect(readLocalPreferences()).toEqual(emptyUserPreferences());
		expect(() =>
			writeLocalPreferences({
				selected_space_id: "space-a",
				locale: "ja",
				ui_theme: "classic",
				color_mode: "dark",
				primary_color: "blue",
			}),
		).not.toThrow();
	});

	it("returns empty preferences when localStorage is malformed", async () => {
		const invalidStorage = {};
		vi.stubGlobal("window", { localStorage: invalidStorage });
		vi.stubGlobal("localStorage", invalidStorage);
		vi.resetModules();

		const { emptyUserPreferences, readLocalPreferences } = await import("./preferences-local");

		expect(readLocalPreferences()).toEqual(emptyUserPreferences());
	});
});
