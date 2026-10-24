import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import {
  applySqlDocumentSemanticEditTransaction,
  parseSqlDocument,
  type SqlDocumentFilterOperator,
  type SqlDocumentSemanticEdit,
  type SqlDocumentSemanticLiteral,
} from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import { IconButton } from "~/components/IconButton";

interface Props {
  action: "filter" | "sort";
  sql: () => string;
  scope?: () => string;
  contexts: () => readonly SqlStructureAdapterContext[];
  onSqlChange: (sql: string) => void;
  onAdvanced: () => void;
}
type SortItem = {
  id: number;
  label: string;
  direction: "asc" | "desc" | null;
  nulls: "first" | "last" | null;
};

const operators: {
  value: SqlDocumentFilterOperator;
  label: string;
  valueInput: boolean;
}[] = [
  { value: "eq", label: "=", valueInput: true },
  { value: "not_eq", label: "≠", valueInput: true },
  { value: "lt", label: "<", valueInput: true },
  { value: "lte", label: "≤", valueInput: true },
  { value: "gt", label: ">", valueInput: true },
  { value: "gte", label: "≥", valueInput: true },
  { value: "like", label: "LIKE", valueInput: true },
  { value: "not_like", label: "NOT LIKE", valueInput: true },
  { value: "is_null", label: "IS NULL", valueInput: false },
  { value: "is_not_null", label: "IS NOT NULL", valueInput: false },
];
type ValueKind = "string" | "number" | "boolean";

