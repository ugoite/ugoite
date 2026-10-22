import type { JSX } from "solid-js";
import { createSignal, For, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { t } from "~/lib/i18n";
import type {
  SqlStructureAggregate,
  SqlStructureColumnType,
  SqlStructureHavingExpression,
  SqlStructureHavingOperator,
  SqlStructureHavingValue,
} from "~/lib/ugoite-client";

export type SqlHavingValueKind =
  | "integer"
  | "numeric"
  | "string"
  | "date"
  | "timestamp"
  | "boolean";
export type SqlHavingCandidate = {
  aggregate: SqlStructureAggregate;
  label: string;
  valueKind: SqlHavingValueKind;
};
type HavingDraft =
  | { kind: "and" | "or"; id: string; expressions: HavingDraft[] }
  | { kind: "not"; id: string; expression: HavingDraft }
  | {
    kind: "predicate";
    id: string;
    aggregate: SqlStructureAggregate;
    operator: SqlStructureHavingOperator;
    value: SqlStructureHavingValue;
  };

interface SqlHavingExpressionEditorProps {
  expression: SqlStructureHavingExpression | null;
  onChange: (expression: SqlStructureHavingExpression | null) => void;
  candidates: readonly SqlHavingCandidate[];
  variables: readonly { name: string; type: SqlStructureColumnType }[];
  disabled?: boolean;
}

const operators: { value: SqlStructureHavingOperator; label: string }[] = [
  { value: "eq", label: "=" },
  { value: "not_eq", label: "≠" },
  { value: "lt", label: "<" },
  { value: "lt_eq", label: "≤" },
  { value: "gt", label: ">" },
  { value: "gt_eq", label: "≥" },
];

const operatorsFor = (valueKind: SqlHavingValueKind | undefined) =>
  valueKind === "boolean"
    ? operators.filter((operator) =>
      operator.value === "eq" || operator.value === "not_eq"
    )
    : operators;

const id = () => `having-${crypto.randomUUID()}`;

const toDraft = (expression: SqlStructureHavingExpression): HavingDraft => {
  switch (expression.kind) {
    case "and":
    case "or":
      return {
        ...expression,
        id: id(),
        expressions: expression.expressions.map(toDraft),
      };
    case "not":
      return {
        kind: "not",
        id: id(),
        expression: toDraft(expression.expression),
      };
    case "predicate":
      return { ...expression, id: id() };
  }
};

const toExpression = (draft: HavingDraft): SqlStructureHavingExpression => {
  switch (draft.kind) {
    case "and":
    case "or":
      return {
        kind: draft.kind,
        expressions: draft.expressions.map(toExpression),
      };
    case "not":
      return { kind: "not", expression: toExpression(draft.expression) };
    case "predicate":
      return {
        kind: "predicate",
        aggregate: draft.aggregate,
        operator: draft.operator,
        value: draft.value,
      };
  }
};

const defaultValue = (kind: SqlHavingValueKind): SqlStructureHavingValue => {
  switch (kind) {
    case "integer":
      return { kind, value: "0" };
    case "numeric":
      return { kind, value: "0" };
    case "string":
      return { kind, value: "" };
    case "date":
      return { kind, value: "1970-01-01" };
    case "timestamp":
      return { kind, value: "1970-01-01T00:00:00" };
    case "boolean":
      return { kind, value: false };
  }
};

const variableKind = (
  type: SqlStructureColumnType,
): SqlHavingValueKind | undefined => {
  switch (type) {
    case "integer":
      return "integer";
    case "float":
    case "decimal":
      return "numeric";
    case "string":
      return "string";
    case "boolean":
      return "boolean";
    case "date":
      return "date";
    case "timestamp":
      return "timestamp";
    default:
      return undefined;
  }
};

const sameAggregate = (
  left: SqlStructureAggregate,
  right: SqlStructureAggregate,
) =>
  left.function === right.function && left.column === right.column &&
  left.distinct === right.distinct &&
  (left.arguments ?? []).length === (right.arguments ?? []).length &&
  (left.arguments ?? []).every((argument, index) =>
    argument === right.arguments?.[index]
  );

export function SqlHavingExpressionEditor(
  props: SqlHavingExpressionEditorProps,
) {
  const [draft, setDraft] = createSignal<HavingDraft | undefined>(
    props.expression ? toDraft(props.expression) : undefined,
  );

  const publish = (next: HavingDraft | undefined) => {
    setDraft(next);
    props.onChange(next ? toExpression(next) : null);
  };

  const defaultPredicate = (): HavingDraft => {
    const candidate = props.candidates[0];
    return {
      kind: "predicate",
      id: id(),
      aggregate: candidate?.aggregate ?? { function: "count", distinct: false },
      operator: "gt",
      value: defaultValue(candidate?.valueKind ?? "integer"),
    };
  };

  const update = (
    current: HavingDraft,
    targetId: string,
    change: (node: HavingDraft) => HavingDraft,
  ): HavingDraft => {
    if (current.id === targetId) return change(current);
    if (current.kind === "not") {
      return {
        ...current,
        expression: update(current.expression, targetId, change),
      };
    }
    if (current.kind === "and" || current.kind === "or") {
      return {
        ...current,
        expressions: current.expressions.map((child) =>
          update(child, targetId, change)
        ),
      };
    }
    return current;
  };

  const remove = (
    current: HavingDraft,
    targetId: string,
  ): HavingDraft | undefined => {
    if (current.id === targetId) return undefined;
    if (current.kind === "not") {
      const expression = remove(current.expression, targetId);
      return expression ? { ...current, expression } : undefined;
    }
    if (current.kind === "and" || current.kind === "or") {
      const expressions = current.expressions
        .map((child) => remove(child, targetId))
        .filter((child): child is HavingDraft => child !== undefined);
      if (expressions.length === 0) return undefined;
      if (expressions.length === 1) return expressions[0];
      return { ...current, expressions };
    }
    return current;
  };

  const addToGroup = (targetId: string, child: HavingDraft) => {
    const current = draft();
    if (!current) return;
    publish(update(current, targetId, (node) => {
      if (node.kind === "and" || node.kind === "or") {
        return { ...node, expressions: [...node.expressions, child] };
      }
      return { kind: "and", id: id(), expressions: [node, child] };
    }));
  };

  const toggleNot = (targetId: string) => {
    const current = draft();
    if (!current) return;
    publish(
      update(
        current,
        targetId,
        (node) =>
          node.kind === "not"
            ? node.expression
            : { kind: "not", id: id(), expression: node },
      ),
    );
  };

  const addRoot = (kind: "predicate" | "and" | "or") => {
    if (props.disabled) return;
    const child = defaultPredicate();
    const current = draft();
    if (kind === "predicate") {
      publish(
        current
          ? { kind: "and", id: id(), expressions: [current, child] }
          : child,
      );
      return;
    }
    const second = defaultPredicate();
    publish(
      current
        ? {
          kind: "and",
          id: id(),
          expressions: [current, {
            kind,
            id: id(),
            expressions: [child, second],
          }],
        }
        : { kind, id: id(), expressions: [child, second] },
    );
  };

  const changeAggregate = (
    node: Extract<HavingDraft, { kind: "predicate" }>,
    index: number,
  ) => {
    const candidate = props.candidates[index];
    if (!candidate) return node;
    const nextOperator =
      operatorsFor(candidate.valueKind).some((operator) =>
          operator.value === node.operator
        )
        ? node.operator
        : "eq";
    return {
      ...node,
      aggregate: candidate.aggregate,
      operator: nextOperator,
      value: node.value.kind === "variable" ||
          (node.value.kind === "integer" &&
            candidate.valueKind !== "integer") ||
          (node.value.kind !== "integer" && node.value.kind !== "variable" &&
            node.value.kind !== candidate.valueKind)
        ? defaultValue(candidate.valueKind)
        : node.value,
    };
  };

  const candidateIndex = (aggregate: SqlStructureAggregate) =>
    props.candidates.findIndex((candidate) =>
      sameAggregate(candidate.aggregate, aggregate)
    );

  const renderNode = (node: HavingDraft, depth = 0): JSX.Element => {
    if (node.kind === "predicate") {
      const selectedCandidate = () =>
        props.candidates[candidateIndex(node.aggregate)];
      const allowedOperators = () =>
        operatorsFor(selectedCandidate()?.valueKind);
      const allowedVariables = () =>
        props.variables.filter((variable) =>
          variableKind(variable.type) === selectedCandidate()?.valueKind
        );
      const setNode = (change: (predicate: typeof node) => typeof node) => {
        const current = draft();
        if (current) {
          publish(
            update(
              current,
              node.id,
              (item) => item.kind === "predicate" ? change(item) : item,
            ),
          );
        }
      };
      return (
        <div
          class="sql-structure-having-row"
          data-having-node={node.id}
          style={{ "--having-depth": depth }}
        >
          <label class="ui-field">
            <span>{t("sqlPage.structure.aggregate")}</span>
            <select
              class="ui-input"
              value={String(candidateIndex(node.aggregate))}
              disabled={props.disabled || props.candidates.length === 0}
              onChange={(event) => {
                const index = Number(event.currentTarget.value);
                setNode((predicate) => changeAggregate(predicate, index));
              }}
            >
              <For each={props.candidates}>
                {(candidate, index) => (
                  <option value={index()}>{candidate.label}</option>
                )}
              </For>
            </select>
          </label>
          <label class="ui-field">
            <span>{t("sqlPage.structure.operator")}</span>
            <select
              class="ui-input"
              value={node.operator}
              disabled={props.disabled}
              onChange={(event) =>
                setNode((predicate) => ({
                  ...predicate,
                  operator: event.currentTarget
                    .value as SqlStructureHavingOperator,
                }))}
            >
              <For each={allowedOperators()}>
                {(operator) => (
                  <option value={operator.value}>{operator.label}</option>
                )}
              </For>
            </select>
          </label>
          <label class="ui-field">
            <span>{t("sqlPage.structure.value")}</span>
            <Show
              when={selectedCandidate()?.valueKind === "boolean"}
              fallback={
                <input
                  class="ui-input"
                  type={selectedCandidate()?.valueKind === "date"
                    ? "date"
                    : selectedCandidate()?.valueKind === "timestamp"
                    ? "datetime-local"
                    : "text"}
                  value={node.value.kind === "boolean"
                    ? String(node.value.value)
                    : node.value.value}
                  inputMode={node.value.kind === "integer" ||
                      node.value.kind === "numeric"
                    ? "decimal"
                    : undefined}
                  disabled={props.disabled}
                  onInput={(event) =>
                    setNode((predicate) => ({
                      ...predicate,
                      value: {
                        kind: selectedCandidate()?.valueKind ?? "integer",
                        value: event.currentTarget.value,
                      } as SqlStructureHavingValue,
                    }))}
                />
              }
            >
              <select
                class="ui-input"
                value={node.value.kind === "boolean"
                  ? String(node.value.value)
                  : "false"}
                disabled={props.disabled}
                onChange={(event) =>
                  setNode((predicate) => ({
                    ...predicate,
                    value: {
                      kind: "boolean",
                      value: event.currentTarget.value === "true",
                    },
                  }))}
              >
                <option value="true">TRUE</option>
                <option value="false">FALSE</option>
              </select>
            </Show>
          </label>
          <Show when={allowedVariables().length > 0}>
            <label class="ui-field">
              <span>{t("sqlPage.structure.argument")}</span>
              <select
                class="ui-input"
                value={node.value.kind === "variable" ? node.value.value : ""}
                disabled={props.disabled}
                onChange={(event) =>
                  setNode((predicate) => ({
                    ...predicate,
                    value: event.currentTarget.value
                      ? { kind: "variable", value: event.currentTarget.value }
                      : defaultValue(
                        selectedCandidate()?.valueKind ?? "integer",
                      ),
                  }))}
              >
                <option value="">
                  {t("sqlPage.structure.literalValue", { index: 1 })}
                </option>
                <For each={allowedVariables()}>
                  {(variable) => (
                    <option value={variable.name}>${variable.name}</option>
                  )}
                </For>
              </select>
            </label>
          </Show>
          <IconButton
            icon="trash"
            label={t("sqlPage.structure.removeHavingCondition")}
            disabled={props.disabled}
            onClick={() => {
              const current = draft();
              if (current) publish(remove(current, node.id));
            }}
          />
          <IconButton
            icon="plus"
            label={t("sqlPage.structure.addHavingCondition")}
            disabled={props.disabled}
            onClick={() => addToGroup(node.id, defaultPredicate())}
          />
        </div>
      );
    }

    if (node.kind === "not") {
      return (
        <div class="sql-structure-having-group" data-having-node={node.id}>
          <span class="sql-structure-having-operator">NOT</span>
          {renderNode(node.expression, depth + 1)}
          <IconButton
            icon="trash"
            label={t("sqlPage.structure.removeHavingCondition")}
            disabled={props.disabled}
            onClick={() => toggleNot(node.id)}
          />
        </div>
      );
    }

    return (
      <fieldset class="sql-structure-having-group" data-having-node={node.id}>
        <legend>
          <select
            class="ui-input"
            value={node.kind}
            disabled={props.disabled}
            aria-label={t("sqlPage.structure.havingJoiner")}
            onChange={(event) => {
              const current = draft();
              if (current) {
                publish(update(current, node.id, (item) =>
                  item.kind === "and" || item.kind === "or"
                    ? {
                      ...item,
                      kind: event.currentTarget.value as "and" | "or",
                    }
                    : item));
              }
            }}
          >
            <option value="and">AND</option>
            <option value="or">OR</option>
          </select>
        </legend>
        <For each={node.expressions}>
          {(child) =>
            renderNode(child, depth + 1)}
        </For>
        <div class="sql-structure-having-controls">
          <IconButton
            icon="plus"
            label={t("sqlPage.structure.addHavingCondition")}
            disabled={props.disabled}
            onClick={() =>
              addToGroup(node.id, defaultPredicate())}
          />
          <button
            type="button"
            class="ui-button ui-button-secondary"
            disabled={props.disabled}
            onClick={() =>
              toggleNot(node.id)}
          >
            NOT
          </button>
          <IconButton
            icon="trash"
            label={t("sqlPage.structure.removeHavingCondition")}
            disabled={props.disabled}
            onClick={() => {
              const current = draft();
              if (current) {
                publish(remove(current, node.id));
              }
            }}
          />
        </div>
      </fieldset>
    );
  };

  return (
    <div class="sql-structure-having-editor">
      <Show
        when={draft()}
        fallback={
          <div class="sql-structure-having-controls">
            <button
              type="button"
              class="ui-button ui-button-secondary"
              disabled={props.disabled || props.candidates.length === 0}
              onClick={() => addRoot("predicate")}
            >
              {t("sqlPage.structure.addHavingCondition")}
            </button>
            <button
              type="button"
              class="ui-button ui-button-secondary"
              disabled={props.disabled || props.candidates.length === 0}
              onClick={() => addRoot("and")}
            >
              AND
            </button>
            <button
              type="button"
              class="ui-button ui-button-secondary"
              disabled={props.disabled || props.candidates.length === 0}
              onClick={() => addRoot("or")}
            >
              OR
            </button>
          </div>
        }
      >
        {(current) => renderNode(current())}
      </Show>
      <Show when={draft()}>
        <div class="sql-structure-having-controls">
          <button
            type="button"
            class="ui-button ui-button-secondary"
            disabled={props.disabled}
            onClick={() => addRoot("predicate")}
          >
            {t("sqlPage.structure.addHavingCondition")}
          </button>
          <button
            type="button"
            class="ui-button ui-button-secondary"
            disabled={props.disabled}
            onClick={() => addRoot("and")}
          >
            AND
          </button>
          <button
            type="button"
            class="ui-button ui-button-secondary"
            disabled={props.disabled}
            onClick={() => addRoot("or")}
          >
            OR
          </button>
        </div>
      </Show>
    </div>
  );
}
