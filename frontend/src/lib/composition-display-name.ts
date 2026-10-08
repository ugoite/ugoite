import { t } from "~/lib/i18n";
import type { DraftDisplay, DraftSource } from "~/lib/composition-draft";

export const displayValueName = (
  display: DraftDisplay,
  sources: readonly DraftSource[] = [],
  fieldNames?: (formId: string, fieldId: number) => string | undefined,
): string => {
  if (display.kind === "text") return "";
  if (display.kind === "table") return "";
  if ("column" in display.valueField) return display.valueField.column;
  const source = sources.find((entry) =>
    entry.draftId === display.sourceDraftId
  );
  if (source?.kind !== "entry_query") return t("composition.studioValue");
  const field = source.fieldSchema.find((entry) =>
    entry.field_id === display.valueField.fieldId
  );
  return fieldNames?.(source.formId, display.valueField.fieldId) ??
    t("composition.studioFieldIndex", {
      index: field
        ? source.fieldSchema.findIndex((entry) =>
          entry.field_id === field.field_id
        ) + 1
        : 1,
    });
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
  fieldNames?: (formId: string, fieldId: number) => string | undefined,
): string => {
  if (display.label) return display.label;
  if (display.kind === "text") {
    return display.text || t("composition.studioText");
  }
  const name = sourceName(sources, display.sourceDraftId);
  if (display.kind === "table") return name;
  const value = displayValueName(display, sources, fieldNames);
  return value ? `${name} / ${value}` : name;
};

export const displayKindLabel = (display: DraftDisplay): string =>
  display.kind === "table"
    ? t("composition.studioTable")
    : display.kind === "metric"
    ? t("composition.studioMetric")
    : t("composition.studioText");
