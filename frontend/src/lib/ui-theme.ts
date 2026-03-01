import { createEffect, createRoot, createSignal } from "solid-js";
import { isServer } from "solid-js/web";

export type UiTheme = "materialize" | "classic" | "pop";
export type ColorMode = "light" | "dark";
export type PrimaryColor = "violet" | "blue" | "emerald" | "amber";

const THEME_STORAGE_KEY = "ugoite-ui-theme";
const MODE_STORAGE_KEY = "ugoite-color-mode";
const PRIMARY_COLOR_STORAGE_KEY = "ugoite-primary-color";

const safeStorage = () => {
	/* v8 ignore start */
	if (isServer || typeof window === "undefined") return null;
	/* v8 ignore stop */
	return window.localStorage;
};

const readStoredTheme = (): UiTheme | null => {
	const storage = safeStorage();
	/* v8 ignore start */
	if (!storage) return null;
	/* v8 ignore stop */
	const value = storage.getItem(THEME_STORAGE_KEY);
	/* v8 ignore start */
	if (value === "materialize" || value === "classic" || value === "pop") {
		return value;
	}
	/* v8 ignore stop */
	return null;
};

const readStoredMode = (): ColorMode | null => {
	const storage = safeStorage();
	/* v8 ignore start */
	if (!storage) return null;
	/* v8 ignore stop */
	const value = storage.getItem(MODE_STORAGE_KEY);
	/* v8 ignore start */
	if (value === "light" || value === "dark") {
		return value;
	}
	/* v8 ignore stop */
	return null;
};

const readStoredPrimaryColor = (): PrimaryColor | null => {
	const storage = safeStorage();
	/* v8 ignore start */
	if (!storage) return null;
	/* v8 ignore stop */
	const value = storage.getItem(PRIMARY_COLOR_STORAGE_KEY);
	/* v8 ignore start */
	if (value === "violet" || value === "blue" || value === "emerald" || value === "amber") {
		return value;
	}
	/* v8 ignore stop */
	return null;
};

const resolveSystemMode = (): ColorMode => {
	/* v8 ignore start */
	if (isServer || typeof window === "undefined") return "light";
	if (typeof window.matchMedia !== "function") {
		return "light";
	}
	/* v8 ignore stop */
	/* v8 ignore start */
	return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
	/* v8 ignore stop */
};

const applyThemeAttributes = (theme: UiTheme, mode: ColorMode, primaryColor: PrimaryColor) => {
	/* v8 ignore start */
	if (isServer || typeof document === "undefined") return;
	/* v8 ignore stop */
	const root = document.documentElement;
	root.dataset.uiTheme = theme;
	root.dataset.colorMode = mode;
	root.dataset.primaryColor = primaryColor;
};

const themeStore = createRoot(() => {
	const [theme, setTheme] = createSignal<UiTheme>(readStoredTheme() ?? "materialize");
	const [mode, setMode] = createSignal<ColorMode>(readStoredMode() ?? resolveSystemMode());
	const [primaryColor, setPrimaryColor] = createSignal<PrimaryColor>(
		readStoredPrimaryColor() ?? "violet",
	);

	createEffect(() => {
		const storage = safeStorage();
		const nextTheme = theme();
		const nextMode = mode();
		const nextPrimaryColor = primaryColor();

		applyThemeAttributes(nextTheme, nextMode, nextPrimaryColor);

		storage?.setItem(THEME_STORAGE_KEY, nextTheme);
		storage?.setItem(MODE_STORAGE_KEY, nextMode);
		storage?.setItem(PRIMARY_COLOR_STORAGE_KEY, nextPrimaryColor);
	});

	return {
		theme,
		setTheme,
		mode,
		setMode,
		primaryColor,
		setPrimaryColor,
	};
});

export const uiTheme = themeStore.theme;
export const setUiTheme = themeStore.setTheme;
export const colorMode = themeStore.mode;
export const setColorMode = themeStore.setMode;
export const primaryColor = themeStore.primaryColor;
export const setPrimaryColor = themeStore.setPrimaryColor;

export const initializeUiTheme = () => {
	applyThemeAttributes(uiTheme(), colorMode(), primaryColor());
};
