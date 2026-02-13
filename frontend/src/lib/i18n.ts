import uiDictionary from "../../../shared/i18n/ui.json";
import { createRoot, createSignal } from "solid-js";
import { isServer } from "solid-js/web";

type Dictionary = typeof uiDictionary;

export type Locale = keyof Dictionary;
export type TranslationKey = keyof Dictionary["en"];

const LOCALE_STORAGE_KEY = "ugoite-locale";

const safeStorage = () => {
	if (isServer || typeof window === "undefined") return null;
	return window.localStorage;
};

const availableLocales = new Set<Locale>(Object.keys(uiDictionary) as Locale[]);

const readStoredLocale = (): Locale | null => {
	const storage = safeStorage();
	if (!storage) return null;
	const value = storage.getItem(LOCALE_STORAGE_KEY);
	if (value && availableLocales.has(value as Locale)) {
		return value as Locale;
	}
	return null;
};

const applyLocaleAttributes = (nextLocale: Locale) => {
	if (isServer || typeof document === "undefined") return;
	document.documentElement.lang = nextLocale;
	document.documentElement.dataset.locale = nextLocale;
};

const localeStore = createRoot(() => {
	const [locale, setLocaleInternal] = createSignal<Locale>(readStoredLocale() ?? "en");

	const setLocale = (nextLocale: Locale) => {
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
	const currentDict = uiDictionary[currentLocale] ?? uiDictionary.en;
	return currentDict[key] ?? uiDictionary.en[key] ?? key;
};

export const initializeLocale = () => {
	applyLocaleAttributes(locale());
};
