import { createResource, For, type JSX, Show } from "solid-js";
import { ResultPagination } from "~/components/ResultPagination";
import { SqlResultTable } from "~/components/SqlResultTable";
import type { CompositionEvaluationResponse } from "~/lib/composition-api";
import type {
  CompositionComponent,
  CompositionGraphDraft,
  CompositionLiteral,
  CompositionScalarType,
} from "~/lib/composition-variable-binding";
import { safeText } from "~/lib/display-value";
import { parseCompositionTextTemplate } from "~/lib/ugoite-client";
import type { CompositionTextTemplateSegment } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";

export interface CompositionGraphRendererProps {
  draft: CompositionGraphDraft;
  /** Editor-only frame around the same semantic value renderer. */
  decorateComponent?: (
    component: () => CompositionComponent,
    value: JSX.Element,
  ) => JSX.Element;
  evaluation?: CompositionEvaluationResponse;
  rawInputs: Readonly<Record<string, string>>;
  inputStates: Readonly<Record<string, "pending" | "invalid" | "missing">>;
  blockedQueryIds: ReadonlySet<string>;
  previousPageQueryIds: ReadonlySet<string>;
  pageBusy: boolean;
  onInput: (variableId: string, raw: string) => void;
  onNext: (variableId: string, cursor: string) => void;
  onPrevious: (variableId: string) => void;
}

const literalValue = (value: CompositionLiteral | undefined): unknown =>
  !value || value.kind === "null" ? null : value.value;

const evaluatedInputValue = (
  value: unknown,
  type: CompositionScalarType,
): unknown => {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return value;
  }
  // Exact integers arrive as tagged values; JSON remains non-displayable.
  if (type === "json") return value;
  const literal = value as { kind?: unknown; value?: unknown };
  if (literal.kind === "null") return null;
  return literal.kind === type && "value" in value ? literal.value : value;
};

const isDisplayable = (type: CompositionScalarType): boolean => type !== "json";

const variableNode = (draft: CompositionGraphDraft, id: string) =>
  draft.variables.find((candidate) => candidate.id === id);

const variableValue = (
  draft: CompositionGraphDraft,
  evaluation: CompositionEvaluationResponse | undefined,
  rawInputs: Readonly<Record<string, string>>,
  id: string,
  unavailableInputIds: ReadonlySet<string>,
): unknown => {
  const node = variableNode(draft, id);
  if (!node) return undefined;
  if (node.kind === "constant") {
    return isDisplayable(node.value_type.scalar_type)
      ? literalValue(node.value)
      : undefined;
  }
  if (node.kind === "input") {
    if (!isDisplayable(node.value_type.scalar_type) ||
      unavailableInputIds.has(id)) return undefined;
    if (Object.hasOwn(rawInputs, id)) return rawInputs[id];
    const values = evaluation?.variable_values;
    return values && Object.hasOwn(values, id)
      ? evaluatedInputValue(values[id], node.value_type.scalar_type)
      : literalValue(node.default);
  }
  if (
    node.returns.kind !== "scalar" ||
    !isDisplayable(node.returns.value_type.scalar_type) ||
    unavailableInputIds.has(id)
  ) return undefined;
  const result = evaluation?.query_results[id]?.value;
  return result?.kind === "scalar" ? literalValue(result.value) : undefined;
};

const inputType = (type: CompositionScalarType): string =>
  type === "date" ? "date" : type === "timestamp" ? "datetime-local" : "text";

const queryError = (
  evaluation: CompositionEvaluationResponse | undefined,
  variableId: string,
): string | undefined => {
  const query = evaluation?.query_diagnostics?.find((item) =>
    item.variable_id === variableId
  );
  if (!query) return undefined;
  const code = query.result_diagnostic?.result_diagnostic?.code ??
    query.result_diagnostic?.code ?? query.code;
  const key = code === "evaluation_aborted"
    ? "composition.evaluationAborted"
    : code === "scalar_no_rows"
    ? "composition.diagnostic.scalar_no_rows"
    : code === "scalar_multiple_rows"
    ? "composition.diagnostic.scalar_multiple_rows"
    : code === "scalar_column_count_mismatch"
    ? "composition.diagnostic.scalar_result_not_scalar"
    : code === "column_type_mismatch" || code === "value_type_mismatch" ||
        code === "null_not_allowed"
    ? "composition.diagnostic.scalar_result_type_mismatch"
    : undefined;
  return key ? t(key) : t("composition.queryFailed");
};

