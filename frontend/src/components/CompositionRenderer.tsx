import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import {
  CompositionEntryQueryTable,
  type CompositionFieldTypeLookup,
} from "~/components/CompositionEntryQueryTable";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { ResultPagination } from "~/components/ResultPagination";
import { SqlResultTable } from "~/components/SqlResultTable";
import { t, type TranslationKey } from "~/lib/i18n";
import {
  compositionApi,
  type CompositionMetricPresentation,
  type CompositionMetricQueryResponse,
  type CompositionMetricQueryTarget,
  type CompositionResolvedComponentBinding,
  type CompositionResolveDiagnostic,
  type CompositionResolvedSource,
  type CompositionResolvePlan,
  type CompositionResultType,
  type CompositionTextStyle,
} from "~/lib/composition-api";
import { TextComponent } from "~/components/composition/TextComponent";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";

export type CompositionFieldNames = (
  formId: string,
  fieldId: number,
  sourceId?: string,
) => string | undefined;

export type CompositionFieldKeys = (
  formId: string,
  fieldId: number,
  sourceId?: string,
) => string | undefined;

export type { CompositionFieldTypeLookup };

type SavedRevisionMetricQueryContext = {
  spaceId: string;
  target: Extract<CompositionMetricQueryTarget, { kind: "saved_revision" }>;
  parameters: Record<string, unknown>;
  onEdit?: () => void;
};

type DraftMetricQueryContext = {
  spaceId: string;
  target: Extract<CompositionMetricQueryTarget, { kind: "draft" }>;
  draftFingerprint: string;
  parameters: Record<string, unknown>;
  onEdit?: () => void;
};

export type CompositionMetricQueryContext =
  | SavedRevisionMetricQueryContext
  | DraftMetricQueryContext;

