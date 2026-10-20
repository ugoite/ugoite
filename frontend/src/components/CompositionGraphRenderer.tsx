import { For, Show } from "solid-js";
import { ResultPagination } from "~/components/ResultPagination";
import { UiIcon } from "~/components/UiIcon";
import type { CompositionEvaluationResponse } from "~/lib/composition-api";
import type {
  CompositionGraphDraft,
  CompositionLiteral,
  CompositionScalarType,
} from "~/lib/composition-variable-binding";
import { safeText } from "~/lib/display-value";
import { t } from "~/lib/i18n";

export interface CompositionGraphRendererProps {
  draft: CompositionGraphDraft;
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
    return literalValue(evaluation?.variable_values[id] ?? node.default);
  }
  if (unavailableInputIds.has(id)) return undefined;
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
  const code = query?.result_diagnostic?.result_diagnostic?.code ??
    query?.result_diagnostic?.code ?? query?.code;
  const key = code === "scalar_no_rows"
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

/** Render only the variable graph's validated values; no source or SQL reads occur here. */
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
                      const queryResult = () =>
                        variableId() &&
                          !props.blockedQueryIds.has(variableId()!)
                          ? props.evaluation?.query_results[variableId()!]
                          : undefined;
                      const evaluated = () => queryResult()?.value;
                      const blocked = () =>
                        Boolean(
                          variableId() &&
                            props.blockedQueryIds.has(variableId()!),
                        );
                      const error = () =>
                        variableId() && !blocked()
                          ? queryError(props.evaluation, variableId()!)
                          : undefined;
                      return (
                        <Show
                          when={!blocked()}
                          fallback={
                            <p role="status">
                              {t("composition.waitingForInputs")}
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
                                      ? <h2>{value}</h2>
                                      : <p>{value}</p>;
                                  }
                                  const value = evaluated()?.kind === "scalar"
                                    ? literalValue(evaluated()!.value)
                                    : variableValue(
                                      props.draft,
                                      props.evaluation,
                                      props.rawInputs,
                                      variableId() ?? "",
                                      new Set([
                                        ...Object.keys(props.inputStates),
                                        ...props.blockedQueryIds,
                                      ]),
                                    );
                                  const displayValue = value === null
                                    ? t("composition.metricNoData")
                                    : safeText(value);
                                  return (
                                    <section
                                      class="compositionGraphMetric"
                                      aria-label={variableId()}
                                    >
                                      <UiIcon name="canvas-metric" />
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
                                    return (
                                      <div
                                        class="result-table-viewport compositionGraphTableViewport"
                                        role="region"
                                        aria-label={`${variableId()} results`}
                                        tabindex="0"
                                      >
                                        <table
                                          class="result-table"
                                          aria-label={`${variableId()} results`}
                                        >
                                          <thead>
                                            <tr>
                                              <For
                                                each={table()?.columns ?? []}
                                              >
                                                {(column) => (
                                                  <th scope="col">
                                                    {column.name}
                                                  </th>
                                                )}
                                              </For>
                                            </tr>
                                          </thead>
                                          <tbody>
                                            <For each={table()?.rows ?? []}>
                                              {(values) => (
                                                <tr>
                                                  <For each={values}>
                                                    {(value) => (
                                                      <td>
                                                        {safeText(
                                                          literalValue(value),
                                                        )}
                                                      </td>
                                                    )}
                                                  </For>
                                                </tr>
                                              )}
                                            </For>
                                          </tbody>
                                        </table>
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
                                      </div>
                                    );
                                  })()}
                                </Show>
                              </Show>
                            </Show>
                          </Show>
                        </Show>
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
