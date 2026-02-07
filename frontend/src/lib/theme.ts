export type UiTheme = "materialize" | "classic" | "pop";
export type UiTone = "light" | "dark";

type UiThemeState = {
	theme: UiTheme;
	tone: UiTone;
};

const DEFAULT_THEME: UiTheme = "materialize";
const DEFAULT_TONE: UiTone = "light";
const THEME_KEY = "ieapp-ui-theme";
const TONE_KEY = "ieapp-ui-tone";

const isUiTheme = (value: string): value is UiTheme =>
	value === "materialize" || value === "classic" || value === "pop";

const isUiTone = (value: string): value is UiTone => value === "light" || value === "dark";

export const resolveTheme = (): UiThemeState => {
	if (typeof window === "undefined") {
		return { theme: DEFAULT_THEME, tone: DEFAULT_TONE };
	}
	const storedTheme = window.localStorage.getItem(THEME_KEY) ?? "";
	const storedTone = window.localStorage.getItem(TONE_KEY) ?? "";
	return {
		theme: isUiTheme(storedTheme) ? storedTheme : DEFAULT_THEME,
		tone: isUiTone(storedTone) ? storedTone : DEFAULT_TONE,
	};
};

export const applyTheme = (theme: UiTheme, tone: UiTone) => {
	if (typeof document === "undefined") {
		return;
	}
	const root = document.documentElement;
	root.setAttribute("data-ui-theme", theme);
	root.setAttribute("data-ui-tone", tone);
	root.classList.toggle("dark", tone === "dark");
};

export const persistTheme = (theme: UiTheme, tone: UiTone) => {
	if (typeof window === "undefined") {
		return;
	}
	window.localStorage.setItem(THEME_KEY, theme);
	window.localStorage.setItem(TONE_KEY, tone);
};
