import { createResource, For, Show } from "solid-js";
import type { JSX } from "solid-js";
import {
  type CompositionInspectorDataJump,
  type InspectorApply,
  InspectorDataJumpButton,
  InspectorLabelField,
  InspectorShell,
  InspectorSourceField,
} from "~/components/composition/InspectorFields";
import {
  entryQueryAggregateCandidates,
  savedSqlScalarCandidates,
} from "~/components/CompositionDisplayPicker";
import {
  type CompositionDraft,
  type DraftAggregateOperation,
  type DraftDisplay,
  type DraftMetricPresentation,
  metricDefinitionHasNumericResult,
  setMetricDefinition,
  setMetricPresentation,
  setMetricSource,
} from "~/lib/composition-draft";
import { supportedCompositionAggregateFieldTypes } from "~/lib/composition-metric-capability";
import { t } from "~/lib/i18n";

const aggregateOperations: readonly DraftAggregateOperation[] = [
  "count_rows",
  "count_values",
  "sum",
  "avg",
  "min",
  "max",
];

const operationLabel = (operation: DraftAggregateOperation) =>
  t(
    `composition.metricOperation.${operation}` as
      | "composition.metricOperation.count_rows"
      | "composition.metricOperation.count_values"
      | "composition.metricOperation.sum"
      | "composition.metricOperation.avg"
      | "composition.metricOperation.min"
      | "composition.metricOperation.max",
  );

