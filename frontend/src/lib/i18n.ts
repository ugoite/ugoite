import uiDictionary from "../../../shared/i18n/ui.json";
import { createRoot, createSignal } from "solid-js";
import { isServer } from "solid-js/web";
import { readLocalPreferences, writeLocalPreferences } from "./preferences-local";

type Dictionary = typeof uiDictionary;

export type Locale = keyof Dictionary;
export type TranslationKey = keyof Dictionary["en"];
export type TranslationParams = Record<string, string | number>;

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
	if (isServer || typeof document === "undefined") return;
	/* v8 ignore stop */
	document.documentElement.lang = nextLocale;
	document.documentElement.dataset.locale = nextLocale;
};

const localeStore = createRoot(() => {
	const [locale, setLocaleInternal] = createSignal<Locale>(readStoredLocale() ?? "en");

	const setLocale = (nextLocale: Locale) => {
		if (!availableLocales.has(nextLocale)) {
			return;
		}
		setLocaleInternal(nextLocale);
		writeLocalPreferences({ locale: nextLocale });
		applyLocaleAttributes(nextLocale);
	};

	return {
		locale,
		setLocale,
	};
});

export const locale = localeStore.locale;
export const setLocale = localeStore.setLocale;

export const t = (key: TranslationKey, params?: TranslationParams): string => {
	const currentLocale = locale();
	/* v8 ignore start */
	const currentDict = uiDictionary[currentLocale] ?? uiDictionary.en;
	const template = currentDict[key] ?? uiDictionary.en[key] ?? key;
	if (!params) return template;
	return Object.entries(params).reduce(
		(message, [name, value]) => message.replaceAll(`{${name}}`, String(value)),
		template,
	);
	/* v8 ignore stop */
};

export const initializeLocale = () => {
	applyLocaleAttributes(locale());
};
