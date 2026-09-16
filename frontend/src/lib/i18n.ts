import uiDictionary from "../../../shared/i18n/ui.json";
import { createRoot, createSignal } from "solid-js";
import {
  readLocalPreferences,
  writeLocalPreferences,
} from "./preferences-local";

type Dictionary = typeof uiDictionary;

export type Locale = Extract<keyof Dictionary, string>;
export type TranslationKey = Extract<keyof Dictionary["en"], string>;
export type TranslationParam = string | number | boolean;
export type TranslationParams = Record<string, TranslationParam>;

const availableLocales = new Set<Locale>(Object.keys(uiDictionary) as Locale[]);

const readStoredLocale = (): Locale | null => {
  const value = readLocalPreferences().locale;
  /* v8 ignore start */
  if (value && availableLocales.has(value as Locale)) {
    return value as Locale;
  }
  /* v8 ignore stop */
  return null;
};

const applyLocaleAttributes = (nextLocale: Locale) => {
  /* v8 ignore start */
  if (typeof document === "undefined") return;
  /* v8 ignore stop */
  document.documentElement.lang = nextLocale;
  document.documentElement.dataset.locale = nextLocale;
};

const localeStore = createRoot(() => {
  const [locale, setLocaleInternal] = createSignal<Locale>(
    readStoredLocale() ?? "en",
    {
      equals: false,
    },
  );

  const setLocale = (nextLocale: Locale) => {
    if (!availableLocales.has(nextLocale)) {
      return;
    }
    setLocaleInternal(nextLocale);
    writeLocalPreferences({ locale: nextLocale as "en" | "ja" });
    applyLocaleAttributes(nextLocale);
  };

  return {
    locale,
    setLocale,
  };
});

export const locale = localeStore.locale;
export const setLocale = localeStore.setLocale;

export const intlLocale = (value: Locale = locale()): string =>
  value === "ja" ? "ja-JP" : "en-US";

const formatTranslationParam = (value: unknown): string => {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  // Never coerce objects/arrays/functions/symbols/bigints to
  // "[object Object]"; unknown shapes render as empty instead.
  return "";
};

export const t = (key: TranslationKey, params?: TranslationParams): string => {
  const currentLocale = locale();
  /* v8 ignore start */
  const currentDict = uiDictionary[currentLocale] ?? uiDictionary.en;
  const template = currentDict[key] ?? uiDictionary.en[key] ?? key;
  if (!params) return template;
  return Object.entries(params).reduce(
    (message, [name, value]) =>
      message.replaceAll(`{${name}}`, formatTranslationParam(value)),
    template,
  );
  /* v8 ignore stop */
};

export const initializeLocale = () => {
  const nextLocale = readStoredLocale() ?? locale();
  if (nextLocale !== locale()) {
    setLocale(nextLocale);
    return;
  }
  applyLocaleAttributes(nextLocale);
};
