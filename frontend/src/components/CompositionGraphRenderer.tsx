import { For, type JSX, Show } from "solid-js";
import { ResultPagination } from "~/components/ResultPagination";
import { SqlResultTable } from "~/components/SqlResultTable";
import { UiIcon } from "~/components/UiIcon";
import type { CompositionEvaluationResponse } from "~/lib/composition-api";
import type {
  CompositionComponent,
  CompositionGraphDraft,
  CompositionLiteral,
  CompositionScalarType,
} from "~/lib/composition-variable-binding";
import { safeText } from "~/lib/display-value";
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
  // The REST response uses JSON values, while exact-integer decoding can
  // preserve a typed integer literal. Keep JSON objects opaque.
  if (type === "json") return value;
  const literal = value as { kind?: unknown; value?: unknown };
  if (literal.kind === "null") return null;
  return literal.kind === type && "value" in value ? literal.value : value;
};

const variableValue = (
  draft: CompositionGraphDraft,
  evaluation: CompositionEvaluationResponse | undefined,
  rawInputs: Readonly<Record<string, string>>,
  id: string,
  unavailableInputIds: ReadonlySet<string>,
  preferRawInput = false,
): unknown => {
  const node = draft.variables.find((candidate) => candidate.id === id);
  if (!node) return undefined;
  if (node.kind === "constant") return literalValue(node.value);
  if (node.kind === "input") {
    if (preferRawInput && Object.hasOwn(rawInputs, id)) return rawInputs[id];
    if (unavailableInputIds.has(id)) return undefined;
    const values = evaluation?.variable_values;
    return values && Object.hasOwn(values, id)
      ? evaluatedInputValue(values[id], node.value_type.scalar_type)
      : literalValue(node.default);
  }
  if (unavailableInputIds.has(id)) return undefined;
  const result = evaluation?.query_results[id]?.value;
  return result?.kind === "scalar" ? literalValue(result.value) : undefined;
};

