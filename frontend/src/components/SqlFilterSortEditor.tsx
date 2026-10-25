import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
  untrack,
} from "solid-js";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import {
  applySqlDocumentEditTransaction,
  applySqlDocumentSemanticEditTransaction,
  parseSqlDocument,
  type SqlDocumentEditTransaction,
  type SqlDocumentFilterOperator,
  type SqlDocumentNode,
  type SqlDocumentSemanticEdit,
  type SqlDocumentSemanticLiteral,
} from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import { IconButton } from "~/components/IconButton";

interface Props {
  action: "filter" | "sort";
  sql: () => string;
  scope?: () => string;
  disabled?: () => boolean;
  contexts: () => readonly SqlStructureAdapterContext[];
  onSqlChange: (sql: string) => void;
  onAdvanced: () => void;
}
type SortItem = {
  id: number;
  label: string;
  direction: "asc" | "desc" | null;
  nulls: "first" | "last" | null;
  source: string;
  scope: string | undefined;
};
type SortDraft = { direction: "asc" | "desc"; nulls: "first" | "last" | null };
type RootScope = {
  queryId: number;
  selectId: number;
  relation: string;
  qualifier?: { value: string; quote_style: string | null };
};

function rootScope(nodes: readonly SqlDocumentNode[]): RootScope | null {
  const queries = nodes.filter((node) => node.kind === "query");
  const selects = nodes.filter((node) => node.kind === "select");
  if (
    queries.length !== 1 || selects.length !== 1 ||
    nodes.some((node) => node.kind === "with_clause")
  ) return null;
  const factors = nodes.filter((node) => node.kind === "table_factor");
  if (
    factors.length !== 1 || factors[0].semantic?.kind !== "table_factor" ||
    factors[0].semantic.factor_kind !== "table"
  ) return null;
  const relationNode = nodes.find((node) =>
    node.kind === "relation" && node.parent === factors[0].id &&
    node.semantic?.kind === "relation"
  );
  if (relationNode?.semantic?.kind !== "relation") return null;
  const parts = relationNode.semantic.name;
  const relation = parts.map((part) => part.value).join(".");
  const factor = factors[0].semantic;
  return {
    queryId: queries[0].id,
    selectId: selects[0].id,
    relation,
    qualifier: factor.alias,
  };
}

