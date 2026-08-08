// REQ-FE-044: Frontend multilingual dictionary and locale switching
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { initializeLocale, locale, setLocale, t } from "./i18n";
import uiDictionary from "../../../shared/i18n/ui.json";

describe("i18n", () => {
  afterEach(() => setLocale("en"));

  beforeEach(() => {
    localStorage.clear();
    window.localStorage.clear();
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
    expect(t("missing.translation.key" as never)).toBe(
      "missing.translation.key",
    );
  });

  it("restores locale from localStorage during initializeLocale", async () => {
    window.localStorage.setItem("ugoite-locale", "ja");
    vi.resetModules();

    const i18n = await import("./i18n");
    i18n.initializeLocale();
    expect(i18n.locale()).toBe("ja");
    expect(document.documentElement.lang).toBe("ja");
    expect(document.documentElement.dataset.locale).toBe("ja");
    expect(i18n.t("themeMenu.language")).toBe("言語");
  });

  it("REQ-FE-044: initializeLocale refreshes locale from stored preferences", async () => {
    vi.resetModules();
    const i18n = await import("./i18n");

    expect(i18n.locale()).toBe("en");

    window.localStorage.setItem("ugoite-locale", "ja");
    i18n.initializeLocale();

    expect(i18n.locale()).toBe("ja");
    expect(document.documentElement.lang).toBe("ja");
    expect(document.documentElement.dataset.locale).toBe("ja");
    expect(i18n.t("themeMenu.language")).toBe("言語");
  });

  it("ignores invalid locale", () => {
    setLocale("en");
    setLocale("invalid" as never);
    expect(locale()).toBe("en");
  });

  it("REQ-FE-044: interpolates localized placeholders", () => {
    expect(t("dashboard.section.createEntry.formsAvailable", { count: 2 }))
      .toBe(
        "2 forms available",
      );

    setLocale("ja");

    expect(t("dashboard.section.createEntry.formsAvailable", { count: 2 }))
      .toBe(
        "利用可能なフォーム 2 件",
      );
  });

  it("REQ-FE-044: keeps the English and Japanese key sets identical", () => {
    expect(Object.keys(uiDictionary.en).sort()).toEqual(
      Object.keys(uiDictionary.ja).sort(),
    );
  });

  it("REQ-FE-044: preserves contextual English navigation copy", () => {
    expect(t("globalShell.settings")).toBe("Settings");
    expect(t("dashboard.formsEntries")).toBe("Forms / Entries");
    expect(t("formsPage.selectForm")).toBe("Select a Form");
    expect(t("sqlPage.createTitle")).toBe("SQL / New");
    expect(t("sqlPage.variablesTitle")).toBe("SQL / Variables");
  });
});