const metricInputValue = (
  draft: CompositionGraphDraft,
  evaluation: CompositionEvaluationResponse | undefined,
  rawInputs: Readonly<Record<string, string>>,
  id: string,
  unavailableInputIds: ReadonlySet<string>,
): unknown => {
  const node = draft.variables.find((candidate) => candidate.id === id);
  const value = variableValue(
    draft,
    evaluation,
    rawInputs,
    id,
    unavailableInputIds,
    true,
  );
  if (
    node?.kind === "input" && node.value_type.scalar_type === "integer" &&
    typeof value === "number" && !Number.isSafeInteger(value)
  ) {
    return node.default !== undefined ? literalValue(node.default) : undefined;
  }
  return value;
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
  const code = query?.result_diagnostic?.result_diagnostic?.code ??
    query?.result_diagnostic?.code ?? query?.code;
  const key = code === "evaluation_aborted"
    ? "composition.evaluationAborted"
    : code === "scalar_no_rows"
    ? "composition.diagnostic.metric_scalar_no_rows"
    : code === "scalar_multiple_rows"
    ? "composition.diagnostic.metric_scalar_multiple_rows"
    : code === "scalar_column_count_mismatch"
    ? "composition.diagnostic.metric_result_not_scalar"
    : code === "column_type_mismatch" || code === "value_type_mismatch" ||
        code === "null_not_allowed"
    ? "composition.diagnostic.metric_result_type_mismatch"
    : code === "saved_sql_column_missing"
    ? "composition.diagnostic.metric_result_column_missing"
    : code === "saved_sql_column_ambiguous"
    ? "composition.diagnostic.metric_result_column_ambiguous"
    : undefined;
  return key ? t(key) : t("composition.queryFailed");
};

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
                const component = () =>
                  componentById(item.component);
                return (
                  <Show when={component()}>
                    {(entry) => {
                      const variableId = () =>
                        entry().kind === "text"
                          ? entry().content.kind === "variable"
                            ? entry().content.variable
                            : undefined
                          : entry().variable;
                      const node = () =>
                        props.draft.variables.find((value) =>
                          value.id === variableId()
                        );
                      const valueLabel = () => {
                        const valueNode = node();
                        if (
                          valueNode?.kind === "query" &&
                          valueNode.query.kind === "saved_sql" &&
                          valueNode.query.label_source === "source"
                        ) {
                          return valueNode.query.column ??
                            t("composition.studioMetricPreview");
                        }
                        return variableId() ?? "";
                      };
                      const queryResult = () =>
                        variableId() &&
                          !props.blockedQueryIds.has(variableId()!)
                          ? props.evaluation?.query_results[variableId()!]
                          : undefined;
                      const evaluated = () => queryResult()?.value;
                      const blocked = () =>
                        Boolean(
                          variableId() &&
                            (props.blockedQueryIds.has(variableId()!) ||
                              entry().kind !== "input" &&
                                node()?.kind === "input" &&
                                !!props.inputStates[variableId()!] &&
                                (props.inputStates[variableId()!] !==
                                    "pending" ||
                                  (node() as Extract<
                                      CompositionGraphDraft["variables"][
                                        number
                                      ],
                                      { kind: "input" }
                                    >).value_type.scalar_type !== "string")),
                        );
                      const error = () =>
                        variableId() && !blocked()
                          ? queryError(props.evaluation, variableId()!)
                          : undefined;
                      const rendered = (
                        <Show
                          when={!blocked() &&
                            !(node()?.kind === "query" && !queryResult() &&
                              !error())}
                          fallback={
                            <p role="status">
                              {t(
                                blocked()
                                  ? "composition.waitingForInputs"
                                  : "composition.queryLoading",
                              )}
                            </p>
                          }
                        >
                          <Show
                            when={!error()}
                            fallback={
                              <p class="ui-text-danger" role="alert">
                                {error()}
                              </p>
                            }
                          >
                            <Show
                              when={entry().kind !== "input"}
                              fallback={(() => {
                                const input = node();
                                if (
                                  !input || input.kind !== "input"
                                ) return null;
                                const fallback = input.default
                                  ? String(literalValue(input.default) ?? "")
                                  : "";
                                const inputState = () =>
                                  props.inputStates[input.id];
                                const inputError = () =>
                                  inputState() === "invalid"
                                    ? t("composition.variableInvalid")
                                    : inputState() === "missing"
                                    ? t(
                                      "composition.diagnostic.variable_missing",
                                      {
                                        variable: input.id,
                                      },
                                    )
                                    : undefined;
                                const errorId =
                                  `composition-input-error-${entry().id}`;
                                return (
                                  <label class="compositionGraphInput">
                                    <span>{input.id}</span>
                                    <input
                                      class="ui-input"
                                      type={inputType(
                                        input.value_type.scalar_type,
                                      )}
                                      aria-label={input.id}
                                      aria-required={input.required ||
                                        undefined}
                                      aria-invalid={inputError()
                                        ? true
                                        : undefined}
                                      aria-describedby={inputError()
                                        ? errorId
                                        : undefined}
                                      required={input.required}
                                      value={props.rawInputs[input.id] ??
                                        fallback}
                                      onInput={(event) =>
                                        props.onInput(
                                          input.id,
                                          event.currentTarget.value,
                                        )}
                                    />
                                    <Show when={inputError()}>
                                      {(error) => (
                                        <span
                                          id={errorId}
                                          class="ui-text-danger"
                                          role="alert"
                                        >
                                          {error()}
                                        </span>
                                      )}
                                    </Show>
                                  </label>
                                );
                              })()}
                            >
                              <Show
                                when={entry().kind === "table"}
                                fallback={(() => {
                                  if (entry().kind === "text") {
                                    const content = entry().content;
                                    const value = content.kind === "literal"
                                      ? content.text
                                      : safeText(variableValue(
                                        props.draft,
                                        props.evaluation,
                                        props.rawInputs,
                                        content.variable,
                                        new Set([
                                          ...Object.keys(props.inputStates),
                                          ...props.blockedQueryIds,
                                        ]),
                                        true,
                                      ));
                                    const tag = entry().style === "title" ||
                                        entry().style === "heading"
                                      ? "h2"
                                      : "p";
                                    return tag === "h2"
                                      ? (
                                        <h2
                                          class={`compositionGraphText compositionGraphText--${entry().style}`}
                                        >
                                          {value}
                                        </h2>
                                      )
                                      : (
                                        <p
                                          class={`compositionGraphText compositionGraphText--${entry().style}`}
                                        >
                                          {value}
                                        </p>
                                      );
                                  }
                                  const unavailableInputIds = new Set([
                                    ...Object.keys(props.inputStates),
                                    ...props.blockedQueryIds,
                                  ]);
                                  const result = evaluated();
                                  let value: unknown;
                                  if (result?.kind === "scalar") {
                                    value = literalValue(result.value);
                                  } else if (entry().kind === "metric") {
                                    value = metricInputValue(
                                      props.draft,
                                      props.evaluation,
                                      props.rawInputs,
                                      variableId() ?? "",
                                      unavailableInputIds,
                                    );
                                  } else {
                                    value = variableValue(
                                      props.draft,
                                      props.evaluation,
                                      props.rawInputs,
                                      variableId() ?? "",
                                      unavailableInputIds,
                                    );
                                  }
                                  const displayValue = value === null
                                    ? t("composition.metricNoData")
                                    : safeText(value);
                                  return (
                                    <section
                                      class="compositionGraphMetric"
                                      aria-label={valueLabel()}
                                    >
                                      <UiIcon name="canvas-metric" />
                                      <h2 class="compositionGraphMetricLabel">
                                        {valueLabel()}
                                      </h2>
                                      <strong>{displayValue}</strong>
                                    </section>
                                  );
                                })()}
                              >
                                <Show
                                  when={evaluated()?.kind === "table"}
                                  fallback={
                                    <p role="status">
                                      {t("composition.queryLoading")}
                                    </p>
                                  }
                                >
                                  {(() => {
                                    const table = () =>
                                      evaluated()?.kind === "table"
                                        ? evaluated() as Extract<
                                          NonNullable<typeof evaluated>,
                                          { kind: "table" }
                                        >
                                        : undefined;
                                    const tableComponent = () =>
                                      entry().kind === "table"
                                        ? entry()
                                        : undefined;
                                    return (
                                      <>
                                        <SqlResultTable
                                          columns={table()?.columns.map((
                                            column,
                                          ) => column.name) ?? []}
                                          columnTypes={table()?.columns.map((
                                            column,
                                          ) =>
                                            column.value_type.scalar_type
                                          )}
                                          rows={table()?.rows.map((row) =>
                                            row.map(literalValue)
                                          ) ?? []}
                                          displayColumns={tableComponent()
                                            ?.presentation?.columns}
                                          pageIdentity={props.evaluation
                                            ?.evaluation_id ??
                                            variableId() ?? "composition"}
                                          tableLabel={`${variableId()} results`}
                                          emptyMessage={t("sqlPage.noResults")}
                                          invalidViewMessage={t(
                                            "composition.tableViewUnavailable",
                                          )}
                                          viewportClassName="compositionGraphTableViewport"
                                        />
                                        <ResultPagination
                                          class="compositionGraphPagination"
                                          canPrevious={Boolean(
                                            variableId() &&
                                              props.previousPageQueryIds.has(
                                                variableId()!,
                                              ),
                                          )}
                                          canNext={Boolean(
                                            queryResult()?.has_more &&
                                              queryResult()?.next &&
                                              variableId(),
                                          )}
                                          busy={props.pageBusy}
                                          previousLabel={t("common.previous")}
                                          nextLabel={t("common.next")}
                                          ariaLabel={t(
                                            "composition.resultPages",
                                          )}
                                          onPrevious={() =>
                                            props.onPrevious(variableId()!)}
                                          onNext={() =>
                                            props.onNext(
                                              variableId()!,
                                              queryResult()!.next!,
                                            )}
                                        />
                                      </>
                                    );
                                  })()}
                                </Show>
                              </Show>
                            </Show>
                          </Show>
                        </Show>
                      );
                      return props.decorateComponent
                        ? props.decorateComponent(entry, rendered)
                        : (
                          <div
                            class="compositionGraphValue"
                            data-component-id={entry().id}
                          >
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
