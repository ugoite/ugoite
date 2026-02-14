import type { PrimaryColor } from "~/lib/ui-theme";
import sharedPrimaryColors from "../../../shared/themes/primary-color-definitions.json";

export interface PrimaryColorDefinition {
	id: PrimaryColor;
	label: string;
}

export const PRIMARY_COLORS: PrimaryColorDefinition[] = [
	...sharedPrimaryColors,
] as PrimaryColorDefinition[];