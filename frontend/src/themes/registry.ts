import type { UiTheme } from "~/lib/ui-theme";

export interface ThemeDefinition {
	id: UiTheme;
	label: string;
}

export const UI_THEMES: ThemeDefinition[] = [
	{ id: "materialize", label: "Materialize" },
	{ id: "classic", label: "Classic" },
	{ id: "pop", label: "Pop" },
];
