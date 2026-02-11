import { createEffect, createRoot, createSignal } from "solid-js";
import { isServer } from "solid-js/web";

export type UiTheme = "materialize" | "classic" | "pop";
export type ColorMode = "light" | "dark";

const THEME_STORAGE_KEY = "ugoite-ui-theme";
const MODE_STORAGE_KEY = "ugoite-color-mode";

const safeStorage = () => {
	if (isServer || typeof window === "undefined") return null;
	return window.localStorage;
};

const readStoredTheme = (): UiTheme | null => {
	const storage = safeStorage();
	if (!storage) return null;
	const value = storage.getItem(THEME_STORAGE_KEY);
	if (value === "materialize" || value === "classic" || value === "pop") {
		return value;
	}
	return null;
};

const readStoredMode = (): ColorMode | null => {
	const storage = safeStorage();
	if (!storage) return null;
	const value = storage.getItem(MODE_STORAGE_KEY);
	if (value === "light" || value === "dark") {
		return value;
	}
	return null;
};

const resolveSystemMode = (): ColorMode => {
	if (isServer || typeof window === "undefined") return "light";
	if (typeof window.matchMedia !== "function") {
		return "light";
	}
	return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
};

const applyThemeAttributes = (theme: UiTheme, mode: ColorMode) => {
	if (isServer || typeof document === "undefined") return;
	const root = document.documentElement;
	root.dataset.uiTheme = theme;
	root.dataset.colorMode = mode;
};

const themeStore = createRoot(() => {
	const [theme, setTheme] = createSignal<UiTheme>(readStoredTheme() ?? "materialize");
	const [mode, setMode] = createSignal<ColorMode>(readStoredMode() ?? resolveSystemMode());

	createEffect(() => {
		const storage = safeStorage();
		const nextTheme = theme();
		const nextMode = mode();

		applyThemeAttributes(nextTheme, nextMode);

		storage?.setItem(THEME_STORAGE_KEY, nextTheme);
		storage?.setItem(MODE_STORAGE_KEY, nextMode);
	});

	return {
		theme,
		setTheme,
		mode,
		setMode,
	};
});

export const uiTheme = themeStore.theme;
export const setUiTheme = themeStore.setTheme;
export const colorMode = themeStore.mode;
export const setColorMode = themeStore.setMode;

export const initializeUiTheme = () => {
	applyThemeAttributes(uiTheme(), colorMode());
};
