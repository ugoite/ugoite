import { isServer } from "solid-js/web";
import type { UserPreferences, UserPreferencesPatchPayload } from "./types";

const SELECTED_SPACE_STORAGE_KEY = "ugoite-selected-space";
const LOCALE_STORAGE_KEY = "ugoite-locale";
const THEME_STORAGE_KEY = "ugoite-ui-theme";
const MODE_STORAGE_KEY = "ugoite-color-mode";
const PRIMARY_COLOR_STORAGE_KEY = "ugoite-primary-color";

export const LOCAL_PREFERENCE_KEYS = {
	selectedSpaceId: SELECTED_SPACE_STORAGE_KEY,
	locale: LOCALE_STORAGE_KEY,
	uiTheme: THEME_STORAGE_KEY,
	colorMode: MODE_STORAGE_KEY,
	primaryColor: PRIMARY_COLOR_STORAGE_KEY,
} as const;

const safeStorage = () => {
	/* v8 ignore start */
	if (isServer || typeof window === "undefined") return null;
	/* v8 ignore stop */
	return window.localStorage;
};

const readAllowedValue = <T extends string>(key: string, allowed: readonly T[]): T | null => {
	const storage = safeStorage();
	/* v8 ignore start */
	if (!storage) return null;
	/* v8 ignore stop */
	const value = storage.getItem(key);
	return value && allowed.includes(value as T) ? (value as T) : null;
};

const readStringValue = (key: string): string | null => {
	const storage = safeStorage();
	/* v8 ignore start */
	if (!storage) return null;
	/* v8 ignore stop */
	return storage.getItem(key);
};

const writeStoredValue = (key: string, value: string | null | undefined): void => {
	const storage = safeStorage();
	/* v8 ignore start */
	if (!storage || value === undefined) return;
	/* v8 ignore stop */
	if (value === null) {
		storage.removeItem(key);
		return;
	}
	storage.setItem(key, value);
};

export const emptyUserPreferences = (): UserPreferences => {
	const preferences = {} as UserPreferences;
	preferences.selected_space_id = null;
	preferences.locale = null;
	preferences.ui_theme = null;
	preferences.color_mode = null;
	preferences.primary_color = null;
	return preferences;
};

export const readLocalPreferences = (): UserPreferences => {
	const preferences = emptyUserPreferences();
	preferences.selected_space_id = readStringValue(SELECTED_SPACE_STORAGE_KEY);
	preferences.locale = readAllowedValue(LOCALE_STORAGE_KEY, ["en", "ja"]);
	preferences.ui_theme = readAllowedValue(THEME_STORAGE_KEY, ["materialize", "classic", "pop"]);
	preferences.color_mode = readAllowedValue(MODE_STORAGE_KEY, ["light", "dark"]);
	preferences.primary_color = readAllowedValue(PRIMARY_COLOR_STORAGE_KEY, [
		"violet",
		"blue",
		"emerald",
		"amber",
	]);
	return preferences;
};

export const writeLocalPreferences = (patch: UserPreferencesPatchPayload): void => {
	writeStoredValue(SELECTED_SPACE_STORAGE_KEY, patch.selected_space_id);
	writeStoredValue(LOCALE_STORAGE_KEY, patch.locale);
	writeStoredValue(THEME_STORAGE_KEY, patch.ui_theme);
	writeStoredValue(MODE_STORAGE_KEY, patch.color_mode);
	writeStoredValue(PRIMARY_COLOR_STORAGE_KEY, patch.primary_color);
};
