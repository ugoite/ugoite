import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  PagedResultTable,
  type ResultColumn,
} from "~/components/PagedResultTable";
import { t, type TranslationKey } from "~/lib/i18n";
import {
  compositionApi,
  type CompositionResolvedComponentBinding,
  type CompositionResolveDiagnostic,
  type CompositionResolvedSource,
  type CompositionResolvePlan,
} from "~/lib/composition-api";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";

type RendererProps = {
  plan: CompositionResolvePlan;
  sources: Record<string, CompositionSourcePageState>;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
};

const knownDiagnosticCodes = new Set([
  "unsupported_format_version",
  "invalid_composition",
  "parameter_unknown",
  "parameter_missing",
  "parameter_type_mismatch",
  "source_unavailable",
  "missing_field",
  "field_type_changed",
  "source_schema_changed",
  "metric_field_not_projected",
  "metric_result_not_scalar",
  "metric_result_type_mismatch",
  "metric_result_empty",
  "metric_result_multiple_rows",
  "metric_result_column_missing",
  "metric_result_column_ambiguous",
  "metric_result_page_incomplete",
]);

export const compositionDiagnosticMessage = (
  diagnostic: CompositionResolveDiagnostic,
  parameterDefinitions: readonly { id: string; label?: string }[] = [],
): string => {
  if (!knownDiagnosticCodes.has(diagnostic.code)) {
    return t("composition.detailFailed");
  }
  const parameter = parameterDefinitions.find((definition) =>
    definition.id === diagnostic.parameter_id
  );
  return t(
    `composition.diagnostic.${diagnostic.code}` as TranslationKey,
    diagnostic.parameter_id
      ? { parameter: parameter?.label ?? diagnostic.parameter_id }
      : undefined,
  );
};

export function CompositionDiagnostics(props: {
  diagnostics: readonly CompositionResolveDiagnostic[];
  parameterDefinitions?: readonly { id: string; label?: string }[];
}) {
  return (
    <ul
      class="ui-text-danger"
      aria-label={t("composition.diagnostics")}
      role="alert"
    >
      <For each={props.diagnostics}>
        {(diagnostic) => (
          <li>
            {compositionDiagnosticMessage(
              diagnostic,
              props.parameterDefinitions,
            )}
          </li>
        )}
      </For>
    </ul>
  );
}

const displayCell = (value: unknown): string => {
  if (value === null || value === undefined) return "—";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  try {
    return JSON.stringify(value) ?? "—";
  } catch {
    return "—";
  }
};

const objectValue = (value: unknown): Record<string, unknown> | undefined =>
  value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;

const rowValue = (row: unknown, column: string, index: number): unknown => {
  if (Array.isArray(row)) return row[index];
  return objectValue(row)?.[column];
};

const columnsFor = (
  page: NonNullable<CompositionSourcePageState["page"]>,
): ResultColumn<unknown>[] => {
  const names = page.kind === "saved_sql"
    ? page.page.columns
    : Object.keys(objectValue(page.page.rows[0]?.properties) ?? {});
  return names.map((name, index) => ({
    key: name,
    label: name,
    cell: (row) =>
      displayCell(
        page.kind === "entry_query"
          ? objectValue((row as { properties?: unknown }).properties)?.[name]
          : rowValue(row, name, index),
      ),
  }));
};

const rowsFor = (
  page: NonNullable<CompositionSourcePageState["page"]>,
): readonly unknown[] => page.page.rows;

const pageInfo = (
  page: NonNullable<CompositionSourcePageState["page"]>,
) => ({ hasMore: page.page.has_more, next: page.page.next });

