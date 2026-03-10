import { createEffect, createRoot, createSignal } from "solid-js";
import { isServer } from "solid-js/web";
import { readLocalPreferences, writeLocalPreferences } from "./preferences-local";

export type UiTheme = "materialize" | "classic" | "pop";
export type ColorMode = "light" | "dark";
export type PrimaryColor = "violet" | "blue" | "emerald" | "amber";

const readStoredTheme = (): UiTheme | null => {
	const value = readLocalPreferences().ui_theme;
	/* v8 ignore start */
	if (value === "materialize" || value === "classic" || value === "pop") {
		return value;
	}
	/* v8 ignore stop */
	return null;
};

const readStoredMode = (): ColorMode | null => {
	const value = readLocalPreferences().color_mode;
	/* v8 ignore start */
	if (value === "light" || value === "dark") {
		return value;
	}
	/* v8 ignore stop */
	return null;
};

const readStoredPrimaryColor = (): PrimaryColor | null => {
	const value = readLocalPreferences().primary_color;
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
		const nextTheme = theme();
		const nextMode = mode();
		const nextPrimaryColor = primaryColor();

		applyThemeAttributes(nextTheme, nextMode, nextPrimaryColor);

		const patch = {} as import("./types").UserPreferencesPatchPayload;
		patch.ui_theme = nextTheme;
		patch.color_mode = nextMode;
		patch.primary_color = nextPrimaryColor;
		writeLocalPreferences(patch);
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
