import { For, Show } from "solid-js";
import { CompositionEntryQueryTable } from "~/components/CompositionEntryQueryTable";
import {
  type CompositionFieldKeys,
  type CompositionFieldNames,
  type CompositionFieldTypeLookup,
  CompositionMetric,
  type CompositionMetricQueryContext,
  CompositionSavedSqlTable,
  usesServerMetricQuery,
} from "~/components/CompositionRenderer";
import { ParameterControl } from "~/components/composition/ParameterControl";
import { TextComponent } from "~/components/composition/TextComponent";
import { t } from "~/lib/i18n";
import type {
  CompositionFlowLayout,
  CompositionFlowLayoutItem,
  CompositionVariableDefinition,
  CompositionResolvedComponentBinding,
  CompositionResolvePlan,
  CompositionTextStyle,
} from "~/lib/composition-api";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import { compositionParameterDisplayName } from "~/lib/composition-parameter-name";

export type DashboardFlowTexts = Record<
  string,
  { text: string; style: CompositionTextStyle }
>;

/**
 * First layout-ordered owner wins each source status: blocks sharing one
 * source show a single loading or error state. Shared with the Design
 * canvas so edit mode and saved Tools agree on status ownership.
 */
export const flowSourceStatusOwner = (
  rows: ReadonlyArray<{ items: ReadonlyArray<CompositionFlowLayoutItem> }>,
  bindings: ReadonlyMap<string, CompositionResolvedComponentBinding>,
  sources: ReadonlyMap<string, CompositionResolvePlan["sources"][number]>,
): Map<string, string> => {
  const owners = new Map<string, string>();
  for (const row of rows) {
    for (const item of row.items) {
      if (!item.component) continue;
      const binding = bindings.get(item.component);
      if (!binding || binding.kind === "text" || binding.kind === "input") {
        continue;
      }
      if (usesServerMetricQuery(binding)) continue;
      if (sources.has(binding.source_id) && !owners.has(binding.source_id)) {
        owners.set(binding.source_id, binding.component_id);
      }
    }
  }
  return owners;
};

export interface DashboardFlowItemProps {
  item: CompositionFlowLayoutItem;
  /** Resolved binding for component items; absent while preview is pending. */
  binding?: CompositionResolvedComponentBinding;
  texts: Readonly<DashboardFlowTexts>;
  /** Semantic definition for Input components; absent when undeclared. */
  definition?: CompositionVariableDefinition;
  parameterDisplayName?: string;
  parameterValues: Readonly<Record<string, unknown>>;
  parameterInvalid?: (parameterId: string) => boolean;
  onParameterChange?: (
    parameterId: string,
    value: unknown | undefined,
  ) => void;
  sources: Record<string, CompositionSourcePageState>;
  sourceById: ReadonlyMap<
    string,
    CompositionResolvePlan["sources"][number]
  >;
  ownsSourceStatus: boolean;
  fieldNames?: CompositionFieldNames;
  fieldKeys?: CompositionFieldKeys;
  fieldTypes?: CompositionFieldTypeLookup;
  metricQueryContext?: CompositionMetricQueryContext;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
}

/**
 * One flow layout item shared by the saved-Tool renderer and the Design
 * canvas. Input controls bind transient Work state, text joins its
 * declaration by component id with no source fetch, and metrics and
 * tables delegate to the source-native presenters.
 */
export function DashboardFlowItem(props: DashboardFlowItemProps) {
  if (!props.item.component) return null;
  if (props.binding?.kind === "input" && props.definition) {
    return (
      <div class="compositionFlowItem compositionFlowItem--parameter">
        <ParameterControl
          definition={props.definition}
          displayName={props.parameterDisplayName ??
            t("composition.studioParameters")}
          value={props.parameterValues[props.definition.id]}
          onChange={(value) =>
            props.onParameterChange?.(props.definition.id, value)}
          invalid={props.parameterInvalid?.(props.definition.id)}
        />
      </div>
    );
  }
  return (
    <Show when={props.binding}>
      {(entry) => (
        <DashboardFlowComponent
          binding={entry()}
          texts={props.texts}
          sources={props.sources}
          sourceById={props.sourceById}
          ownsSourceStatus={props.ownsSourceStatus}
          fieldNames={props.fieldNames}
          fieldKeys={props.fieldKeys}
          fieldTypes={props.fieldTypes}
          metricQueryContext={props.metricQueryContext}
          onNext={props.onNext}
          onPrevious={props.onPrevious}
          onRetry={props.onRetry}
        />
      )}
    </Show>
  );
}