function CompositionTable(props: {
  binding: Extract<CompositionResolvedComponentBinding, { kind: "table" }>;
  source: CompositionResolvedSource;
  sourceState?: CompositionSourcePageState;
  ownsSourceStatus: boolean;
  onNext: () => void;
  onPrevious: () => void;
  onRetry: () => void;
}) {
  const page = () => props.sourceState?.page;
  const rows = () => page() ? rowsFor(page()!) : [];
  const resultColumns = () => page() ? columnsFor(page()!) : [];
  const paging = () =>
    page() ? pageInfo(page()!) : { hasMore: false, next: undefined };
  const sourceId = () => props.binding.source_id;
  const error = () =>
    props.sourceState?.status === "error" ? t("composition.queryFailed") : null;
  return (
    <section class="section">
      <Show when={props.binding.label}>
        <h2>{props.binding.label}</h2>
      </Show>
      <Show
        when={props.ownsSourceStatus || props.sourceState?.status === "ready"}
      >
        <PagedResultTable
          columns={resultColumns()}
          rows={rows()}
          rowKey={(row, index) => {
            if (page()?.kind === "entry_query") {
              const entry = row as { id?: string; revision_id?: string };
              return `${entry.id ?? "row"}:${entry.revision_id ?? index}`;
            }
            return `${sourceId()}:${
              props.sourceState?.cursor ?? "first"
            }:${index}`;
          }}
          pageIdentity={`${sourceId()}:${props.sourceState?.cursor ?? "first"}`}
          loading={props.ownsSourceStatus &&
            (!props.sourceState || props.sourceState.status === "loading")}
          loadingLabel={t("composition.queryLoading")}
          error={props.ownsSourceStatus ? error() : null}
          emptyLabel={t("composition.queryEmpty")}
          retryLabel={t("composition.retry")}
          onRetry={props.ownsSourceStatus &&
              props.sourceState?.status === "error"
            ? props.onRetry
            : undefined}
          canPrevious={(props.sourceState?.cursorStack.length ?? 0) > 1}
          canNext={paging().hasMore && !!paging().next}
          previousLabel={t("composition.previous")}
          nextLabel={t("composition.next")}
          onPrevious={props.onPrevious}
          onNext={props.onNext}
          paginationLabel={t("composition.resultPages")}
          classNames={{
            table: "ui-table",
            scroll: "ui-table-wrapper mt-4 overflow-x-auto",
          }}
        />
      </Show>
    </section>
  );
}

const metricRequest = (
  binding: Extract<CompositionResolvedComponentBinding, { kind: "metric" }>,
  source: CompositionResolvedSource,
  page: NonNullable<CompositionSourcePageState["page"]>,
) => {
  let selectedColumnCount = 0;
  let selectedValue: unknown;
  let hasSelectedValue = false;

  if (page.kind === "saved_sql") {
    const result = page.page;
    const key = binding.result_property_key;
    const indexes = key === undefined
      ? []
      : result.columns.flatMap((column, index) =>
        column === key ? [index] : []
      );
    selectedColumnCount = indexes.length;
    if (indexes.length === 1 && result.rows.length > 0) {
      const row = result.rows[0];
      if (Array.isArray(row)) {
        selectedValue = row[indexes[0]];
        hasSelectedValue = indexes[0] < row.length;
      } else {
        const values = objectValue(row);
        hasSelectedValue = !!values && Object.hasOwn(values, key!);
        selectedValue = values?.[key!];
      }
    }
  } else {
    const result = page.page;
    const key = binding.result_property_key;
    const projection = source.kind === "entry_query"
      ? source.request.projection
      : undefined;
    selectedColumnCount = projection?.kind === "fields"
      ? projection.fields.filter((field) =>
        field.kind === "property" && field.field_id === binding.metric_field_id
      ).length
      : 0;
    if (result.rows.length > 0 && key !== undefined) {
      const values = objectValue(result.rows[0].properties);
      hasSelectedValue = !!values && Object.hasOwn(values, key);
      selectedValue = values?.[key];
    }
  }

  return {
    expected_result_type: binding.expected_result_type,
    is_complete: !page.page.has_more && page.page.next === undefined,
    row_count: page.page.rows.length,
    selected_column_count: selectedColumnCount,
    ...(hasSelectedValue ? { selected_value: selectedValue } : {}),
  };
};

