// REQ-FE-044: Frontend multilingual dictionary and locale switching
import { describe, it, expect, beforeEach } from "vitest";
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
});