type DashboardFlowRendererProps = {
  /**
   * First-class flow layout: rows render top to bottom, items within a row
   * render in order. Each Input is a normal component binding in the
   * canonical document and resolves its Variable definition here.
   */
  layout: Pick<CompositionFlowLayout, "rows">;
  /** Input component id -> variable id from the canonical document. */
  inputVariables?: Readonly<Record<string, string>>;
  /** Optional Input label overrides keyed by component id. */
  inputLabels?: Readonly<Record<string, string>>;
  /** Resolved sources with their layout-ordered component bindings. */
  plan: Pick<CompositionResolvePlan, "sources" | "component_bindings">;
  /** Text content and style joined by component id. */
  texts: Readonly<DashboardFlowTexts>;
  /** Semantic variable definitions used by Input component bindings. */
  parameterDefinitions: readonly CompositionVariableDefinition[];
  /** Declaration-order definitions used only for localized display names. */
  parameterDisplayDefinitions?: readonly CompositionVariableDefinition[];
  /** Transient parameter values owned by the composition query handle. */
  parameterValues: Readonly<Record<string, unknown>>;
  parameterInvalid?: (parameterId: string) => boolean;
  onParameterChange?: (
    parameterId: string,
    value: unknown | undefined,
  ) => void;
  sources: Record<string, CompositionSourcePageState>;
  fieldNames?: CompositionFieldNames;
  fieldKeys?: CompositionFieldKeys;
  fieldTypes?: CompositionFieldTypeLookup;
  metricQueryContext?: CompositionMetricQueryContext;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
};

/**
 * Read-only dashboard flow renderer for saved Tools. Layout rows and item
 * order come from the saved document; metric and table blocks reuse the
 * existing source-native presenters with exact-scalar semantics, and
 * parameter controls stay bound to transient Work state.
 */
export function DashboardFlowRenderer(props: DashboardFlowRendererProps) {
  const bindingById = () => {
    const bindings = new Map(
      props.plan.component_bindings.map((binding) => [
        binding.component_id,
        binding,
      ]),
    );
    for (
      const [componentId, variableId] of Object.entries(
        props.inputVariables ?? {},
      )
    ) {
      if (!bindings.has(componentId)) {
        bindings.set(componentId, {
          component_id: componentId,
          kind: "input" as const,
          variable_id: variableId,
        });
      }
    }
    return bindings;
  };
  const sourceById = () =>
    new Map(props.plan.sources.map((source) => [source.source_id, source]));
  const definitionById = () =>
    new Map(
      props.parameterDefinitions.map((
        definition,
      ) => [definition.id, definition]),
    );
  // One block owns each source status: the first binding in row and item
  // order, so shared sources show a single loading or error state.
  const sourceStatusOwner = () =>
    flowSourceStatusOwner(
      props.layout.rows,
      bindingById(),
      sourceById(),
    );

  return (
    <div class="compositionFlow">
      <Show
        when={props.layout.rows.length > 0}
        fallback={<p class="ui-muted">{t("composition.queryEmpty")}</p>}
      >
        <For each={props.layout.rows}>
          {(row) => (
            <div class="compositionFlowRow">
              <For each={row.items}>
                {(item) => {
                  const binding = () => bindingById().get(item.component);
                  const definition = () =>
                    binding()?.kind === "input"
                      ? definitionById().get(binding()!.variable_id)
                      : undefined;
                  const ownsSourceStatus = () => {
                    const current = binding();
                    return current !== undefined && current.kind !== "text" &&
                      current.kind !== "input" &&
                      sourceStatusOwner().get(
                          (current as { source_id: string }).source_id,
                        ) === current.component_id;
                  };
                  return (
                    <DashboardFlowItem
                      item={item}
                      binding={binding()}
                      texts={props.texts}
                      definition={definition()}
                      parameterDisplayName={binding()?.kind === "input"
                        ? binding()!.label?.trim() ||
                          props.inputLabels?.[item.component] ||
                          compositionParameterDisplayName(
                            props.parameterDisplayDefinitions ??
                              props.parameterDefinitions,
                            binding()!.variable_id,
                          )
                        : undefined}
                      parameterValues={props.parameterValues}
                      parameterInvalid={props.parameterInvalid}
                      onParameterChange={props.onParameterChange}
                      sources={props.sources}
                      sourceById={sourceById()}
                      ownsSourceStatus={ownsSourceStatus()}
                      fieldNames={props.fieldNames}
                      fieldKeys={props.fieldKeys}
                      fieldTypes={props.fieldTypes}
                      metricQueryContext={props.metricQueryContext}
                      onNext={props.onNext}
                      onPrevious={props.onPrevious}
                      onRetry={props.onRetry}
                    />
                  );
                }}
              </For>
            </div>
          )}
        </For>
      </Show>
    </div>
  );
}

