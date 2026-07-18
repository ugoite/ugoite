import { isServer } from "solid-js/web";
import type { UserPreferences, UserPreferencesPatchPayload } from "./types";

const SELECTED_SPACE_STORAGE_KEY = "ugoite-selected-space";
const LOCALE_STORAGE_KEY = "ugoite-locale";
const MODE_STORAGE_KEY = "ugoite-color-mode";
const CONTENT_WIDTH_STORAGE_KEY = "ugoite-content-width";

export const LOCAL_PREFERENCE_KEYS = {
  selectedSpaceId: SELECTED_SPACE_STORAGE_KEY,
  locale: LOCALE_STORAGE_KEY,
  colorMode: MODE_STORAGE_KEY,
  contentWidth: CONTENT_WIDTH_STORAGE_KEY,
} as const;

const safeStorage = () => {
  /* v8 ignore start */
  if (isServer || typeof window === "undefined") return null;
  /* v8 ignore stop */
  const storage = window.localStorage;
  if (
    !storage ||
    typeof storage.getItem !== "function" ||
    typeof storage.setItem !== "function" ||
    typeof storage.removeItem !== "function"
  ) {
    return null;
  }
  return storage;
};

const readAllowedValue = <T extends string>(
  key: string,
  allowed: readonly T[],
): T | null => {
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

const writeStoredValue = (
  key: string,
  value: string | null | undefined,
): void => {
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
  preferences.color_mode = null;
  preferences.content_width = null;
  return preferences;
};

export const readLocalPreferences = (): UserPreferences => {
  const preferences = emptyUserPreferences();
  preferences.selected_space_id = readStringValue(SELECTED_SPACE_STORAGE_KEY);
  preferences.locale = readAllowedValue(LOCALE_STORAGE_KEY, ["en", "ja"]);
  preferences.color_mode = readAllowedValue(MODE_STORAGE_KEY, [
    "light",
    "dark",
  ]);
  preferences.content_width = readAllowedValue(CONTENT_WIDTH_STORAGE_KEY, [
    "standard",
    "wide",
  ]);
  return preferences;
};

export const writeLocalPreferences = (
  patch: UserPreferencesPatchPayload,
): void => {
  writeStoredValue(SELECTED_SPACE_STORAGE_KEY, patch.selected_space_id);
  writeStoredValue(LOCALE_STORAGE_KEY, patch.locale);
  writeStoredValue(MODE_STORAGE_KEY, patch.color_mode);
  writeStoredValue(CONTENT_WIDTH_STORAGE_KEY, patch.content_width);
};
