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
  type SqlDocumentIdentifier,
  type SqlDocumentNode,
  type SqlDocumentSemanticEdit,
} from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import { IconButton } from "~/components/IconButton";
import type { SqlVariable } from "~/lib/types";
import {
  SqlConditionWorkspace,
  type SqlConditionWorkspaceHistoryState,
} from "~/components/SqlConditionWorkspace";

interface Props {
  action: "filter" | "sort";
  sql: () => string;
  scope?: () => string;
  disabled?: () => boolean;
  contexts: () => readonly SqlStructureAdapterContext[];
  onSqlChange: (sql: string) => void;
  onAdvanced: () => void;
  argumentDefinitions?: () => readonly SqlVariable[];
  onArgumentAdded?: (variable: SqlVariable) => boolean | void;
  scalarVariables?: readonly { id: string; name: string; type: string }[];
  argumentBindings?: () => readonly { name: string; variableId: string }[];
  reservedVariableNames?: () => readonly string[];
  onFilterEdit?: (change: {
    expectedSql: string;
    expectedScope: string | undefined;
    resultSql: string;
    createdArgument?: SqlVariable;
    binding?: { name: string; variableId: string };
    expectedBindings?: readonly { name: string; variableId: string }[];
    resultBindings?: readonly { name: string; variableId: string }[];
    expectedArguments?: readonly SqlVariable[];
    resultArguments?: readonly SqlVariable[];
  }) => boolean | void;
  historyState?: SqlConditionWorkspaceHistoryState;
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
  relation: string;
  qualifier?: SqlDocumentIdentifier | null;
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

type SortHistoryEntry = {
  transaction: SqlDocumentEditTransaction;
  beforeBindings: readonly { name: string; variableId: string }[];
  afterBindings: readonly { name: string; variableId: string }[];
  beforeArguments: readonly SqlVariable[];
  afterArguments: readonly SqlVariable[];
};

/** Task-oriented controls construct only typed operations; Rust owns SQL text. */
function SqlFilterSortEditorCore(props: Props) {
  const [contextIndex, setContextIndex] = createSignal(0);
  const [column, setColumn] = createSignal("");
  const [direction, setDirection] = createSignal<"asc" | "desc">("asc");
  const [nulls, setNulls] = createSignal<"" | "first" | "last">("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [sortItems, setSortItems] = createSignal<readonly SortItem[]>([]);
  const [sortDrafts, setSortDrafts] = createSignal<Record<number, SortDraft>>(
    {},
  );
  const [history, setHistory] = createSignal<SortHistoryEntry[]>([]);
  const [redoHistory, setRedoHistory] = createSignal<SortHistoryEntry[]>([]);
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
  const sortColumns = () =>
    columns().filter((item) =>
      selected()?.columnCapabilities[item.name]?.sortable === true
    );
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
      untrack(history).at(-1)?.transaction.expected_source !== source &&
      untrack(redoHistory).at(-1)?.transaction.expected_source !== source
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
      setHistory([...history(), {
        transaction: result.undo,
        beforeBindings: props.argumentBindings?.() ?? [],
        afterBindings: props.argumentBindings?.() ?? [],
        beforeArguments: props.argumentDefinitions?.() ?? [],
        afterArguments: props.argumentDefinitions?.() ?? [],
      }].slice(-32));
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
  const submitSort = async () => {
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
      ) throw new Error(t("sqlPage.structure.documentNoOperationTarget"));
      const target = document.nodes.find((node) =>
        node.id === resolved.queryId
      );
      if (!target || !column()) {
        throw new Error(t("sqlPage.structure.documentChooseColumn"));
      }
      if (!activeContext.columnCapabilities[column()]?.sortable) {
        throw new Error(t("sqlPage.structure.documentNoOperationTarget"));
      }
      const edit: SqlDocumentSemanticEdit = {
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
      if (!requestIsCurrent(request, source, scope)) {
        return;
      }
      setHistory([...history(), {
        transaction: result.undo,
        beforeBindings: props.argumentBindings?.() ?? [],
        afterBindings: props.argumentBindings?.() ?? [],
        beforeArguments: props.argumentDefinitions?.() ?? [],
        afterArguments: props.argumentDefinitions?.() ?? [],
      }].slice(-32));
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
    const entry = (direction === "undo" ? history() : redoHistory()).at(-1);
    if (!entry || entry.transaction.expected_source !== source) return;
    setBusy(true);
    setError(undefined);
    try {
      const result = await applySqlDocumentEditTransaction(
        source,
        entry.transaction,
      );
      if (!requestIsCurrent(request, source, scope)) return;
      const expectedBindings = direction === "undo"
        ? entry.afterBindings
        : entry.beforeBindings;
      const resultBindings = direction === "undo"
        ? entry.beforeBindings
        : entry.afterBindings;
      const expectedArguments = direction === "undo"
        ? entry.afterArguments
        : entry.beforeArguments;
      const resultArguments = direction === "undo"
        ? entry.beforeArguments
        : entry.afterArguments;
      if (
        JSON.stringify(props.argumentBindings?.() ?? []) !==
          JSON.stringify(expectedBindings) ||
        JSON.stringify(props.argumentDefinitions?.() ?? []) !==
          JSON.stringify(expectedArguments)
      ) throw new Error(t("sqlPage.structure.editFailed"));
      if (props.onFilterEdit) {
        const accepted = props.onFilterEdit({
          expectedSql: source,
          expectedScope: scope,
          resultSql: result.document.source,
          expectedBindings,
          resultBindings,
          expectedArguments,
          resultArguments,
        });
        if (accepted === false) {
          throw new Error(t("sqlPage.structure.editFailed"));
        }
      } else props.onSqlChange(result.document.source);
      if (direction === "undo") {
        setHistory(history().slice(0, -1));
        setRedoHistory(
          [...redoHistory(), { ...entry, transaction: result.undo }].slice(-32),
        );
      } else {
        setRedoHistory(redoHistory().slice(0, -1));
        setHistory(
          [...history(), { ...entry, transaction: result.undo }].slice(-32),
        );
      }
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
                  {selected()?.columnLabels?.[item.name] ?? item.name}
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
            history().at(-1)?.transaction.expected_source !== props.sql()}
          onClick={() => void applyHistory("undo")}
        >
          {t("sqlPage.columns.undo")}
        </button>
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={busy() || locked() ||
            redoHistory().at(-1)?.transaction.expected_source !== props.sql()}
          onClick={() => void applyHistory("redo")}
        >
          {t("sqlPage.columns.redo")}
        </button>
      </div>
      <button
        type="button"
        class="ui-button ui-button-primary"
        onClick={submitSort}
        disabled={busy() || locked()}
      >
        {t("sqlPage.structure.addSort")}
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

export function SqlFilterSortEditor(props: Props) {
  return (
    <Show
      when={props.action === "filter"}
      fallback={<SqlFilterSortEditorCore {...props} />}
    >
      <SqlConditionWorkspace
        sql={props.sql}
        scope={props.scope}
        disabled={props.disabled}
        contexts={props.contexts}
        onSqlChange={props.onSqlChange}
        onAdvanced={props.onAdvanced}
        argumentDefinitions={props.argumentDefinitions}
        onArgumentAdded={props.onArgumentAdded}
        scalarVariables={props.scalarVariables}
        argumentBindings={props.argumentBindings}
        reservedVariableNames={props.reservedVariableNames}
        historyState={props.historyState}
        onFilterEdit={props.onFilterEdit}
      />
    </Show>
  );
}
