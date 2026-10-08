import { For, Show } from "solid-js";
import { CompositionEntryQueryTable } from "~/components/CompositionEntryQueryTable";
import {
  type CompositionFieldKeys,
  type CompositionFieldNames,
  CompositionMetric,
  CompositionSavedSqlTable,
} from "~/components/CompositionRenderer";
import { ParameterControl } from "~/components/composition/ParameterControl";
import { TextComponent } from "~/components/composition/TextComponent";
import { t } from "~/lib/i18n";
import type {
  CompositionFlowLayout,
  CompositionFlowLayoutItem,
  CompositionParameterDefinition,
  CompositionResolvedComponentBinding,
  CompositionResolvePlan,
  CompositionTextStyle,
} from "~/lib/composition-api";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";

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
      if (item.kind !== "component" || !item.component) continue;
      const binding = bindings.get(item.component);
      if (!binding || binding.kind === "text") continue;
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
  /** Semantic definition for parameter items; absent when undeclared. */
  definition?: CompositionParameterDefinition;
  parameterValues: Readonly<Record<string, unknown>>;
  onParameterChange: (parameterId: string, value: unknown | undefined) => void;
  parameterInvalid?: (parameterId: string) => boolean;
  sources: Record<string, CompositionSourcePageState>;
  sourceById: ReadonlyMap<
    string,
    CompositionResolvePlan["sources"][number]
  >;
  ownsSourceStatus: boolean;
  fieldNames?: CompositionFieldNames;
  fieldKeys?: CompositionFieldKeys;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
}

/**
 * One flow layout item shared by the saved-Tool renderer and the Design
 * canvas. Parameter controls bind transient Work state, text joins its
 * declaration by component id with no source fetch, and metrics and
 * tables delegate to the source-native presenters.
 */
export function DashboardFlowItem(props: DashboardFlowItemProps) {
  if (props.item.kind === "parameter" && props.item.parameter) {
    return (
      <Show when={props.definition}>
        {(entry) => (
          <div class="compositionFlowItem compositionFlowItem--parameter">
            <ParameterControl
              definition={entry()}
              value={props.parameterValues[entry().id]}
              onChange={(value) => props.onParameterChange(entry().id, value)}
              invalid={props.parameterInvalid?.(entry().id)}
            />
          </div>
        )}
      </Show>
    );
  }
  if (props.item.kind !== "component" || !props.item.component) return null;
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
   * render in order. The resolve plan carries bindings in this same order;
   * parameter controls emit no bindings and render from the definitions.
   */
  layout: Pick<CompositionFlowLayout, "rows">;
  /** Resolved sources with their layout-ordered component bindings. */
  plan: Pick<CompositionResolvePlan, "sources" | "component_bindings">;
  /** Text content and style joined by component id. */
  texts: Readonly<DashboardFlowTexts>;
  /** Semantic parameter definitions; layout-placed controls come first. */
  parameterDefinitions: readonly CompositionParameterDefinition[];
  /** Transient parameter values owned by the composition query handle. */
  parameterValues: Readonly<Record<string, unknown>>;
  onParameterChange: (parameterId: string, value: unknown | undefined) => void;
  parameterInvalid?: (parameterId: string) => boolean;
  sources: Record<string, CompositionSourcePageState>;
  fieldNames?: CompositionFieldNames;
  fieldKeys?: CompositionFieldKeys;
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
  const bindingById = () =>
    new Map(
      props.plan.component_bindings.map((binding) => [
        binding.component_id,
        binding,
      ]),
    );
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
    flowSourceStatusOwner(props.layout.rows, bindingById(), sourceById());

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
                  const binding = () =>
                    item.kind === "component" && item.component
                      ? bindingById().get(item.component)
                      : undefined;
                  const definition = () =>
                    item.kind === "parameter" && item.parameter
                      ? definitionById().get(item.parameter)
                      : undefined;
                  const ownsSourceStatus = () => {
                    const current = binding();
                    return current !== undefined && current.kind !== "text" &&
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
                      parameterValues={props.parameterValues}
                      onParameterChange={props.onParameterChange}
                      parameterInvalid={props.parameterInvalid}
                      sources={props.sources}
                      sourceById={sourceById()}
                      ownsSourceStatus={ownsSourceStatus()}
                      fieldNames={props.fieldNames}
                      fieldKeys={props.fieldKeys}
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
                  source={entry()}
                  sourceState={sourceState()}
                  ownsSourceStatus={props.ownsSourceStatus}
                  onRetry={() => props.onRetry(sourced().source_id)}
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