/** Task-oriented controls construct only typed operations; Rust owns SQL text. */
export function SqlFilterSortEditor(props: Props) {
  const [contextIndex, setContextIndex] = createSignal(0);
  const [column, setColumn] = createSignal("");
  const [operator, setOperator] = createSignal<SqlDocumentFilterOperator>("eq");
  const [valueKind, setValueKind] = createSignal<ValueKind>("string");
  const [value, setValue] = createSignal("");
  const [direction, setDirection] = createSignal<"asc" | "desc">("asc");
  const [nulls, setNulls] = createSignal<"" | "first" | "last">("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [sortItems, setSortItems] = createSignal<readonly SortItem[]>([]);
  const contexts = () => props.contexts();
  const selected = createMemo(() => contexts()[contextIndex()]);
  const columns = () => selected()?.parser.columns ?? [];
  const currentOperator = () =>
    operators.find((item) => item.value === operator()) ?? operators[0];
  createEffect(() => {
    const source = props.sql();
    let current = true;
    void parseSqlDocument(source).then((document) => {
      if (!current || props.sql() !== source) return;
      setSortItems(
        document.nodes.flatMap((node) =>
          node.kind === "order_by_item" &&
            node.semantic?.kind === "order_by_item"
            ? [{
              id: node.id,
              label: (() => {
                const expression = document.nodes.find((child) =>
                  child.parent === node.id &&
                  ["identifier_expression", "compound_identifier_expression"]
                    .includes(child.kind)
                );
                if (expression?.semantic?.kind === "identifier") {
                  return selected()?.columnLabels[expression.semantic.value] ??
                    expression.semantic.value;
                }
                if (expression?.semantic?.kind === "compound_identifier") {
                  const parts = expression.semantic.parts;
                  const name = parts.at(-1)?.value;
                  const label = name
                    ? selected()?.columnLabels[name]
                    : undefined;
                  return label
                    ? [...parts.slice(0, -1).map((part) => part.value), label]
                      .join(".")
                    : parts.map((part) => part.value).join(".");
                }
                return t("sqlPage.structure.documentFilterExpression");
              })(),
              direction: node.semantic.direction,
              nulls: node.semantic.nulls_order,
            }]
            : []
        ),
      );
    }).catch(() => {
      if (current) setSortItems([]);
    });
    return () => {
      current = false;
    };
  });
  const applyOrderEdit = async (edit: SqlDocumentSemanticEdit) => {
    if (busy()) return;
    const source = props.sql();
    const scope = props.scope?.();
    setBusy(true);
    setError(undefined);
    try {
      const document = await parseSqlDocument(source);
      if (props.sql() !== source || props.scope?.() !== scope) return;
      const result = await applySqlDocumentSemanticEditTransaction(source, {
        contract_version: document.contract_version,
        expected_source: source,
        edits: [edit],
      });
      if (props.sql() !== source || props.scope?.() !== scope) return;
      props.onSqlChange(result.document.source);
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : t("sqlPage.structure.editFailed"),
      );
    } finally {
      setBusy(false);
    }
  };
  const submit = async () => {
    if (busy()) return;
    const source = props.sql();
    const scope = props.scope?.();
    setBusy(true);
    setError(undefined);
    try {
      const document = await parseSqlDocument(source);
      if (props.sql() !== source || props.scope?.() !== scope) return;
      const targetKind = props.action === "filter" ? "select" : "query";
      const targets = document.nodes.filter((node) => node.kind === targetKind);
      if (targets.length !== 1) {
        throw new Error(t("sqlPage.structure.documentNoOperationTarget"));
      }
      const target = targets[0];
      const relation = selected()?.parser.relation;
      if (!column() || !relation) {
        throw new Error(t("sqlPage.structure.documentChooseColumn"));
      }
      let literal: SqlDocumentSemanticLiteral | null = null;
      if (props.action === "filter" && currentOperator().valueInput) {
        if (valueKind() !== "string" && !value()) {
          throw new Error(t("sqlPage.structure.documentEnterValue"));
        }
        if (valueKind() === "number") {
          literal = { kind: "number", value: value() };
        } else if (valueKind() === "boolean") {
          literal = { kind: "boolean", value: value() === "true" };
        } else literal = { kind: "string", value: value() };
      }
      const edit: SqlDocumentSemanticEdit = props.action === "filter"
        ? {
          kind: "add_filter_comparison",
          node_id: target.id,
          expected_kind: "select",
          column: {
            qualifier: { value: relation, quote_style: null },
            column: { value: column(), quote_style: null },
          },
          operator: operator(),
          literal,
        }
        : {
          kind: "add_order_by_column",
          node_id: target.id,
          expected_kind: "query",
          column: {
            qualifier: { value: relation, quote_style: null },
            column: { value: column(), quote_style: null },
          },
          direction: direction(),
          nulls_order: nulls() || null,
        };
      const result = await applySqlDocumentSemanticEditTransaction(source, {
        contract_version: document.contract_version,
        expected_source: source,
        edits: [edit],
      });
      if (props.sql() !== source || props.scope?.() !== scope) return;
      props.onSqlChange(result.document.source);
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : t("sqlPage.structure.editFailed"),
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <div class="sql-filter-sort-editor" aria-busy={busy() || undefined}>
      <Show when={contexts().length > 1}>
        <label class="ui-field">
          <span>{t("sqlPage.structure.filterSortScope")}</span>
          <select
            class="ui-input"
            value={contextIndex()}
            onChange={(event) =>
              setContextIndex(Number(event.currentTarget.value))}
          >
            <For each={contexts()}>
              {(context, index) => (
                <option value={index()}>
                  {context.label ?? context.parser.relation}
                </option>
              )}
            </For>
          </select>
        </label>
      </Show>
      <Show when={props.action === "filter"}>
        <label class="ui-field">
          <span>{t("sqlPage.structure.column")}</span>
          <select
            class="ui-input"
            value={column()}
            onChange={(event) => setColumn(event.currentTarget.value)}
          >
            <option value="" />
            <For each={columns()}>
              {(item) => (
                <option value={item.name}>
                  {selected()?.columnLabels[item.name] ?? item.name}
                </option>
              )}
            </For>
          </select>
        </label>
        <label class="ui-field">
          <span>{t("sqlPage.structure.operator")}</span>
          <select
            class="ui-input"
            value={operator()}
            onChange={(event) =>
              setOperator(
                event.currentTarget.value as SqlDocumentFilterOperator,
              )}
          >
            <For each={operators}>
              {(item) => <option value={item.value}>{item.label}</option>}
            </For>
          </select>
        </label>
        <Show when={currentOperator().valueInput}>
          <label class="ui-field">
            <span>{t("sqlPage.structure.value")}</span>
            <Show
              when={valueKind() === "boolean"}
              fallback={
                <input
                  class="ui-input"
                  value={value()}
                  onInput={(event) => setValue(event.currentTarget.value)}
                />
              }
            >
              <select
                class="ui-input"
                value={value()}
                onChange={(event) => setValue(event.currentTarget.value)}
              >
                <option value="" />
                <option value="true">TRUE</option>
                <option value="false">FALSE</option>
              </select>
            </Show>
          </label>
          <label class="ui-field">
            <span>{t("sqlPage.structure.documentValueKind")}</span>
            <select
              class="ui-input"
              value={valueKind()}
              onChange={(event) =>
                setValueKind(event.currentTarget.value as ValueKind)}
            >
              <option value="string">
                {t("sqlPage.structure.documentString")}
              </option>
              <option value="number">
                {t("sqlPage.structure.documentNumber")}
              </option>
              <option value="boolean">
                {t("sqlPage.structure.literal.boolean")}
              </option>
            </select>
          </label>
        </Show>
      </Show>
      <Show when={props.action === "sort"}>
        <For each={sortItems()}>
          {(item, index) => (
            <div
              class="sql-filter-sort-editor__sort-row"
              role="group"
              aria-label={`${t("sqlPage.structure.sort")} ${index() + 1}`}
            >
              <span>{item.label}</span>
              <select
                class="ui-input"
                aria-label={t("sqlPage.structure.direction")}
                value={item.direction ?? "asc"}
                onChange={(event) => {
                  const current = sortItems()[index()];
                  if (current) {
                    void applyOrderEdit({
                      kind: "set_order_by_options",
                      node_id: current.id,
                      expected_kind: "order_by_item",
                      direction: event.currentTarget.value as "asc" | "desc",
                      nulls_order: current.nulls,
                    });
                  }
                }}
              >
                <option value="asc">{t("sqlPage.structure.ascending")}</option>
                <option value="desc">
                  {t("sqlPage.structure.descending")}
                </option>
              </select>
              <select
                class="ui-input"
                aria-label={t("sqlPage.structure.nullOrder")}
                value={item.nulls ?? ""}
                onChange={(event) => {
                  const current = sortItems()[index()];
                  if (current) {
                    void applyOrderEdit({
                      kind: "set_order_by_options",
                      node_id: current.id,
                      expected_kind: "order_by_item",
                      direction: current.direction ?? "asc",
                      nulls_order: (event.currentTarget.value || null) as
                        | "first"
                        | "last"
                        | null,
                    });
                  }
                }}
              >
                <option value="">
                  {t("sqlPage.structure.nullOrderDefault")}
                </option>
                <option value="first">
                  {t("sqlPage.structure.nullOrderFirst")}
                </option>
                <option value="last">
                  {t("sqlPage.structure.nullOrderLast")}
                </option>
              </select>
              <IconButton
                icon="close"
                label={t("sqlPage.structure.removeSort")}
                onClick={() => {
                  const current = sortItems()[index()];
                  if (current) {
                    void applyOrderEdit({
                      kind: "remove_order_by_item",
                      node_id: current.id,
                      expected_kind: "order_by_item",
                    });
                  }
                }}
              />
              <IconButton
                icon="arrow-up"
                label={t("sqlPage.structure.documentMoveOrderEarlier")}
                disabled={index() === 0}
                onClick={() => {
                  const current = sortItems()[index()];
                  if (current) {
                    void applyOrderEdit({
                      kind: "move_order_by_item",
                      node_id: current.id,
                      expected_kind: "order_by_item",
                      to_index: index() - 1,
                    });
                  }
                }}
              />
              <IconButton
                icon="arrow-down"
                label={t("sqlPage.structure.documentMoveOrderLater")}
                disabled={index() === sortItems().length - 1}
                onClick={() => {
                  const current = sortItems()[index()];
                  if (current) {
                    void applyOrderEdit({
                      kind: "move_order_by_item",
                      node_id: current.id,
                      expected_kind: "order_by_item",
                      to_index: index() + 1,
                    });
                  }
                }}
              />
            </div>
          )}
        </For>
        <label class="ui-field">
          <span>{t("sqlPage.structure.column")}</span>
          <select
            class="ui-input"
            value={column()}
            onChange={(event) => setColumn(event.currentTarget.value)}
          >
            <option value="" />
            <For each={columns()}>
              {(item) => <option value={item.name}>{item.name}</option>}
            </For>
          </select>
        </label>
        <label class="ui-field">
          <span>{t("sqlPage.structure.direction")}</span>
          <select
            class="ui-input"
            value={direction()}
            onChange={(event) =>
              setDirection(event.currentTarget.value as "asc" | "desc")}
          >
            <option value="asc">{t("sqlPage.structure.ascending")}</option>
            <option value="desc">{t("sqlPage.structure.descending")}</option>
          </select>
        </label>
        <label class="ui-field">
          <span>{t("sqlPage.structure.nullOrder")}</span>
          <select
            class="ui-input"
            value={nulls()}
            onChange={(event) =>
              setNulls(event.currentTarget.value as "" | "first" | "last")}
          >
            <option value="" />
            <option value="first">
              {t("sqlPage.structure.nullOrderFirst")}
            </option>
            <option value="last">{t("sqlPage.structure.nullOrderLast")}</option>
          </select>
        </label>
      </Show>
      <Show when={error()}>
        <p role="alert" class="ui-text-danger">{error()}</p>
      </Show>
      <button
        type="button"
        class="ui-button ui-button-primary"
        onClick={submit}
        disabled={busy()}
      >
        {props.action === "filter"
          ? t("sqlPage.structure.addFilter")
          : t("sqlPage.structure.addSort")}
      </button>
      <button
        type="button"
        class="ui-button ui-button-secondary"
        onClick={props.onAdvanced}
      >
        {t("sqlPage.structure.advancedStructure")}
      </button>
    </div>
  );
}
