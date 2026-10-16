import { t } from "~/lib/i18n";
import {
  type DraftDisplay,
  type DraftSource,
  type DraftVariable,
} from "~/lib/composition-draft";
import { compositionVariableDisplayName } from "~/lib/composition-variable-name";

export const displayValueName = (
  display: DraftDisplay,
): string => {
  if (display.kind === "text") return "";
  if (display.kind === "table") return "";
  return display.column ?? t("composition.studioValue");
};

const sourceName = (
  sources: readonly DraftSource[],
  sourceDraftId: string,
): string =>
  sources.find((source) => source.draftId === sourceDraftId)?.name ?? "";

/** Default block name comes from the source or SQL result column. */
export const displayDefaultName = (
  display: DraftDisplay,
  sources: readonly DraftSource[],
  variables: readonly DraftVariable[],
): string => {
  if (display.kind === "text") {
    return display.content.kind === "literal"
      ? display.content.text || t("composition.studioText")
      : compositionVariableDisplayName(variables, display.content.variable);
  }
  const name = sourceName(sources, display.sourceDraftId);
  if (display.kind === "table") return name;
  const value = displayValueName(display);
  return value ? `${name} / ${value}` : name;
};

export const displayKindLabel = (display: DraftDisplay): string =>
  display.kind === "table"
    ? t("composition.studioTable")
    : display.kind === "metric"
    ? t("composition.studioMetric")
    : t("composition.studioText");
