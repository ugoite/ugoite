import { t } from "~/lib/i18n";
import type { DraftDisplay, DraftSource } from "~/lib/composition-draft";

export const displayValueName = (
  display: DraftDisplay,
  sources: readonly DraftSource[] = [],
  fieldNames?: (formId: string, fieldId: number) => string | undefined,
): string => {
  if (display.kind === "text") return "";
  if (display.kind === "table") return "";
  if (display.definition.kind === "legacy_value_field") {
    const valueField = display.definition.valueField;
    if (valueField.kind === "sql_column") return valueField.name;
    const source = sources.find((entry) =>
      entry.draftId === display.sourceDraftId
    );
    return source?.kind === "entry_query"
      ? fieldNames?.(source.formId, valueField.fieldId) ??
        t("composition.studioValue")
      : t("composition.studioValue");
  }
  if (display.definition.kind === "sql_scalar") {
    return display.definition.column ?? t("composition.studioValue");
  }
  const operation = t(
    `composition.metricOperation.${display.definition.operation}` as
      | "composition.metricOperation.count_rows"
      | "composition.metricOperation.count_values"
      | "composition.metricOperation.sum"
      | "composition.metricOperation.avg"
      | "composition.metricOperation.min"
      | "composition.metricOperation.max",
  );
  if (display.definition.operation === "count_rows") return operation;
  const source = sources.find((entry) =>
    entry.draftId === display.sourceDraftId
  );
  if (
    source?.kind !== "entry_query" || display.definition.fieldId === undefined
  ) {
    return operation;
  }
  return fieldNames?.(source.formId, display.definition.fieldId) ?? operation;
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
