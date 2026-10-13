import { t } from "~/lib/i18n";

export type CompositionVariableName = {
  id: string;
  label?: string;
};

/** Use a declared human label or a localized ordinal, never the binding key. */
export const compositionVariableDisplayName = (
  variables: readonly CompositionVariableName[],
  variableId: string,
): string => {
  const index = variables.findIndex((variable) => variable.id === variableId);
  if (index < 0) return t("composition.variableFallback");
  const variable = variables[index];
  const label = variable.label?.trim();
  return label || t("composition.variableOrdinal", { number: index + 1 });
};