function DashboardFlowComponent(props: {
  binding: CompositionResolvedComponentBinding;
  texts: Readonly<DashboardFlowTexts>;
  sources: Record<string, CompositionSourcePageState>;
  sourceById: ReadonlyMap<
    string,
    CompositionResolvePlan["sources"][number]
  >;
  ownsSourceStatus: boolean;
  fieldNames?: CompositionFieldNames;
  fieldKeys?: CompositionFieldKeys;
  fieldTypes?: CompositionFieldTypeLookup;
  metricQueryContext?: CompositionMetricQueryContext;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
}) {
  const binding = () => props.binding;
  // Text carries no source binding: join content and style by component
  // id, and render nothing when the declaration is missing.
  if (binding().kind === "text") {
    const text = () => props.texts[binding().component_id];
    return (
      <Show when={text()}>
        {(entry) => (
          <div class="compositionFlowItem compositionFlowItem--text">
            <TextComponent text={entry().text} style={entry().style} />
          </div>
        )}
      </Show>
    );
  }
  const sourced = () =>
    binding() as Extract<
      CompositionResolvedComponentBinding,
      { kind: "metric" | "table" }
    >;
  const source = () => props.sourceById.get(sourced().source_id);
  const sourceState = () => props.sources[sourced().source_id];
  return (
    <Show when={source()}>
      {(entry) => (
        <>
          {sourced().kind === "metric"
            ? (
              <div class="compositionFlowItem compositionFlowItem--metric">
                <CompositionMetric
                  binding={sourced() as Extract<
                    CompositionResolvedComponentBinding,
                    { kind: "metric" }
                  >}
                  metricQueryContext={props.metricQueryContext}
                />
              </div>
            )
            : entry().kind === "entry_query"
            ? (
              <div class="compositionFlowItem compositionFlowItem--table">
                <CompositionEntryQueryTable
                  binding={sourced() as Extract<
                    CompositionResolvedComponentBinding,
                    { kind: "table" }
                  >}
                  source={entry() as Extract<
                    CompositionResolvePlan["sources"][number],
                    { kind: "entry_query" }
                  >}
                  sourceState={sourceState()}
                  ownsSourceStatus={props.ownsSourceStatus}
                  fieldNames={props.fieldNames}
                  fieldKeys={props.fieldKeys}
                  fieldTypes={props.fieldTypes}
                  onNext={() => props.onNext(sourced().source_id)}
                  onPrevious={() => props.onPrevious(sourced().source_id)}
                  onRetry={() => props.onRetry(sourced().source_id)}
                />
              </div>
            )
            : (
              <div class="compositionFlowItem compositionFlowItem--table">
                <CompositionSavedSqlTable
                  binding={sourced() as Extract<
                    CompositionResolvedComponentBinding,
                    { kind: "table" }
                  >}
                  source={entry() as Extract<
                    CompositionResolvePlan["sources"][number],
                    { kind: "saved_sql" }
                  >}
                  sourceState={sourceState()}
                  ownsSourceStatus={props.ownsSourceStatus}
                  onNext={() => props.onNext(sourced().source_id)}
                  onPrevious={() => props.onPrevious(sourced().source_id)}
                  onRetry={() => props.onRetry(sourced().source_id)}
                />
              </div>
            )}
        </>
      )}
    </Show>
  );
}
