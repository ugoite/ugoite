import { For, Show } from "solid-js";
import {
  CompositionMetric,
  type CompositionMetricQueryContext,
  CompositionSqlTable,
  usesServerMetricQuery,
} from "~/components/CompositionRenderer";
import { InputControl } from "~/components/composition/InputControl";
import { TextComponent } from "~/components/composition/TextComponent";
import { t } from "~/lib/i18n";
import type {
  CompositionFlowLayout,
  CompositionFlowLayoutItem,
  CompositionResolvedComponentBinding,
  CompositionResolvePlan,
  CompositionTextStyle,
  CompositionVariableDefinition,
} from "~/lib/composition-api";
import type {
  CompositionSourcePageState,
  CompositionVariableInputState,
} from "~/lib/composition-query-handle";
import { compositionVariableDisplayName } from "~/lib/composition-variable-name";

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
  inputDisplayName?: string;
  variableValues?: Readonly<Record<string, unknown>>;
  variableInputs?: Readonly<Record<string, CompositionVariableInputState>>;
  variableInvalid?: (variableId: string) => boolean;
  onVariableInput?: (variableId: string, raw: string) => void;
  sources: Record<string, CompositionSourcePageState>;
  sourceById: ReadonlyMap<
    string,
    CompositionResolvePlan["sources"][number]
  >;
  ownsSourceStatus: boolean;
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
  return (
    <Show
      when={props.binding?.kind === "input" && props.definition}
      fallback={
        <Show when={props.binding}>
          {(entry) => (
            <DashboardFlowComponent
              binding={entry()}
              texts={props.texts}
              sources={props.sources}
              sourceById={props.sourceById}
              ownsSourceStatus={props.ownsSourceStatus}
              metricQueryContext={props.metricQueryContext}
              onNext={props.onNext}
              onPrevious={props.onPrevious}
              onRetry={props.onRetry}
            />
          )}
        </Show>
      }
    >
      {(definition) => (
        <div class="compositionFlowItem compositionFlowItem--input">
          <InputControl
            definition={definition()}
            displayName={props.inputDisplayName ??
              t("composition.studioVariables")}
            value={props.variableValues?.[definition().id]}
            rawValue={props.variableInputs?.[definition().id]?.raw}
            onChange={(raw) =>
              props.onVariableInput?.(definition().id, raw)}
            invalid={props.variableInvalid?.(definition().id)}
          />
        </div>
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
  /** Resolved sources with their layout-ordered component bindings. */
  plan: Pick<
    CompositionResolvePlan,
    "sources" | "component_bindings" | "dependencies"
  >;
  /** Text content and style joined by component id. */
  texts: Readonly<DashboardFlowTexts>;
  /** Semantic variable definitions used by Input component bindings. */
  variableDefinitions: readonly CompositionVariableDefinition[];
  /** Declaration-order definitions used only for localized display names. */
  variableDisplayDefinitions?: readonly CompositionVariableDefinition[];
  /** Transient variable values owned by the composition query handle. */
  variableValues?: Readonly<Record<string, unknown>>;
  variableInputs?: Readonly<Record<string, CompositionVariableInputState>>;
  variableInvalid?: (variableId: string) => boolean;
  onVariableInput?: (variableId: string, raw: string) => void;
  sources: Record<string, CompositionSourcePageState>;
  metricQueryContext?: CompositionMetricQueryContext;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
};

/**
 * Read-only dashboard flow renderer for saved Tools. Layout rows and item
 * order come from the saved document; metric and table blocks reuse the
 * existing source-native presenters with exact-scalar semantics, and
 * Input components stay bound to transient Work state.
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
      props.variableDefinitions.map((
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
  const metricQueryContext = () =>
    props.metricQueryContext
      ? {
        ...props.metricQueryContext,
        dependencies: props.metricQueryContext.dependencies ??
          props.plan.dependencies,
      }
      : undefined;

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
                      inputDisplayName={binding()?.kind === "input"
                        ? compositionVariableDisplayName(
                          props.variableDisplayDefinitions ??
                            props.variableDefinitions,
                          binding()!.variable_id,
                        )
                        : undefined}
                      variableValues={props.variableValues}
                      variableInputs={props.variableInputs}
                      variableInvalid={props.variableInvalid}
                      onVariableInput={props.onVariableInput}
                      sources={props.sources}
                      sourceById={sourceById()}
                      ownsSourceStatus={ownsSourceStatus()}
                      metricQueryContext={metricQueryContext()}
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
            : (
              <div class="compositionFlowItem compositionFlowItem--table">
                <CompositionSqlTable
                  binding={sourced() as Extract<
                    CompositionResolvedComponentBinding,
                    { kind: "table" }
                  >}
                  source={entry()}
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