function CompositionTextValue(props: {
  componentId: string;
  content: string;
  draft: CompositionGraphDraft;
  evaluation?: CompositionEvaluationResponse;
  rawInputs: Readonly<Record<string, string>>;
  inputStates: Readonly<Record<string, "pending" | "invalid" | "missing">>;
  blockedQueryIds: ReadonlySet<string>;
}) {
  const [parsedSegments] = createResource(
    () => props.content,
    async (content): Promise<CompositionTextTemplateSegment[] | undefined> => {
      try {
        return (await parseCompositionTextTemplate(content)).segments;
      } catch {
        return undefined;
      }
    },
  );
  const unavailable = () => new Set([
    ...Object.keys(props.inputStates),
    ...props.blockedQueryIds,
  ]);
  const serverSegments = () =>
    props.evaluation?.component_bindings.find((binding) =>
      binding.component_id === props.componentId && binding.kind === "text"
    )?.text_segments;
  const segments = () => {
    const local = parsedSegments();
    const server = serverSegments();
    if (!local) return undefined;
    // A response from a previous draft cannot replace the Rust parser's
    // interpretation of the currently edited template.
    return server && JSON.stringify(server) === JSON.stringify(local)
      ? server
      : local;
  };
  const referencedQueries = () =>
    (segments() ?? []).flatMap((part) => {
      if (part.kind !== "variable") return [];
      const node = variableNode(props.draft, part.variable);
      return node?.kind === "query" ? [node.id] : [];
    });
  const error = () => referencedQueries().map((id) =>
    queryError(props.evaluation, id)
  ).find((message) => message);
  const loading = () => !segments();
  const waitingForInputs = () => (segments() ?? []).some((part) => {
    if (part.kind !== "variable") return false;
    if (props.inputStates[part.variable]) return true;
    const node = variableNode(props.draft, part.variable);
    return node?.kind === "query" && props.blockedQueryIds.has(node.id);
  });
  const waiting = () => (segments() ?? []).some((part) => {
    if (part.kind !== "variable") return false;
    if (unavailable().has(part.variable)) return true;
    const node = variableNode(props.draft, part.variable);
    return node?.kind === "query" &&
      (props.blockedQueryIds.has(node.id) ||
        (!props.evaluation?.query_results[node.id] &&
          !queryError(props.evaluation, node.id)));
  });
  const value = () => (segments() ?? []).map((part) =>
    part.kind === "literal"
      ? part.text
      : safeText(variableValue(
        props.draft,
        props.evaluation,
        props.rawInputs,
        part.variable,
        unavailable(),
      ))
  ).join("");

  return (
    <Show
      when={!error() && !loading() && !waiting()}
      fallback={error()
        ? <span class="ui-text-danger" role="alert">{error()}</span>
        : <span role="status">
          {waitingForInputs()
            ? t("composition.waitingForInputs")
            : t("composition.queryLoading")}
        </span>}
    >
      {value()}
    </Show>
  );
}

