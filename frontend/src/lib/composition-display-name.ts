import { t } from "~/lib/i18n";
import type { DraftDisplay, DraftSource } from "~/lib/composition-draft";

export const displayValueName = (display: DraftDisplay): string => {
  if (display.kind === "text") return "";
  return display.kind === "table"
    ? ""
    : "column" in display.valueField
    ? display.valueField.column
    : `#${display.valueField.fieldId}`;
};

const sourceName = (
  sources: readonly DraftSource[],
  sourceDraftId: string,
): string =>
  sources.find((source) => source.draftId === sourceDraftId)?.name ?? "";

/** Default block name: the label, else the source name (plus value for metrics). */
export const displayDefaultName = (
  display: DraftDisplay,
  sources: readonly DraftSource[],
): string => {
  if (display.label) return display.label;
  if (display.kind === "text") return display.text || display.draftId;
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
