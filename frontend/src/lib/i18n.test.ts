// REQ-FE-044: Frontend multilingual dictionary and locale switching
import { describe, it, expect, beforeEach, vi } from "vitest";
import { locale, setLocale, t, initializeLocale } from "./i18n";

describe("i18n", () => {
	beforeEach(() => {
		localStorage.clear();
		setLocale("en");
	});

	it("switches locale and translates labels", () => {
		expect(t("themeMenu.language")).toBe("Language");

		setLocale("ja");

		expect(locale()).toBe("ja");
		expect(t("themeMenu.language")).toBe("言語");
		expect(localStorage.getItem("ugoite-locale")).toBe("ja");
	});

	it("applies locale attributes on initialize", () => {
		setLocale("ja");
		initializeLocale();

		expect(document.documentElement.lang).toBe("ja");
		expect(document.documentElement.dataset.locale).toBe("ja");
	});

	it("falls back to key text when translation key is unknown", () => {
		setLocale("ja");
		expect(t("missing.translation.key" as never)).toBe("missing.translation.key");
	});

	it("restores locale from localStorage on module initialization", async () => {
		localStorage.setItem("ugoite-locale", "ja");
		vi.resetModules();

		const i18n = await import("./i18n");
		expect(i18n.locale()).toBe("ja");
		expect(i18n.t("themeMenu.language")).toBe("言語");
	});
});
