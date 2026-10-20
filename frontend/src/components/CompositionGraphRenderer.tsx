import { For, Show } from "solid-js";
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
  onInput: (variableId: string, raw: string) => void;
  onNext: (variableId: string, cursor: string) => void;
}

const literalValue = (value: CompositionLiteral | undefined): unknown =>
  !value || value.kind === "null" ? null : value.value;

const variableValue = (
  draft: CompositionGraphDraft,
  evaluation: CompositionEvaluationResponse | undefined,
  id: string,
  unavailableInputIds: ReadonlySet<string>,
): unknown => {
  const node = draft.variables.find((candidate) => candidate.id === id);
  if (!node) return undefined;
  if (node.kind === "constant") return literalValue(node.value);
  if (node.kind === "input") {
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
  return query?.result_diagnostic?.result_diagnostic?.code ?? query?.code;
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
                      const error = () =>
                        variableId() &&
                          !props.blockedQueryIds.has(variableId()!)
                          ? queryError(props.evaluation, variableId()!)
                          : undefined;
                      return (
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
                                    aria-required={input.required || undefined}
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
                                      content.variable,
                                      new Set([
                                        ...Object.keys(props.inputStates),
                                        ...props.blockedQueryIds,
                                      ]),
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
                                    variableId() ?? "",
                                    new Set([
                                      ...Object.keys(props.inputStates),
                                      ...props.blockedQueryIds,
                                    ]),
                                  );
                                return (
                                  <section
                                    class="compositionGraphMetric"
                                    aria-label={variableId()}
                                  >
                                    <UiIcon name="canvas-metric" />
                                    <strong>{safeText(value)}</strong>
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
                                            <For each={table()?.columns ?? []}>
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
                                      <Show
                                        when={queryResult()?.has_more &&
                                          queryResult()?.next && variableId()}
                                      >
                                        <button
                                          type="button"
                                          class="ui-button ui-button-secondary"
                                          onClick={() =>
                                            props.onNext(
                                              variableId()!,
                                              queryResult()!.next!,
                                            )}
                                        >
                                          {t("common.next")}
                                        </button>
                                      </Show>
                                    </div>
                                  );
                                })()}
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