/** Render evaluated values and raw direct bindings without issuing source reads. */
export function CompositionGraphRenderer(props: CompositionGraphRendererProps) {
  const componentById = (id: string) =>
    props.draft.components.find((component) => component.id === id);

  return (
    <main class="compositionGraphRender" aria-label={props.draft.name}>
      <For each={props.draft.layout.rows}>
        {(row) => (
          <section class="compositionGraphCanvasRow" aria-label={row.id}>
            <For each={row.items}>
              {(item) => {
                const component = () => componentById(item.component);
                return (
                  <Show when={component()}>
                    {(entry) => {
                      const variableId = () =>
                        entry().kind === "text" ? undefined : entry().variable;
                      const node = () => {
                        const id = variableId();
                        return id ? variableNode(props.draft, id) : undefined;
                      };
                      const queryResult = () => {
                        const id = variableId();
                        return id && !props.blockedQueryIds.has(id)
                          ? props.evaluation?.query_results[id]
                          : undefined;
                      };
                      const error = () => {
                        const id = variableId();
                        return id && !props.blockedQueryIds.has(id)
                          ? queryError(props.evaluation, id)
                          : undefined;
                      };
                      const blocked = () => {
                        const id = variableId();
                        return Boolean(
                          id && (props.blockedQueryIds.has(id) ||
                            props.inputStates[id]),
                        );
                      };

                      let rendered: JSX.Element;
                      if (entry().kind === "text") {
                        const tag = entry().style === "title" ||
                            entry().style === "heading"
                          ? "h2"
                          : "p";
                        const text = (
                          <CompositionTextValue
                            componentId={entry().id}
                            content={entry().content}
                            draft={props.draft}
                            evaluation={props.evaluation}
                            rawInputs={props.rawInputs}
                            inputStates={props.inputStates}
                            blockedQueryIds={props.blockedQueryIds}
                          />
                        );
                        rendered = tag === "h2"
                          ? <h2 class={`compositionGraphText compositionGraphText--${entry().style}`}>{text}</h2>
                          : <p class={`compositionGraphText compositionGraphText--${entry().style}`}>{text}</p>;
                      } else if (entry().kind === "input") {
                        const input = node();
                        if (!input || input.kind !== "input") return null;
                        const state = () => props.inputStates[input.id];
                        const inputError = () =>
                          state() === "invalid"
                            ? t("composition.variableInvalid")
                            : state() === "missing"
                            ? t("composition.diagnostic.variable_missing", {
                              variable: input.id,
                            })
                            : undefined;
                        const errorId = `composition-input-error-${entry().id}`;
                        rendered = (
                          <label class="compositionGraphInput">
                            <span>{input.id}</span>
                            <input
                              class="ui-input"
                              type={inputType(input.value_type.scalar_type)}
                              aria-label={input.id}
                              aria-required={input.required || undefined}
                              aria-invalid={inputError() ? true : undefined}
                              aria-describedby={inputError() ? errorId : undefined}
                              required={input.required}
                              value={props.rawInputs[input.id] ??
                                String(literalValue(input.default) ?? "")}
                              onInput={(event) =>
                                props.onInput(input.id, event.currentTarget.value)}
                            />
                            <Show when={inputError()}>
                              {(message) => (
                                <span id={errorId} class="ui-text-danger" role="alert">
                                  {message()}
                                </span>
                              )}
                            </Show>
                          </label>
                        );
                      } else {
                        const id = variableId() ?? "";
                        const result = () => queryResult()?.value;
                        rendered = (
                          <>
                            <Show when={blocked() || (!result() && !error())}>
                              <p role="status">
                                {blocked()
                                  ? t("composition.waitingForInputs")
                                  : t("composition.queryLoading")}
                              </p>
                            </Show>
                            <Show when={error()}>
                              {(message) => <p class="ui-text-danger" role="alert">{message()}</p>}
                            </Show>
                            <Show when={result()?.kind === "table" && !blocked() && !error()}>
                              {(() => {
                                const table = () =>
                                  result()?.kind === "table"
                                    ? result() as Extract<NonNullable<typeof result>, { kind: "table" }>
                                    : undefined;
                                return (
                                  <>
                                    <SqlResultTable
                                      columns={table()?.columns.map((column) => column.name) ?? []}
                                      columnTypes={table()?.columns.map((column) => column.value_type.scalar_type)}
                                      rows={table()?.rows.map((row) => row.map(literalValue)) ?? []}
                                      pageIdentity={props.evaluation?.evaluation_id ?? id}
                                      tableLabel={id}
                                      emptyMessage={t("sqlPage.noResults")}
                                      viewportClassName="compositionGraphTableViewport"
                                    />
                                    <ResultPagination
                                      class="compositionGraphPagination"
                                      canPrevious={props.previousPageQueryIds.has(id)}
                                      canNext={Boolean(queryResult()?.has_more && queryResult()?.next)}
                                      busy={props.pageBusy}
                                      previousLabel={t("common.previous")}
                                      nextLabel={t("common.next")}
                                      ariaLabel={t("composition.resultPages")}
                                      onPrevious={() => props.onPrevious(id)}
                                      onNext={() => props.onNext(id, queryResult()!.next!)}
                                    />
                                  </>
                                );
                              })()}
                            </Show>
                          </>
                        );
                      }

                      return props.decorateComponent
                        ? props.decorateComponent(entry, rendered)
                        : (
                          <div class="compositionGraphValue" data-component-id={entry().id}>
                            {rendered}
                          </div>
                        );
                    }}
                  </Show>
                );
              }}
            </For>
          </section>
        )}
      </For>
    </main>
  );
}