function CompositionMetric(props: {
  binding: Extract<CompositionResolvedComponentBinding, { kind: "metric" }>;
  source: CompositionResolvedSource;
  sourceState?: CompositionSourcePageState;
  ownsSourceStatus: boolean;
  onRetry: () => void;
}) {
  const [evaluation, setEvaluation] = createSignal<
    Awaited<ReturnType<typeof compositionApi.evaluateMetricPage>> | undefined
  >();
  const [loading, setLoading] = createSignal(false);
  const [failed, setFailed] = createSignal(false);
  let generation = 0;

  createEffect(() => {
    const state = props.sourceState;
    // A metric describes the complete source result, so keep the first-page
    // evaluation while its table is browsing continuation pages.
    if (state?.cursor !== undefined) return;
    const page = state?.page;
    const requestGeneration = ++generation;
    setEvaluation(undefined);
    setFailed(false);
    if (!page || state?.status !== "ready") {
      setLoading(false);
      return;
    }
    setLoading(true);
    void compositionApi.evaluateMetricPage(
      metricRequest(props.binding, props.source, page),
    ).then(
      (result) => {
        if (requestGeneration === generation) setEvaluation(result);
      },
      () => {
        if (requestGeneration === generation) setFailed(true);
      },
    ).finally(() => {
      if (requestGeneration === generation) setLoading(false);
    });
  });

  onCleanup(() => {
    generation += 1;
  });

  const value = () => {
    const result = evaluation();
    return result?.ok ? displayCell(result.value) : undefined;
  };
  const diagnostic = () => {
    const result = evaluation();
    return result && !result.ok
      ? compositionDiagnosticMessage(result.error)
      : undefined;
  };

  return (
    <section class="section">
      <Show when={props.binding.label}>
        <h2>{props.binding.label}</h2>
      </Show>
      <Show when={loading()}>
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show when={!loading() && failed()}>
        <p class="ui-text-danger" role="alert">
          {t("composition.metricUnavailable")}
        </p>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={props.onRetry}
        >
          {t("composition.retry")}
        </button>
      </Show>
      <Show when={!loading() && !failed() && diagnostic()}>
        <p class="ui-text-danger" role="alert">{diagnostic()}</p>
      </Show>
      <Show when={!loading() && !failed() && value() !== undefined}>
        <output class="compositionMetric">{value()}</output>
      </Show>
      <Show
        when={props.ownsSourceStatus &&
          (!props.sourceState || props.sourceState.status === "loading")}
      >
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show
        when={props.ownsSourceStatus && props.sourceState?.status === "error"}
      >
        <p class="ui-text-danger" role="alert">
          {t("composition.queryFailed")}
        </p>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={props.onRetry}
        >
          {t("composition.retry")}
        </button>
      </Show>
    </section>
  );
}

export function CompositionRenderer(props: RendererProps) {
  const sourceById = new Map(
    props.plan.sources.map((source) => [source.source_id, source]),
  );
  const sourceStatusOwner = new Map<string, string>();
  for (const binding of props.plan.component_bindings) {
    if (
      sourceById.has(binding.source_id) &&
      !sourceStatusOwner.has(binding.source_id)
    ) {
      sourceStatusOwner.set(binding.source_id, binding.component_id);
    }
  }
  return (
    <div class="compositionRenderer">
      <For each={props.plan.component_bindings}>
        {(binding) => {
          const source = sourceById.get(binding.source_id);
          const sourceState = () => props.sources[binding.source_id];
          if (!source) return null;
          return binding.kind === "metric"
            ? (
              <CompositionMetric
                binding={binding}
                source={source}
                sourceState={sourceState()}
                ownsSourceStatus={sourceStatusOwner.get(binding.source_id) ===
                  binding.component_id}
                onRetry={() => props.onRetry(binding.source_id)}
              />
            )
            : (
              <CompositionTable
                binding={binding}
                source={source}
                sourceState={sourceState()}
                ownsSourceStatus={sourceStatusOwner.get(binding.source_id) ===
                  binding.component_id}
                onNext={() => props.onNext(binding.source_id)}
                onPrevious={() => props.onPrevious(binding.source_id)}
                onRetry={() => props.onRetry(binding.source_id)}
              />
            );
        }}
      </For>
    </div>
  );
}
