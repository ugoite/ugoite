import { t } from "~/lib/i18n";

export type CompositionParameterName = {
  id: string;
  label?: string;
};

/** Use a declared human label or a localized ordinal, never the binding key. */
export const compositionParameterDisplayName = (
  parameters: readonly CompositionParameterName[],
  parameterId: string,
): string => {
  const index = parameters.findIndex((parameter) =>
    parameter.id === parameterId
  );
  if (index < 0) return t("composition.studioParameters");
  const parameter = parameters[index];
  const label = parameter.label?.trim();
  return label || t("composition.parameterOrdinal", { number: index + 1 });
};