function queryAncestor(
  nodes: readonly SqlDocumentNode[],
  node: SqlDocumentNode,
) {
  let current: SqlDocumentNode | undefined = node;
  while (current) {
    if (current.kind === "query") return current.id;
    current = nodes.find((candidate) => candidate.id === current?.parent);
  }
  return undefined;
}

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
  const [sortDrafts, setSortDrafts] = createSignal<Record<number, SortDraft>>(
    {},
  );
  const [history, setHistory] = createSignal<SqlDocumentEditTransaction[]>([]);
  const [redoHistory, setRedoHistory] = createSignal<
    SqlDocumentEditTransaction[]
  >([]);
  const [parsedScope, setParsedScope] = createSignal<RootScope | null>(null);
  let generation = 0;
  onCleanup(() => generation++);
  let previousScope = props.scope?.();
  const contexts = () => props.contexts();
  const eligibleContexts = createMemo(() => {
    const relation = parsedScope()?.relation;
    if (!relation) return [];
    return contexts().filter((context) => context.parser.relation === relation);
  });
  const selected = createMemo(() => eligibleContexts()[contextIndex()]);
  const columns = () => selected()?.parser.columns ?? [];
  const filterColumns = () =>
    columns().filter((item) =>
      selected()?.columnCapabilities[item.name]?.filterable === true
    );
  const sortColumns = () =>
    columns().filter((item) =>
      selected()?.columnCapabilities[item.name]?.sortable === true
    );
  const supportedFilterOperators = (fieldName = column()) => {
    const capability = selected()?.columnCapabilities[fieldName];
    if (!capability?.filterable) return [];
    const supported = new Set(capability.supportedOperators);
    const mapping: Partial<Record<SqlDocumentFilterOperator, string>> = {
      eq: "eq",
      not_eq: "not_eq",
      lt: "lt",
      lte: "lt_eq",
      gt: "gt",
      gte: "gt_eq",
      like: "like",
      not_like: "not_like",
      is_null: "is_null",
      is_not_null: "is_not_null",
    };
    return operators.filter((item) => supported.has(mapping[item.value] ?? ""));
  };
  const currentOperator = () =>
    operators.find((item) => item.value === operator()) ?? operators[0];
  const locked = () => props.disabled?.() === true;
  const requestIsCurrent = (
    request: number,
    source: string,
    scope: string | undefined,
  ) =>
    request === generation && props.sql() === source &&
    props.scope?.() === scope && !locked();
  createEffect(() => {
    const source = props.sql();
    const scopeKey = props.scope?.();
    if (scopeKey !== previousScope) {
      previousScope = scopeKey;
      setHistory([]);
      setRedoHistory([]);
    } else if (
      untrack(history).at(-1)?.expected_source !== source &&
      untrack(redoHistory).at(-1)?.expected_source !== source
    ) {
      setHistory([]);
      setRedoHistory([]);
    }
    let current = true;
    onCleanup(() => {
      current = false;
    });
    void parseSqlDocument(source).then((document) => {
      if (!current || props.sql() !== source || props.scope?.() !== scopeKey) {
        return;
      }
      const resolved = rootScope(document.nodes);
      setParsedScope(resolved);
      setContextIndex(0);
      setSortDrafts({});
      const exactContext = resolved === null ? undefined : contexts().find(
        (context) => context.parser.relation === resolved.relation,
      );
      setSortItems(
        exactContext
          ? document.nodes.flatMap((node) =>
            node.kind === "order_by_item" &&
              node.semantic?.kind === "order_by_item" &&
              resolved !== null &&
              queryAncestor(document.nodes, node) === resolved.queryId
              ? (() => {
                const expression = document.nodes.find((child) =>
                  child.parent === node.id &&
                  ["identifier_expression", "compound_identifier_expression"]
                    .includes(child.kind)
                );
                const fieldName = expression?.semantic?.kind === "identifier"
                  ? expression.semantic.value
                  : expression?.semantic?.kind === "compound_identifier"
                  ? expression.semantic.parts.at(-1)?.value
                  : undefined;
                if (
                  !fieldName ||
                  exactContext.columnCapabilities[fieldName]?.sortable !== true
                ) return [];
                return [{
                  id: node.id,
                  label: (() => {
                    if (expression?.semantic?.kind === "identifier") {
                      return exactContext.columnLabels
                        ?.[expression.semantic.value] ??
                        expression.semantic.value;
                    }
                    if (expression?.semantic?.kind === "compound_identifier") {
                      const parts = expression.semantic.parts;
                      const name = parts.at(-1)?.value;
                      const label = name
                        ? exactContext.columnLabels?.[name]
                        : undefined;
                      return label
                        ? [
                          ...parts.slice(0, -1).map((part) => part.value),
                          label,
                        ]
                          .join(".")
                        : parts.map((part) => part.value).join(".");
                    }
                    return t("sqlPage.structure.documentFilterExpression");
                  })(),
                  direction: node.semantic.direction,
                  nulls: node.semantic.nulls_order,
                  source,
                  scope: scopeKey,
                }];
              })()
              : []
          )
          : [],
      );
    }).catch(() => {
      if (current && props.sql() === source && props.scope?.() === scopeKey) {
        setParsedScope(null);
        setSortItems([]);
      }
    });
  });
  const applyOrderEdit = async (
    edit: SqlDocumentSemanticEdit,
    source: string,
    scope: string | undefined,
  ) => {
    if (busy() || locked()) return;
    if (props.sql() !== source || props.scope?.() !== scope || locked()) return;
    const request = generation;
    setBusy(true);
    setError(undefined);
    try {
      const document = await parseSqlDocument(source);
      if (!requestIsCurrent(request, source, scope)) return;
      const result = await applySqlDocumentSemanticEditTransaction(source, {
        contract_version: document.contract_version,
        expected_source: source,
        edits: [edit],
      });
      if (!requestIsCurrent(request, source, scope)) return;
      setHistory([...history(), result.undo].slice(-32));
      setRedoHistory([]);
      props.onSqlChange(result.document.source);
    } catch (cause) {
      if (requestIsCurrent(request, source, scope)) {
        setError(
          cause instanceof Error
            ? cause.message
            : t("sqlPage.structure.editFailed"),
        );
      }
    } finally {
      if (request === generation) setBusy(false);
    }
  };
  const submit = async () => {
    if (busy() || locked()) return;
    const source = props.sql();
    const scope = props.scope?.();
    const request = generation;
    setBusy(true);
    setError(undefined);
    try {
      const document = await parseSqlDocument(source);
      if (!requestIsCurrent(request, source, scope)) return;
      const resolved = rootScope(document.nodes);
      const activeContext = selected();
      if (
        !resolved || !activeContext ||
        !eligibleContexts().includes(activeContext) ||
        activeContext.parser.relation !== resolved.relation
      ) {
        throw new Error(t("sqlPage.structure.documentNoOperationTarget"));
      }
      const targetId = props.action === "filter"
        ? resolved.selectId
        : resolved.queryId;
      const target = document.nodes.find((node) => node.id === targetId);
      if (!target || !column()) {
        throw new Error(t("sqlPage.structure.documentChooseColumn"));
      }
      const capability = activeContext.columnCapabilities[column()];
      if (
        props.action === "filter" &&
        (!capability?.filterable ||
          !supportedFilterOperators().some((item) => item.value === operator()))
      ) {
        throw new Error(t("sqlPage.structure.documentNoOperationTarget"));
      }
      if (props.action === "sort" && !capability?.sortable) {
        throw new Error(t("sqlPage.structure.documentNoOperationTarget"));
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
            qualifier: resolved.qualifier,
            column: {
              value: column(),
              quote_style: /^[A-Za-z_][A-Za-z0-9_$]*$/.test(column())
                ? null
                : '"',
            },
          },
          operator: operator(),
          literal,
        }
        : {
          kind: "add_order_by_column",
          node_id: target.id,
          expected_kind: "query",
          column: {
            qualifier: resolved.qualifier,
            column: {
              value: column(),
              quote_style: /^[A-Za-z_][A-Za-z0-9_$]*$/.test(column())
                ? null
                : '"',
            },
          },
          direction: direction(),
          nulls_order: nulls() || null,
        };
      const result = await applySqlDocumentSemanticEditTransaction(source, {
        contract_version: document.contract_version,
        expected_source: source,
        edits: [edit],
      });
      if (!requestIsCurrent(request, source, scope)) return;
      setHistory([...history(), result.undo].slice(-32));
      setRedoHistory([]);
      props.onSqlChange(result.document.source);
    } catch (cause) {
      if (requestIsCurrent(request, source, scope)) {
        setError(
          cause instanceof Error
            ? cause.message
            : t("sqlPage.structure.editFailed"),
        );
      }
    } finally {
      if (request === generation) setBusy(false);
    }
  };
  const applyHistory = async (direction: "undo" | "redo") => {
    if (busy() || locked()) return;
    const source = props.sql();
    const scope = props.scope?.();
    const request = generation;
    const transaction = (direction === "undo" ? history() : redoHistory()).at(
      -1,
    );
    if (!transaction || transaction.expected_source !== source) return;
    setBusy(true);
    setError(undefined);
    try {
      const result = await applySqlDocumentEditTransaction(source, transaction);
      if (!requestIsCurrent(request, source, scope)) return;
      if (direction === "undo") {
        setHistory(history().slice(0, -1));
        setRedoHistory([...redoHistory(), result.undo].slice(-32));
      } else {
        setRedoHistory(redoHistory().slice(0, -1));
        setHistory([...history(), result.undo].slice(-32));
      }
      props.onSqlChange(result.document.source);
    } catch (cause) {
      if (requestIsCurrent(request, source, scope)) {
        setError(
          cause instanceof Error
            ? cause.message
            : t("sqlPage.structure.editFailed"),
        );
      }
    } finally {
      if (request === generation) setBusy(false);
    }
  };
  return (
    <div class="sql-filter-sort-editor" aria-busy={busy() || undefined}>
      <Show when={eligibleContexts().length > 1}>
        <label class="ui-field">
          <span>{t("sqlPage.structure.filterSortScope")}</span>
          <select
            class="ui-input"
            value={contextIndex()}
            onChange={(event) =>
              setContextIndex(Number(event.currentTarget.value))}
          >
            <For each={eligibleContexts()}>
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
            disabled={busy() || locked()}
            value={column()}
            onChange={(event) => {
              const fieldName = event.currentTarget.value;
              setColumn(fieldName);
              const nextOperator = supportedFilterOperators(fieldName)[0]
                ?.value;
              if (nextOperator) setOperator(nextOperator);
            }}
          >
            <option value="" />
            <For each={filterColumns()}>
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
            disabled={busy() || locked()}
            value={operator()}
            onChange={(event) =>
              setOperator(
                event.currentTarget.value as SqlDocumentFilterOperator,
              )}
          >
            <For each={supportedFilterOperators()}>
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
                  disabled={busy() || locked()}
                  value={value()}
                  onInput={(event) => setValue(event.currentTarget.value)}
                />
              }
            >
              <select
                class="ui-input"
                disabled={busy() || locked()}
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
              disabled={busy() || locked()}
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
          {(item, index) => {
            const draft = () =>
              sortDrafts()[item.id] ?? {
                direction: item.direction ?? "asc",
                nulls: item.nulls,
              };
            return (
              <div
                class="sql-filter-sort-editor__sort-row"
                role="group"
                aria-label={`${t("sqlPage.structure.sort")} ${index() + 1}`}
              >
                <span>{item.label}</span>
                <select
                  class="ui-input"
                  disabled={busy() || locked()}
                  aria-label={t("sqlPage.structure.direction")}
                  value={draft().direction}
                  onChange={(event) => {
                    setSortDrafts({
                      ...sortDrafts(),
                      [item.id]: {
                        ...draft(),
                        direction: event.currentTarget.value as "asc" | "desc",
                      },
                    });
                  }}
                >
                  <option value="asc">
                    {t("sqlPage.structure.ascending")}
                  </option>
                  <option value="desc">
                    {t("sqlPage.structure.descending")}
                  </option>
                </select>
                <select
                  class="ui-input"
                  disabled={busy() || locked()}
                  aria-label={t("sqlPage.structure.nullOrder")}
                  value={draft().nulls ?? ""}
                  onChange={(event) => {
                    setSortDrafts({
                      ...sortDrafts(),
                      [item.id]: {
                        ...draft(),
                        nulls: (event.currentTarget.value || null) as
                          | "first"
                          | "last"
                          | null,
                      },
                    });
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
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  disabled={busy() || locked()}
                  onClick={() =>
                    void applyOrderEdit(
                      {
                        kind: "set_order_by_options",
                        node_id: item.id,
                        expected_kind: "order_by_item",
                        direction: draft().direction,
                        nulls_order: draft().nulls,
                      },
                      item.source,
                      item.scope,
                    )}
                >
                  {t("sqlPage.structure.applySort")}
                </button>
                <IconButton
                  icon="close"
                  label={t("sqlPage.structure.removeSort")}
                  onClick={() => {
                    if (item) {
                      void applyOrderEdit(
                        {
                          kind: "remove_order_by_item",
                          node_id: item.id,
                          expected_kind: "order_by_item",
                        },
                        item.source,
                        item.scope,
                      );
                    }
                  }}
                />
                <IconButton
                  icon="arrow-up"
                  label={t("sqlPage.structure.documentMoveOrderEarlier")}
                  disabled={busy() || locked() || index() === 0}
                  onClick={() => {
                    if (item) {
                      void applyOrderEdit(
                        {
                          kind: "move_order_by_item",
                          node_id: item.id,
                          expected_kind: "order_by_item",
                          to_index: index() - 1,
                        },
                        item.source,
                        item.scope,
                      );
                    }
                  }}
                />
                <IconButton
                  icon="arrow-down"
                  label={t("sqlPage.structure.documentMoveOrderLater")}
                  disabled={busy() || locked() ||
                    index() === sortItems().length - 1}
                  onClick={() => {
                    if (item) {
                      void applyOrderEdit(
                        {
                          kind: "move_order_by_item",
                          node_id: item.id,
                          expected_kind: "order_by_item",
                          to_index: index() + 1,
                        },
                        item.source,
                        item.scope,
                      );
                    }
                  }}
                />
              </div>
            );
          }}
        </For>
        <label class="ui-field">
          <span>{t("sqlPage.structure.column")}</span>
          <select
            class="ui-input"
            disabled={busy() || locked()}
            value={column()}
            onChange={(event) => setColumn(event.currentTarget.value)}
          >
            <option value="" />
            <For each={sortColumns()}>
              {(item) => (
                <option value={item.name}>
                  {selected()?.columnLabels[item.name] ?? item.name}
                </option>
              )}
            </For>
          </select>
        </label>
        <label class="ui-field">
          <span>{t("sqlPage.structure.direction")}</span>
          <select
            class="ui-input"
            disabled={busy() || locked()}
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
            disabled={busy() || locked()}
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
      <div class="sql-filter-sort-editor__history">
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={busy() || locked() ||
            history().at(-1)?.expected_source !== props.sql()}
          onClick={() => void applyHistory("undo")}
        >
          {t("sqlPage.columns.undo")}
        </button>
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={busy() || locked() ||
            redoHistory().at(-1)?.expected_source !== props.sql()}
          onClick={() => void applyHistory("redo")}
        >
          {t("sqlPage.columns.redo")}
        </button>
      </div>
      <button
        type="button"
        class="ui-button ui-button-primary"
        onClick={submit}
        disabled={busy() || locked()}
      >
        {props.action === "filter"
          ? t("sqlPage.structure.addFilter")
          : t("sqlPage.structure.addSort")}
      </button>
      <button
        type="button"
        class="ui-button ui-button-secondary"
        onClick={props.onAdvanced}
        disabled={busy() || locked()}
      >
        {t("sqlPage.structure.advancedStructure")}
      </button>
    </div>
  );
}
