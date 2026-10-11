import { For, Show } from "solid-js";
import {
  CompositionEntryQueryTable,
  type CompositionFieldTypeLookup,
} from "~/components/CompositionEntryQueryTable";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { ResultPagination } from "~/components/ResultPagination";
import { SqlResultTable } from "~/components/SqlResultTable";
import { t, type TranslationKey } from "~/lib/i18n";
import {
  type CompositionResolvedComponentBinding,
  type CompositionResolveDiagnostic,
  type CompositionResolvedSource,
  type CompositionResolvePlan,
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

export function CompositionMetric(props: {
  binding: Extract<CompositionResolvedComponentBinding, { kind: "metric" }>;
  source: CompositionResolvedSource;
  sourceState?: CompositionSourcePageState;
  ownsSourceStatus: boolean;
  onRetry: () => void;
}) {
  return (
    <section class="section">
      <Show when={props.binding.label}>
        <h2>{props.binding.label}</h2>
      </Show>
      <Show
        when={props.ownsSourceStatus &&
          (!props.sourceState || props.sourceState.status === "loading")}
      >
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show when={props.ownsSourceStatus && props.sourceState?.status === "error"}>
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
    // Text bindings carry no source binding and trigger no source request.
    if (binding.kind === "text") continue;
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
          if (binding.kind === "metric") {
            return (
              <CompositionMetric
                binding={binding}
                source={source}
                sourceState={sourceState()}
                ownsSourceStatus={ownsSourceStatus}
                onRetry={() => props.onRetry(binding.source_id)}
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
