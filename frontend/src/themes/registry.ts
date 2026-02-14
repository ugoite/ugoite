import type { UiTheme } from "~/lib/ui-theme";
import sharedThemes from "../../../shared/themes/ui-theme-definitions.json";

export interface ThemeDefinition {
	id: UiTheme;
	label: string;
}

export const UI_THEMES: ThemeDefinition[] = [...sharedThemes] as ThemeDefinition[];