type RendererProps = {
  /**
   * Resolved sources with their component bindings. Saved revisions pass
   * the full resolve plan; unsaved draft previews pass the preview plan,
   * which carries the same sources and bindings without a revision.
   */
  plan: Pick<CompositionResolvePlan, "sources" | "component_bindings">;
  sources: Record<string, CompositionSourcePageState>;
  /**
   * Text content joined by component id. Surfaces without the component
   * declarations (draft previews) omit this and skip text bindings.
   */
  texts?: Readonly<
    Record<string, { text: string; style: CompositionTextStyle }>
  >;
  fieldNames?: CompositionFieldNames;
  fieldKeys?: CompositionFieldKeys;
  fieldTypes?: CompositionFieldTypeLookup;
  /** Exact saved revision identity and transient parameters for server metrics. */
  metricQueryContext?: CompositionMetricQueryContext;
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
  "metric_result_not_scalar",
  "metric_result_type_mismatch",
  "metric_scalar_expected_one_row",
  "metric_result_column_missing",
  "metric_result_column_ambiguous",
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

const isNormalizedInteger = (value: string): boolean =>
  /^-?(?:0|[1-9]\d*)$/.test(value) && value !== "-0";

const isNormalizedDecimal = (value: string): boolean => {
  const match = /^(-?)(0|[1-9]\d*)(?:\.(\d+))?$/.exec(value);
  if (!match) return false;
  const [, sign, integer, fraction] = match;
  const isZero = integer === "0" && (!fraction || /^0+$/.test(fraction));
  return !(sign === "-" && isZero);
};

const isValidMetricScalar = (
  type: CompositionResultType,
  value: unknown,
): boolean => {
  if (value === null) return true;
  switch (type) {
    case "integer":
      return typeof value === "string" && isNormalizedInteger(value);
    case "decimal":
      return typeof value === "string" && isNormalizedDecimal(value);
    case "float":
      return typeof value === "number" && Number.isFinite(value);
    case "boolean":
      return typeof value === "boolean";
    case "string":
    case "date":
    case "timestamp":
      return typeof value === "string";
    case "json":
      return false;
  }
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const expandScientificNotation = (value: string): string => {
  const match = /^(-?)(\d+)(?:\.(\d+))?[eE]([+-]?\d+)$/.exec(value);
  if (!match) return value;
  const [, sign, integer, fraction = "", exponentText] = match;
  const digits = `${integer}${fraction}`;
  const decimalIndex = integer.length + Number(exponentText);
  if (decimalIndex <= 0) {
    return `${sign}0.${"0".repeat(-decimalIndex)}${digits}`;
  }
  if (decimalIndex >= digits.length) {
    return `${sign}${digits}${"0".repeat(decimalIndex - digits.length)}`;
  }
  return `${sign}${digits.slice(0, decimalIndex)}.${
    digits.slice(decimalIndex)
  }`;
};

const metricOperations = new Set([
  "sql_scalar",
  "count_rows",
  "count_values",
  "sum",
  "avg",
  "min",
  "max",
]);

const metricResultTypes = new Set<CompositionResultType>([
  "string",
  "boolean",
  "integer",
  "float",
  "decimal",
  "date",
  "timestamp",
  "json",
]);

const isMetricQueryResponse = (
  value: unknown,
): value is CompositionMetricQueryResponse => {
  if (!isRecord(value) || typeof value.ok !== "boolean") return false;
  if (
    value.draft_fingerprint !== undefined &&
    typeof value.draft_fingerprint !== "string"
  ) return false;
  if (
    value.diagnostics !== undefined &&
    (!Array.isArray(value.diagnostics) ||
      !value.diagnostics.every((diagnostic) =>
        isRecord(diagnostic) && typeof diagnostic.code === "string" &&
        (diagnostic.component_id === undefined ||
          typeof diagnostic.component_id === "string") &&
        (diagnostic.parameter_id === undefined ||
          typeof diagnostic.parameter_id === "string")
      ))
  ) return false;
  if (value.result === undefined) return true;
  const result = value.result;
  return isRecord(result) && typeof result.component_id === "string" &&
    typeof result.operation === "string" &&
    metricOperations.has(result.operation) &&
    (result.source_kind === "entry_query" ||
      result.source_kind === "saved_sql") &&
    typeof result.result_type === "string" &&
    metricResultTypes.has(result.result_type as CompositionResultType) &&
    (result.value === null || typeof result.value === "string" ||
      typeof result.value === "number" || typeof result.value === "boolean");
};

const formatMetricValue = (
  type: CompositionResultType,
  value: unknown,
  presentation?: CompositionMetricPresentation,
): string => {
  if (typeof value !== "string" && typeof value !== "number") {
    return displayCell(value);
  }
  let formatted = type === "float" && typeof value === "number"
    ? expandScientificNotation(String(value))
    : String(value);
  if (
    presentation?.kind === "number" &&
    (type === "integer" || type === "decimal" || type === "float")
  ) {
    const fractionDigits = presentation.fraction_digits;
    if (fractionDigits !== undefined) {
      const decimalPoint = formatted.indexOf(".");
      if (decimalPoint === -1) {
        if (fractionDigits > 0) formatted += `.${"0".repeat(fractionDigits)}`;
      } else {
        const scale = formatted.length - decimalPoint - 1;
        if (scale < fractionDigits) {
          formatted += "0".repeat(fractionDigits - scale);
        }
      }
    }
    const unit = presentation.unit?.trim();
    if (unit) formatted += ` ${unit}`;
  }
  return formatted;
};

export function CompositionSavedSqlTable(props: {
  binding: Extract<CompositionResolvedComponentBinding, { kind: "table" }>;
  source: Extract<CompositionResolvedSource, { kind: "saved_sql" }>;
  sourceState?: CompositionSourcePageState;
  ownsSourceStatus: boolean;
  onNext: () => void;
  onPrevious: () => void;
  onRetry: () => void;
}) {
  const page = () => {
    const state = props.sourceState?.page;
    return state?.kind === "saved_sql" ? state.page : undefined;
  };
  const rows = () => page()?.rows ?? [];
  const sourceId = () => props.binding.source_id;
  const status = () => props.sourceState?.status;
  const loading = () =>
    props.ownsSourceStatus &&
    (!props.sourceState || status() === "loading");
  const failed = () => props.ownsSourceStatus && status() === "error";

  return (
    <section class="section">
      <Show when={props.binding.label}>
        <h2>{props.binding.label}</h2>
      </Show>
      <Show
        when={props.ownsSourceStatus || status() === "ready"}
      >
        <Show when={loading()}>
          <LocalBusyIndicator label={t("composition.queryLoading")} />
        </Show>
        <Show when={failed()}>
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
        <Show when={status() === "ready" && rows().length === 0 && !failed()}>
          <p class="ui-muted">{t("composition.queryEmpty")}</p>
        </Show>
        <Show when={!failed() && rows().length > 0}>
          <SqlResultTable
            columns={page()?.columns ?? []}
            columnTypes={page()?.result_schema?.map((column) => column.type)}
            rows={rows()}
            exactIntegerRows={page()?.exact_integer_rows}
            pageIdentity={`${sourceId()}:${
              props.sourceState?.cursor ?? "first"
            }`}
            tableLabel={props.binding.label ?? t("composition.resultPages")}
          />
        </Show>
        <ResultPagination
          canPrevious={(props.sourceState?.cursorStack.length ?? 0) > 1}
          canNext={!!page()?.has_more && !!page()?.next}
          busy={status() !== "ready"}
          previousLabel={t("composition.previous")}
          nextLabel={t("composition.next")}
          ariaLabel={t("composition.resultPages")}
          onPrevious={props.onPrevious}
          onNext={props.onNext}
        />
      </Show>
    </section>
  );
}

export const usesServerMetricQuery = (
  binding: CompositionResolvedComponentBinding,
): binding is Extract<
  CompositionResolvedComponentBinding,
  { kind: "metric" }
> => binding.kind === "metric";

export function CompositionMetric(props: {
  binding: Extract<CompositionResolvedComponentBinding, { kind: "metric" }>;
  metricQueryContext?: CompositionMetricQueryContext;
}) {
  const [serverResult, setServerResult] = createSignal<
    CompositionMetricQueryResponse | undefined
  >();
  const [serverLoading, setServerLoading] = createSignal(false);
  const [serverFailed, setServerFailed] = createSignal(false);
  const [serverInvalid, setServerInvalid] = createSignal(false);
  const [serverRetry, setServerRetry] = createSignal(0);
  let serverGeneration = 0;
  let serverController: AbortController | undefined;
  let lastRequestSignature: string | undefined;
  let lastRequestRetry: number | undefined;

  createEffect(() => {
    const context = props.metricQueryContext;
    const retry = serverRetry();
    const requestSignature = context
      ? JSON.stringify([
        context.spaceId,
        context.target.kind === "saved_revision"
          ? [
            context.target.kind,
            context.target.composition_id,
            context.target.revision_id,
          ]
          : [
            context.target.kind,
            context.target.yaml,
            context.draftFingerprint,
          ],
        Object.entries(context.parameters)
          .sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0)
          .map(([key, value]) => [
            key,
            value === undefined ? { kind: "undefined" } : value,
          ]),
      ])
      : "no-context";
    if (
      requestSignature === lastRequestSignature &&
      retry === lastRequestRetry
    ) return;
    lastRequestSignature = requestSignature;
    lastRequestRetry = retry;
    const requestGeneration = ++serverGeneration;
    serverController?.abort();
    serverController = undefined;
    setServerResult(undefined);
    setServerFailed(false);
    setServerInvalid(false);
    if (!context) {
      setServerLoading(false);
      setServerFailed(true);
      return;
    }
    const controller = new AbortController();
    serverController = controller;
    setServerLoading(true);
    void compositionApi.queryMetric(
      context.spaceId,
      {
        target: { ...context.target },
        component_id: props.binding.component_id,
        parameters: { ...context.parameters },
      },
      controller.signal,
    ).then(
      (response) => {
        if (requestGeneration !== serverGeneration) return;
        if (!isMetricQueryResponse(response)) {
          setServerInvalid(true);
          return;
        }
        if (
          (context.target.kind === "draft" &&
            response.draft_fingerprint !== context.draftFingerprint) ||
          (context.target.kind === "saved_revision" &&
            response.draft_fingerprint !== undefined)
        ) {
          setServerInvalid(true);
          return;
        }
        const result = response;
        const hasDiagnostics = (result.diagnostics?.length ?? 0) > 0;
        if (
          (result.ok && (hasDiagnostics || !result.result)) ||
          (!result.ok && (result.result !== undefined || !hasDiagnostics))
        ) {
          setServerInvalid(true);
          return;
        }
        if (result.ok && result.result) {
          const metric = result.result;
          if (
            metric.component_id !== props.binding.component_id ||
            metric.result_type !== props.binding.expected_result_type ||
            !isValidMetricScalar(metric.result_type, metric.value)
          ) {
            setServerInvalid(true);
            return;
          }
        }
        setServerResult(result);
      },
      () => {
        if (requestGeneration === serverGeneration) setServerFailed(true);
      },
    ).finally(() => {
      if (requestGeneration === serverGeneration) setServerLoading(false);
    });
  });

  onCleanup(() => {
    serverGeneration += 1;
    serverController?.abort();
  });

  const value = () => {
    const result = serverResult();
    return result?.ok && result.result
      ? result.result.value === null
        ? t("composition.metricNoData")
        : formatMetricValue(
          result.result.result_type,
          result.result.value,
          props.binding.presentation,
        )
      : undefined;
  };
  const diagnostic = () => {
    const first = serverResult()?.diagnostics?.[0];
    return first
      ? compositionDiagnosticMessage({ code: first.code })
      : undefined;
  };
  const needsEdit = () =>
    serverResult()?.diagnostics?.[0]?.code === "metric_scalar_expected_one_row" &&
    props.metricQueryContext?.onEdit !== undefined;

  return (
    <section class="section">
      <Show when={props.binding.label}>
        <h2>{props.binding.label}</h2>
      </Show>
      <Show when={serverLoading()}>
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show when={!serverLoading() && serverFailed()}>
        <p class="ui-text-danger" role="alert">
          {t("composition.metricUnavailable")}
        </p>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={() => setServerRetry((retry) => retry + 1)}
        >
          {t("composition.retry")}
        </button>
      </Show>
      <Show when={!serverLoading() && serverInvalid()}>
        <p class="ui-text-danger" role="alert">
          {t("composition.metricInvalid")}
        </p>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={() => setServerRetry((retry) => retry + 1)}
        >
          {t("composition.retry")}
        </button>
      </Show>
      <Show when={!serverLoading() && diagnostic()}>
        <p class="ui-text-danger" role="alert">{diagnostic()}</p>
        <Show
          when={needsEdit()}
          fallback={
            <button
              class="ui-button ui-button-secondary"
              type="button"
              onClick={() => setServerRetry((retry) => retry + 1)}
            >
              {t("composition.retry")}
            </button>
          }
        >
          <button
            class="ui-button ui-button-secondary"
            type="button"
            onClick={() => props.metricQueryContext?.onEdit?.()}
          >
            {t("composition.studioEdit")}
          </button>
        </Show>
      </Show>
      <Show
        when={!serverLoading() && !serverFailed() && !serverInvalid() &&
          value() !== undefined}
      >
        <output class="compositionMetric">{value()}</output>
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
    // Metrics use the bounded server operation and never claim a source page.
    if (binding.kind === "text" || usesServerMetricQuery(binding)) continue;
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
          // Text carries no source binding: join content and style by
          // component id, and skip when the caller has no declarations.
          if (binding.kind === "text") {
            const text = props.texts?.[binding.component_id];
            if (!text) return null;
            return <TextComponent text={text.text} style={text.style} />;
          }
          const source = sourceById.get(binding.source_id);
          const sourceState = () => props.sources[binding.source_id];
          if (!source) return null;
          const ownsSourceStatus = sourceStatusOwner.get(binding.source_id) ===
            binding.component_id;
          if (usesServerMetricQuery(binding)) {
            return (
              <CompositionMetric
                binding={binding}
                metricQueryContext={props.metricQueryContext}
              />
            );
          }
          return source.kind === "entry_query"
            ? (
              <CompositionEntryQueryTable
                binding={binding}
                source={source}
                sourceState={sourceState()}
                ownsSourceStatus={ownsSourceStatus}
                fieldNames={props.fieldNames}
                fieldKeys={props.fieldKeys}
                fieldTypes={props.fieldTypes}
                onNext={() => props.onNext(binding.source_id)}
                onPrevious={() => props.onPrevious(binding.source_id)}
                onRetry={() => props.onRetry(binding.source_id)}
              />
            )
            : (
              <CompositionSavedSqlTable
                binding={binding}
                source={source}
                sourceState={sourceState()}
                ownsSourceStatus={ownsSourceStatus}
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
