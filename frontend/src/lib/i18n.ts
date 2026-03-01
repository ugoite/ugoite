import uiDictionary from "../../../shared/i18n/ui.json";
import { createRoot, createSignal } from "solid-js";
import { isServer } from "solid-js/web";

type Dictionary = typeof uiDictionary;

export type Locale = keyof Dictionary;
export type TranslationKey = keyof Dictionary["en"];

const LOCALE_STORAGE_KEY = "ugoite-locale";

const safeStorage = () => {
	/* v8 ignore start */
	if (isServer || typeof window === "undefined") return null;
	/* v8 ignore stop */
	return window.localStorage;
};

const availableLocales = new Set<Locale>(Object.keys(uiDictionary) as Locale[]);

const readStoredLocale = (): Locale | null => {
	const storage = safeStorage();
	/* v8 ignore start */
	if (!storage) return null;
	/* v8 ignore stop */
	const value = storage.getItem(LOCALE_STORAGE_KEY);
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
		const storage = safeStorage();
		storage?.setItem(LOCALE_STORAGE_KEY, nextLocale);
		applyLocaleAttributes(nextLocale);
	};

	return {
		locale,
		setLocale,
	};
});

export const locale = localeStore.locale;
export const setLocale = localeStore.setLocale;

export const t = (key: TranslationKey): string => {
	const currentLocale = locale();
	/* v8 ignore start */
	const currentDict = uiDictionary[currentLocale] ?? uiDictionary.en;
	return currentDict[key] ?? uiDictionary.en[key] ?? key;
	/* v8 ignore stop */
};

export const initializeLocale = () => {
	applyLocaleAttributes(locale());
};