/** Operation-oriented metric editor. IDs remain bindings and never become labels. */
export function MetricInspector(props: {
  draft: CompositionDraft;
  display: DraftDisplay;
  fieldNames?: (formId: string, fieldId: number) => string | undefined;
  fieldNamesLoading?: boolean;
  apply: InspectorApply;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
  actions?: JSX.Element;
}) {
  const metric = () =>
    props.display.kind === "metric" ? props.display : undefined;
  const sourceId = () => metric()?.sourceDraftId ?? "";
  const source = () =>
    props.draft.sources.find((entry) => entry.draftId === sourceId());
  const operation = () => {
    const definition = metric()?.definition;
    return definition?.kind === "aggregate" ? definition.operation : undefined;
  };
  const [fieldCapabilities] = createResource(
    () => {
      const currentSource = source();
      const currentOperation = operation();
      if (
        currentSource?.kind !== "entry_query" ||
        currentOperation === undefined || currentOperation === "count_rows"
      ) return undefined;
      return {
        sourceDraftId: currentSource.draftId,
        operation: currentOperation,
        fieldSchema: currentSource.fieldSchema,
      };
    },
    async ({ sourceDraftId, operation, fieldSchema }) => ({
      sourceDraftId,
      operation,
      supportedFieldTypes: await supportedCompositionAggregateFieldTypes(
        operation,
        fieldSchema.map((field) => field.field_type),
      ),
    }),
  );
  const currentSupportedFieldTypes = (): ReadonlySet<string> => {
    const result = fieldCapabilities();
    return result?.sourceDraftId === sourceId() &&
        result.operation === operation()
      ? result.supportedFieldTypes
      : new Set<string>();
  };
  const fieldCandidates = () => {
    const currentSource = source();
    const currentOperation = operation();
    return currentSource?.kind === "entry_query" && currentOperation &&
        currentOperation !== "count_rows"
      ? entryQueryAggregateCandidates(
        currentSource.fieldSchema,
        currentSource.formId,
        currentOperation,
        currentSupportedFieldTypes(),
        props.fieldNames,
      )
      : [];
  };
  const sqlCandidates = () => {
    const currentSource = source();
    return currentSource?.kind === "saved_sql"
      ? savedSqlScalarCandidates(currentSource.expectedResult)
      : [];
  };
  const fieldValue = () => {
    const definition = metric()?.definition;
    return definition?.kind === "aggregate" &&
        definition.fieldId !== undefined
      ? String(definition.fieldId)
      : "";
  };
  const columnValue = () => {
    const definition = metric()?.definition;
    return definition?.kind === "sql_scalar" ? definition.column ?? "" : "";
  };
  const supportsNumberPresentation = () => {
    const currentMetric = metric();
    const currentSource = source();
    if (!currentMetric || !currentSource) return false;
    return metricDefinitionHasNumericResult(
      currentSource,
      currentMetric.definition,
    );
  };

  const updatePresentation = (
    values: Omit<DraftMetricPresentation, "kind">,
  ) => {
    const current = metric();
    if (!current) return;
    props.apply(
      setMetricPresentation(props.draft, current.draftId, {
        kind: "number",
        ...(values.unit === undefined ? {} : { unit: values.unit }),
        ...(values.fractionDigits === undefined
          ? {}
          : { fractionDigits: values.fractionDigits }),
      }),
    );
  };

  const pickUnit = (unit: string) => {
    const fractionDigits = metric()?.presentation?.fractionDigits;
    updatePresentation({
      unit,
      ...(fractionDigits === undefined ? {} : { fractionDigits }),
    });
  };

  const pickFractionDigits = (value: string) => {
    const unit = metric()?.presentation?.unit;
    updatePresentation({
      ...(unit === undefined ? {} : { unit }),
      ...(value === "" ? {} : { fractionDigits: Number(value) }),
    });
  };

  const pickSource = (nextSourceDraftId: string) => {
    const current = metric();
    if (!current || nextSourceDraftId === current.sourceDraftId) return;
    props.apply(
      setMetricSource(props.draft, current.draftId, nextSourceDraftId),
    );
  };

  const pickOperation = (nextOperation: DraftAggregateOperation) => {
    const current = metric();
    if (!current) return;
    props.apply(setMetricDefinition(props.draft, current.draftId, {
      kind: "aggregate",
      operation: nextOperation,
    }));
  };

  const pickField = (fieldId: string) => {
    const current = metric();
    const currentOperation = operation();
    if (!current || !currentOperation || currentOperation === "count_rows") {
      return;
    }
    props.apply(setMetricDefinition(props.draft, current.draftId, {
      kind: "aggregate",
      operation: currentOperation,
      ...(fieldId ? { fieldId: Number(fieldId) } : {}),
    }));
  };

  const pickColumn = (column: string) => {
    const current = metric();
    if (!current) return;
    props.apply(setMetricDefinition(props.draft, current.draftId, {
      kind: "sql_scalar",
      ...(column ? { column } : {}),
    }));
  };

  return (
    <InspectorShell
      icon="canvas-metric"
      title={t("composition.studioMetric")}
      actions={props.actions}
    >
      <Show when={metric()}>
        {(entry) => (
          <>
            <InspectorLabelField
              display={entry()}
              draft={props.draft}
              apply={props.apply}
            />
            <InspectorSourceField
              draft={props.draft}
              sourceDraftId={sourceId()}
              fieldId={`studio-inspector-source-${entry().draftId}`}
              onPick={pickSource}
            />
            <Show when={source()?.kind === "entry_query"}>
              <div class="ui-field">
                <label
                  class="ui-label"
                  for={`studio-inspector-operation-${entry().draftId}`}
                >
                  {t("composition.studioOperation")}
                </label>
                <select
                  id={`studio-inspector-operation-${entry().draftId}`}
                  class="ui-input"
                  value={operation() ?? "count_rows"}
                  onChange={(event) =>
                    pickOperation(
                      event.currentTarget.value as DraftAggregateOperation,
                    )}
                >
                  <For each={aggregateOperations}>
                    {(value) => (
                      <option value={value}>{operationLabel(value)}</option>
                    )}
                  </For>
                </select>
              </div>
              <Show when={operation() && operation() !== "count_rows"}>
                <div class="ui-field">
                  <label
                    class="ui-label"
                    for={`studio-inspector-field-${entry().draftId}`}
                  >
                    {t("composition.studioField")}
                  </label>
                  <select
                    id={`studio-inspector-field-${entry().draftId}`}
                    class="ui-input"
                    value={fieldValue()}
                    disabled={props.fieldNamesLoading ||
                      fieldCapabilities.loading}
                    onChange={(event) => pickField(event.currentTarget.value)}
                  >
                    <option value="" disabled>
                      {t("composition.studioChooseField")}
                    </option>
                    <For each={fieldCandidates()}>
                      {(candidate) => (
                        <option value={candidate.id}>{candidate.name}</option>
                      )}
                    </For>
                  </select>
                </div>
              </Show>
            </Show>
            <Show when={source()?.kind === "saved_sql"}>
              <div class="ui-field">
                <label
                  class="ui-label"
                  for={`studio-inspector-column-${entry().draftId}`}
                >
                  {t("composition.studioColumn")}
                </label>
                <select
                  id={`studio-inspector-column-${entry().draftId}`}
                  class="ui-input"
                  value={columnValue()}
                  onChange={(event) => pickColumn(event.currentTarget.value)}
                >
                  <option value="" disabled>
                    {t("composition.studioChooseColumn")}
                  </option>
                  <For each={sqlCandidates()}>
                    {(candidate) => (
                      <option value={candidate.id}>{candidate.name}</option>
                    )}
                  </For>
                </select>
              </div>
            </Show>
            <Show when={supportsNumberPresentation()}>
              <div class="ui-field">
                <label
                  class="ui-label"
                  for={`studio-inspector-metric-unit-${entry().draftId}`}
                >
                  {t("composition.studioMetricUnit")}
                </label>
                <input
                  id={`studio-inspector-metric-unit-${entry().draftId}`}
                  class="ui-input"
                  value={entry().presentation?.unit ?? ""}
                  onInput={(event) => pickUnit(event.currentTarget.value)}
                  onBlur={(event) => pickUnit(event.currentTarget.value.trim())}
                />
              </div>
              <div class="ui-field">
                <label
                  class="ui-label"
                  for={`studio-inspector-metric-fraction-digits-${entry().draftId}`}
                >
                  {t("composition.studioMetricFractionDigits")}
                </label>
                <input
                  id={`studio-inspector-metric-fraction-digits-${entry().draftId}`}
                  class="ui-input"
                  type="number"
                  min="0"
                  max="255"
                  step="1"
                  value={entry().presentation?.fractionDigits?.toString() ?? ""}
                  onInput={(event) =>
                    pickFractionDigits(event.currentTarget.value)}
                />
              </div>
            </Show>
            <InspectorDataJumpButton
              sourceDraftId={sourceId()}
              onDataJump={props.onDataJump}
            />
          </>
        )}
      </Show>
    </InspectorShell>
  );
}
