import {
  createEffect,
  createSignal,
  createUniqueId,
  For,
  Show,
} from "solid-js";
import { IconButton } from "~/components/IconButton";
import { handleDialogKeyDown, useDialogFocus } from "~/components/dialog-focus";
import { t } from "~/lib/i18n";
import { normalizeSqlVariables } from "~/lib/sql";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import {
  analyzeSqlStructure,
  prepareSqlStructureParser,
  proposeSqlStructureOperation,
} from "~/lib/ugoite-client";
import type {
  SqlStructureAggregate,
  SqlStructureAggregateFunction,
  SqlStructureAnalysis,
  SqlStructureFilter,
  SqlStructureFilterOperator,
  SqlStructureOperation,
  SqlStructureProposal,
  SqlStructureSort,
  SqlStructureVariableType,
} from "~/lib/ugoite-client";

type StructureAction =
  | "columns"
  | "filter"
  | "sort"
  | "aggregate"
  | "group_by"
  | "distinct";
type FilterDraft = {
  column: string;
  operator: SqlStructureFilterOperator;
  parameter: string;
};
type SortDraft = { column: string; direction: "asc" | "desc" };
type AnalyzedStructure = {
  context: SqlStructureAdapterContext;
  analysis: SqlStructureAnalysis;
  identity: string;
};
type AnalysisState =
  | { status: "loading" }
  | { status: "ready"; value: AnalyzedStructure }
  | { status: "no_context" }
  | { status: "error" };

interface SqlStructureToolbarProps {
  sql: () => string;
  contexts: () => readonly SqlStructureAdapterContext[];
  onSqlChange?: (sql: string) => void;
  onActionsChange: (actions: readonly SQLWorkspaceAction[]) => void;
  disabled?: () => boolean;
}

const filterOperators: SqlStructureFilterOperator[] = [
  "eq",
  "not_eq",
  "lt",
  "lt_eq",
  "gt",
  "gt_eq",
  "is_null",
  "is_not_null",
];

const operatorKey = (operator: SqlStructureFilterOperator): string =>
  `sqlPage.structure.operator.${operator}`;

const isNullOperator = (operator: SqlStructureFilterOperator) =>
  operator === "is_null" || operator === "is_not_null";

const variableTypeForColumn = (
  type: string,
): SqlStructureVariableType | undefined => {
  switch (type) {
    case "string":
    case "boolean":
    case "integer":
    case "float":
    case "date":
    case "timestamp":
      return type;
    default:
      return undefined;
  }
};

const operatorsForColumn = (
  context: SqlStructureAdapterContext | undefined,
  columnName: string,
): SqlStructureFilterOperator[] =>
  (context?.columnCapabilities[columnName]?.supportedOperators ?? []).filter(
    (operator) => filterOperators.includes(operator),
  );

const unavailableReason = () => t("sqlPage.structureUnsupported");

function analysisReason(state: AnalysisState): string | undefined {
  if (state.status === "loading") return t("sqlPage.structureAnalyzing");
  if (state.status === "no_context") return t("sqlPage.structureNoForm");
  if (state.status === "error") return t("sqlPage.structureUnavailable");
  if (state.value.analysis.status !== "supported") return unavailableReason();
  return undefined;
}

export function SqlStructureToolbar(props: SqlStructureToolbarProps) {
  const dialogTitleId = `sql-structure-title-${createUniqueId()}`;
  const [analysisState, setAnalysisState] = createSignal<AnalysisState>(
    { status: "no_context" },
  );
  const [activeAction, setActiveAction] = createSignal<StructureAction | null>(
    null,
  );
  const [selectedColumns, setSelectedColumns] = createSignal<string[]>([]);
  const [filters, setFilters] = createSignal<FilterDraft[]>([]);
  const [sorts, setSorts] = createSignal<SortDraft[]>([]);
  const [aggregates, setAggregates] = createSignal<SqlStructureAggregate[]>([]);
  const [groupByColumns, setGroupByColumns] = createSignal<string[]>([]);
  const [selectDistinct, setSelectDistinct] = createSignal(false);
  const [proposal, setProposal] = createSignal<SqlStructureProposal>();
  const [proposalContextIdentity, setProposalContextIdentity] = createSignal<
    string
  >();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let analysisGeneration = 0;
  let proposalGeneration = 0;
  let dialog: HTMLDivElement | undefined;

  createEffect(() => {
    const sourceSql = props.sql();
    const sql = normalizeSqlVariables(sourceSql).sql;
    const contexts = [...props.contexts()];
    const contextsKey = JSON.stringify(contexts);
    const inputIdentity = JSON.stringify({ sourceSql, sql, contextsKey });
    const generation = ++analysisGeneration;
    proposalGeneration += 1;
    setBusy(false);
    setProposal(undefined);
    setProposalContextIdentity(undefined);
    setActiveAction(null);
    setSelectedColumns([]);
    setFilters([]);
    setSorts([]);
    setAggregates([]);
    setGroupByColumns([]);
    setSelectDistinct(false);
    setError(null);

    if (contexts.length === 0) {
      setAnalysisState({ status: "no_context" });
      return;
    }
    setAnalysisState({ status: "loading" });
    const timer = setTimeout(() => {
      void (async () => {
        try {
          await prepareSqlStructureParser();
          const outcomes = await Promise.all(
            contexts.map(async (context) => ({
              context,
              analysis: await analyzeSqlStructure(sql, context.parser),
            })),
          );
          if (generation !== analysisGeneration) return;
          const supported = outcomes.filter((item) =>
            item.analysis.status === "supported"
          );
          const chosen = supported.length === 1
            ? supported[0]
            : supported.length === 0
            ? outcomes[0]
            : undefined;
          if (chosen) {
            setAnalysisState({
              status: "ready",
              value: {
                ...chosen,
                identity: JSON.stringify({
                  inputIdentity,
                  contextIdentity: chosen.context.identity,
                }),
              },
            });
          } else {
            setAnalysisState({ status: "error" });
          }
        } catch {
          if (generation === analysisGeneration) {
            setAnalysisState({ status: "error" });
          }
        }
      })();
    }, 120);
    // Tracking contextsKey makes changes to adapter-owned schema metadata
    // invalidate an in-flight analysis even when the array identity is reused.
    void contextsKey;
    return () => clearTimeout(timer);
  });

  const ready = (): AnalyzedStructure | undefined => {
    const state = analysisState();
    return state.status === "ready" ? state.value : undefined;
  };

  const capability = (action: StructureAction) => {
    const current = ready()?.analysis;
    return current?.capabilities[action];
  };

  const actionDisabled = (action: StructureAction) =>
    Boolean(
      props.disabled?.() || !props.onSqlChange ||
        analysisState().status !== "ready" ||
        ready()?.analysis.status !== "supported" ||
        !capability(action)?.enabled || !hasEditableCapabilities(action),
    );

  const actionReason = (action: StructureAction) => {
    const commonReason = analysisReason(analysisState());
    if (commonReason) return commonReason;
    if (props.disabled?.() || !props.onSqlChange) {
      return t("sqlPage.structureReadOnly");
    }
    const current = capability(action);
    return current && (!current.enabled || !hasEditableCapabilities(action))
      ? unavailableReason()
      : undefined;
  };

  const actionLabel = (action: StructureAction) =>
    t(`sqlPage.structure.${action}`);

  const columnCapability = (columnName: string) =>
    ready()?.context.columnCapabilities[columnName];

  const aggregateColumnsFor = (
    fn: SqlStructureAggregateFunction,
    distinct = false,
  ) =>
    (ready()?.context.parser.columns ?? []).filter((column) => {
      const capability = columnCapability(column.name);
      return capability?.aggregateFunctions.includes(fn) &&
        (!distinct || capability.distinctable);
    });

  const aggregateFunctionsAvailable = (): SqlStructureAggregateFunction[] => {
    const functions: SqlStructureAggregateFunction[] = [
      "count",
      "sum",
      "avg",
      "min",
      "max",
    ];
    return functions.filter((fn) =>
      fn === "count" || aggregateColumnsFor(fn).length > 0
    );
  };

  const aggregateIsEditable = (aggregate: SqlStructureAggregate) => {
    if (!aggregate.column) {
      return aggregate.function === "count" && !aggregate.distinct;
    }
    const capability = columnCapability(aggregate.column);
    return Boolean(
      capability?.projectable &&
        capability.aggregateFunctions.includes(aggregate.function) &&
        (!aggregate.distinct || capability.distinctable),
    );
  };

  const groupableColumns = () =>
    (ready()?.context.parser.columns ?? []).filter((column) =>
      columnCapability(column.name)?.groupable
    );

  const existingGroupByIsEditable = () =>
    (ready()?.analysis.group_by ?? []).every((column) =>
      columnCapability(column)?.groupable
    );

  const existingAggregatesAreEditable = () =>
    (ready()?.analysis.aggregates ?? []).every(aggregateIsEditable);

  const aggregateRowsValid = () => {
    if (!aggregates().every(aggregateIsEditable)) return false;
    const current = ready();
    if (!current || aggregates().length === 0) return true;
    const projection = current.analysis.projection;
    return projection.includes("*") ||
      projection.every((column) => groupByColumns().includes(column));
  };

  const aggregateOperationValid = () =>
    aggregateRowsValid() &&
    (aggregates().length > 0 ||
      (ready()?.analysis.aggregates.length ?? 0) > 0);

  const groupByRowsValid = () => {
    const selected = groupByColumns();
    const current = ready();
    if (
      !current || new Set(selected).size !== selected.length ||
      !selected.every((column) =>
        groupableColumns().some((item) => item.name === column)
      )
    ) return false;
    const projection = current.analysis.projection;
    if (projection.includes("*")) {
      // Wildcard projection cannot be safely reconciled with selected group
      // keys. Clearing an existing group is safe when no aggregate remains.
      return selected.length === 0 &&
        current.analysis.aggregates.length === 0;
    }
    return current.analysis.aggregates.length === 0 &&
        selected.length === 0 ||
      projection.every((column) => selected.includes(column));
  };

  const distinctProjectionIsEditable = () => {
    const current = ready();
    if (!current) return false;
    // Turning DISTINCT off is a safe structural simplification even if the
    // existing projection has no trusted comparable type metadata.
    if (current.analysis.distinct) return true;
    const projected = current.analysis.projection.includes("*")
      ? current.context.parser.columns.map((column) => column.name)
      : current.analysis.projection;
    return projected.every((column) => {
      const capability = current.context.columnCapabilities[column];
      return capability?.projectable && capability.distinctable;
    }) && existingGroupByIsEditable() && existingAggregatesAreEditable();
  };

  const hasEditableCapabilities = (action: StructureAction): boolean => {
    const current = ready();
    if (!current) return false;
    if (action === "columns") {
      return current.context.parser.columns.some((column) =>
        current.context.columnCapabilities[column.name]?.projectable
      );
    }
    if (action === "filter") {
      return current.context.parser.columns.some((column) =>
        current.context.columnCapabilities[column.name]?.filterable &&
        operatorsForColumn(current.context, column.name).some((operator) =>
          isNullOperator(operator) || variablesFor(column.name).length > 0
        )
      );
    }
    if (action === "sort") {
      return current.context.parser.columns.some((column) =>
        current.context.columnCapabilities[column.name]?.sortable &&
        [
          "string",
          "boolean",
          "integer",
          "float",
          "date",
          "timestamp",
          "decimal",
        ].includes(column.type)
      );
    }
    if (action === "aggregate") {
      return aggregateFunctionsAvailable().length > 0 &&
        existingAggregatesAreEditable() && existingGroupByIsEditable();
    }
    if (action === "group_by") {
      const projection = current.analysis.projection;
      const projectionCanBeGrouped = !projection.includes("*") ||
        (current.analysis.group_by.length > 0 &&
          current.analysis.aggregates.length === 0);
      return projectionCanBeGrouped && groupableColumns().length > 0 &&
        existingGroupByIsEditable() && existingAggregatesAreEditable();
    }
    return distinctProjectionIsEditable();
  };

  useDialogFocus(() => activeAction() !== null, {
    dialog: () => dialog,
    inert: false,
    onClose: () => setActiveAction(null),
  });

  const openAction = (action: StructureAction) => {
    const current = ready();
    if (!current || actionDisabled(action)) return;
    setError(null);
    setProposal(undefined);
    setProposalContextIdentity(undefined);
    const allColumns = current.context.parser.columns.map((column) =>
      column.name
    );
    const projection = current.analysis.projection;
    setSelectedColumns(
      projection.includes("*")
        ? allColumns.filter((column) =>
          current.context.columnCapabilities[column]?.projectable
        )
        : [...projection],
    );
    setFilters(current.analysis.filters.map((filter) => ({
      column: filter.column,
      operator: filter.operator,
      parameter: filter.parameter ?? "",
    })));
    setSorts(current.analysis.order_by.map((sort) => ({ ...sort })));
    setAggregates(current.analysis.aggregates.map((aggregate) => ({
      ...aggregate,
      ...(aggregate.alias ? { alias: { ...aggregate.alias } } : {}),
    })));
    setGroupByColumns([...current.analysis.group_by]);
    setSelectDistinct(current.analysis.distinct);
    setActiveAction(action);
  };

  const variablesFor = (columnName: string) => {
    const current = ready();
    const column = current?.context.parser.columns.find((item) =>
      item.name === columnName
    );
    const type = column && variableTypeForColumn(column.type);
    return type
      ? (current?.context.parser.variables ?? []).filter((variable) =>
        variable.type === type
      )
      : [];
  };

  const filterableColumns = () =>
    (ready()?.context.parser.columns ?? []).filter((column) =>
      ready()?.context.columnCapabilities[column.name]?.filterable
    );

  const addFilter = () => {
    const candidate = filterableColumns().flatMap((column) => {
      const operators = operatorsForColumn(ready()?.context, column.name);
      const operator = operators.find((item) =>
        !isNullOperator(item) && variablesFor(column.name).length > 0
      ) ?? operators.find(isNullOperator);
      return operator ? [{ column, operator }] : [];
    })[0];
    if (!candidate) return;
    const compatibleVariables = variablesFor(candidate.column.name);
    setFilters((items) => [...items, {
      column: candidate.column.name,
      operator: candidate.operator,
      parameter: isNullOperator(candidate.operator)
        ? ""
        : compatibleVariables[0]?.name ?? "",
    }]);
  };

  const updateFilter = (index: number, update: Partial<FilterDraft>) => {
    setFilters((items) =>
      items.map((item, itemIndex) => {
        if (index !== itemIndex) return item;
        const next = { ...item, ...update };
        const column = ready()?.context.parser.columns.find((entry) =>
          entry.name === next.column
        );
        const supportedOperators = column
          ? operatorsForColumn(ready()?.context, column.name)
          : [];
        if (!supportedOperators.includes(next.operator)) {
          next.operator = supportedOperators[0] ?? "is_null";
          next.parameter = "";
        }
        if (!isNullOperator(next.operator)) {
          const compatibleVariables = variablesFor(next.column);
          if (
            !compatibleVariables.some((variable) =>
              variable.name === next.parameter
            )
          ) next.parameter = compatibleVariables[0]?.name ?? "";
        } else {
          next.parameter = "";
        }
        return next;
      })
    );
  };

  const filterRowsValid = () =>
    filters().every((filter) =>
      Boolean(ready()?.context.columnCapabilities[filter.column]?.filterable) &&
      operatorsForColumn(ready()?.context, filter.column).includes(
        filter.operator,
      ) &&
      (isNullOperator(filter.operator) ||
        variablesFor(filter.column).some((variable) =>
          variable.name === filter.parameter
        ))
    );

  const sortableColumns = () =>
    (ready()?.context.parser.columns ?? []).filter((column) =>
      ready()?.context.columnCapabilities[column.name]?.sortable &&
      ["string", "boolean", "integer", "float", "date", "timestamp", "decimal"]
        .includes(column.type)
    );

  const sortRowsValid = () =>
    sorts().every((sort) =>
      sortableColumns().some((column) => column.name === sort.column)
    );

  const addSort = () => {
    const available = sortableColumns().find((column) =>
      !sorts().some((sort) => sort.column === column.name)
    );
    if (!available) return;
    setSorts((items) => [...items, {
      column: available.name,
      direction: "asc",
    }]);
  };

  const addAggregate = () => {
    setAggregates((items) => [...items, {
      function: "count",
      distinct: false,
    }]);
  };

  const updateAggregateFunction = (
    index: number,
    fn: SqlStructureAggregateFunction,
  ) => {
    setAggregates((items) =>
      items.map((item, itemIndex) => {
        if (index !== itemIndex) return item;
        let column = item.column;
        let distinct = item.distinct;
        const validColumns = aggregateColumnsFor(fn, distinct);
        if (
          (!column && fn !== "count") ||
          (column &&
            !validColumns.some((candidate) => candidate.name === column))
        ) {
          const fallback = validColumns[0] ??
            (distinct ? aggregateColumnsFor(fn)[0] : undefined);
          column = fallback?.name;
          if (distinct && !fallback?.distinctable) distinct = false;
        }
        if (fn === "count" && !column) distinct = false;
        const next: SqlStructureAggregate = {
          ...item,
          function: fn,
          distinct,
        };
        if (column) next.column = column;
        else delete next.column;
        return next;
      })
    );
  };

  const updateAggregateColumn = (index: number, columnName: string) => {
    const column = columnName || undefined;
    setAggregates((items) =>
      items.map((item, itemIndex) => {
        if (index !== itemIndex) return item;
        const next: SqlStructureAggregate = {
          ...item,
          distinct: item.distinct && Boolean(
            column && columnCapability(column)?.distinctable,
          ),
        };
        if (column) next.column = column;
        else delete next.column;
        return next;
      })
    );
  };

  const updateAggregateDistinct = (index: number, distinct: boolean) => {
    setAggregates((items) =>
      items.map((item, itemIndex) => {
        if (index !== itemIndex) return item;
        const capability = item.column
          ? columnCapability(item.column)
          : undefined;
        return {
          ...item,
          distinct: Boolean(
            distinct && item.column && capability?.distinctable,
          ),
        };
      })
    );
  };

  const toggleGroupByColumn = (column: string, selected: boolean) => {
    setGroupByColumns((current) =>
      selected
        ? current.includes(column) ? current : [...current, column]
        : current.filter((item) => item !== column)
    );
  };

  const applyOperation = async () => {
    const current = ready();
    const action = activeAction();
    if (!current || !action || actionDisabled(action) || busy()) return;
    const operation: SqlStructureOperation = action === "columns"
      ? { kind: "set_columns", columns: selectedColumns() }
      : action === "filter"
      ? {
        kind: "set_filters" as const,
        filters: filters().map((filter): SqlStructureFilter => ({
          column: filter.column,
          operator: filter.operator,
          ...(isNullOperator(filter.operator)
            ? {}
            : { parameter: filter.parameter }),
        })),
      }
      : action === "sort"
      ? {
        kind: "set_sort",
        order_by: sorts().map((sort): SqlStructureSort => ({ ...sort })),
      }
      : action === "aggregate"
      ? {
        kind: "set_aggregates",
        aggregates: aggregates().map((aggregate) => ({ ...aggregate })),
      }
      : action === "group_by"
      ? { kind: "set_group_by", columns: groupByColumns() }
      : { kind: "set_distinct", distinct: selectDistinct() };
    if (
      action === "columns" && selectedColumns().length === 0 ||
      action === "filter" && !filterRowsValid() ||
      action === "sort" && !sortRowsValid() ||
      action === "aggregate" && !aggregateOperationValid() ||
      action === "group_by" && !groupByRowsValid() ||
      action === "distinct" && !distinctProjectionIsEditable()
    ) return;

    const baseSql = props.sql();
    const parserSql = normalizeSqlVariables(baseSql).sql;
    const contextIdentity = current.identity;
    const generation = ++proposalGeneration;
    setBusy(true);
    setError(null);
    try {
      const result = await proposeSqlStructureOperation(
        parserSql,
        current.context.parser,
        operation,
      );
      if (
        generation !== proposalGeneration || props.sql() !== baseSql ||
        ready()?.identity !== contextIdentity
      ) return;
      if (
        result.status !== "supported" || !result.requires_confirmation ||
        result.base_sql !== parserSql || result.proposed_sql === undefined
      ) {
        setError(unavailableReason());
        return;
      }
      setProposal({ ...result, base_sql: baseSql });
      setProposalContextIdentity(contextIdentity);
    } catch {
      if (
        generation === proposalGeneration && props.sql() === baseSql &&
        ready()?.identity === contextIdentity
      ) {
        setError(t("sqlPage.structureUnavailable"));
      }
    } finally {
      if (generation === proposalGeneration) setBusy(false);
    }
  };

  const confirmProposal = () => {
    const current = proposal();
    if (
      !current || current.proposed_sql === undefined ||
      props.sql() !== current.base_sql || props.disabled?.() ||
      !props.onSqlChange || ready()?.identity !== proposalContextIdentity()
    ) {
      setProposal(undefined);
      setActiveAction(null);
      return;
    }
    props.onSqlChange(current.proposed_sql);
    setProposal(undefined);
    setProposalContextIdentity(undefined);
    setActiveAction(null);
  };

  const close = () => {
    proposalGeneration += 1;
    setBusy(false);
    setError(null);
    setProposal(undefined);
    setProposalContextIdentity(undefined);
    setActiveAction(null);
  };

  const actionTitle = () => {
    const action = activeAction();
    return proposal()
      ? t("sqlPage.structure.proposal")
      : action
      ? actionLabel(action)
      : "";
  };

  const operationBlockedReason = () => {
    if (activeAction() === "aggregate" && !aggregateOperationValid()) {
      return t("sqlPage.structure.addAggregateFirst");
    }
    if (activeAction() === "group_by" && !groupByRowsValid()) {
      return t("sqlPage.structure.groupProjectedColumns");
    }
    return undefined;
  };

  createEffect(() => {
    const actions = ([
      "columns",
      "filter",
      "sort",
      "aggregate",
      "group_by",
      "distinct",
    ] as const).map(
      (action): SQLWorkspaceAction => {
        const label = actionLabel(action);
        const reason = actionReason(action);
        const disabled = actionDisabled(action);
        return {
          kind: action,
          icon: action,
          label,
          disabled,
          disabledReason: disabled ? reason : undefined,
          active: activeAction() === action,
          onActivate: () => openAction(action),
        };
      },
    );
    props.onActionsChange(actions);
  });

  return (
    <>
      <Show when={activeAction() !== null}>
        <div
          class="ui-backdrop sql-structure-backdrop"
          onClick={(event) => {
            if (event.target === event.currentTarget) close();
          }}
        >
          <div
            ref={(element) => dialog = element}
            class="ui-dialog sql-structure-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby={dialogTitleId}
            tabIndex={-1}
            onKeyDown={(event) => handleDialogKeyDown(event, dialog, close)}
          >
            <header class="ui-dialog-header">
              <h2 id={dialogTitleId} class="ui-dialog-title">
                {actionTitle()}
              </h2>
              <IconButton
                icon="close"
                label={t("common.close")}
                onClick={close}
              />
            </header>

            <Show
              when={proposal()}
              fallback={
                <div class="sql-structure-dialog-content">
                  <Show when={activeAction() === "columns"}>
                    <fieldset class="sql-structure-column-list">
                      <legend class="ui-sr-only">
                        {t("sqlPage.structure.columns")}
                      </legend>
                      <For each={ready()?.context.parser.columns ?? []}>
                        {(column) => (
                          <label class="sql-structure-option">
                            <input
                              type="checkbox"
                              checked={selectedColumns().includes(column.name)}
                              disabled={!ready()?.context.columnCapabilities[
                                column.name
                              ]?.projectable &&
                                !selectedColumns().includes(column.name)}
                              onChange={(event) => {
                                const current = selectedColumns();
                                setSelectedColumns(
                                  event.currentTarget.checked
                                    ? [...current, column.name]
                                    : current.filter((name) =>
                                      name !== column.name
                                    ),
                                );
                              }}
                            />
                            <span>{column.name}</span>
                          </label>
                        )}
                      </For>
                    </fieldset>
                  </Show>

                  <Show when={activeAction() === "filter"}>
                    <div class="sql-structure-filter-list">
                      <For each={filters()}>
                        {(filter, index) => {
                          const context = () => ready()?.context;
                          const operators = () =>
                            operatorsForColumn(
                              context(),
                              filter.column,
                            );
                          const variables = () => variablesFor(filter.column);
                          const allowedColumns = () => {
                            const allowed = filterableColumns().map((item) =>
                              item.name
                            );
                            return allowed.includes(filter.column)
                              ? allowed
                              : [filter.column, ...allowed];
                          };
                          const rowEditable = () =>
                            Boolean(
                              context()?.columnCapabilities[filter.column]
                                ?.filterable && operators().length > 0,
                            );
                          return (
                            <div class="sql-structure-filter-row">
                              <label class="ui-field">
                                <span>{t("sqlPage.structure.column")}</span>
                                <select
                                  class="ui-input"
                                  value={filter.column}
                                  disabled={!rowEditable()}
                                  onChange={(event) =>
                                    updateFilter(index(), {
                                      column: event.currentTarget.value,
                                    })}
                                >
                                  <For each={allowedColumns()}>
                                    {(name) => (
                                      <option value={name}>{name}</option>
                                    )}
                                  </For>
                                </select>
                              </label>
                              <label class="ui-field">
                                <span>{t("sqlPage.structure.operator")}</span>
                                <select
                                  class="ui-input"
                                  value={filter.operator}
                                  disabled={!rowEditable()}
                                  onChange={(event) =>
                                    updateFilter(index(), {
                                      operator: event.currentTarget
                                        .value as SqlStructureFilterOperator,
                                    })}
                                >
                                  <For each={operators()}>
                                    {(operator) => (
                                      <option value={operator}>
                                        {t(operatorKey(operator))}
                                      </option>
                                    )}
                                  </For>
                                  <Show
                                    when={!operators().includes(
                                      filter.operator,
                                    )}
                                  >
                                    <option value={filter.operator}>
                                      {t(
                                        "sqlPage.structure.unsupportedOperator",
                                      )}
                                    </option>
                                  </Show>
                                </select>
                              </label>
                              <Show when={!isNullOperator(filter.operator)}>
                                <label class="ui-field">
                                  <span>{t("sqlPage.structure.variable")}</span>
                                  <select
                                    class="ui-input"
                                    value={filter.parameter}
                                    disabled={!rowEditable()}
                                    onChange={(event) =>
                                      updateFilter(index(), {
                                        parameter: event.currentTarget.value,
                                      })}
                                  >
                                    <For each={variables()}>
                                      {(variable) => (
                                        <option value={variable.name}>
                                          {variable.name}
                                        </option>
                                      )}
                                    </For>
                                    <Show
                                      when={!variables().some((variable) =>
                                        variable.name === filter.parameter
                                      )}
                                    >
                                      <option value={filter.parameter}>
                                        {filter.parameter}
                                      </option>
                                    </Show>
                                  </select>
                                </label>
                              </Show>
                              <IconButton
                                icon="trash"
                                label={t("sqlPage.structure.removeFilter")}
                                onClick={() =>
                                  setFilters((items) =>
                                    items.filter((_, i) => i !== index())
                                  )}
                              />
                            </div>
                          );
                        }}
                      </For>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        onClick={addFilter}
                        disabled={!filterableColumns().some((column) =>
                          operatorsForColumn(ready()?.context, column.name)
                            .some((operator) => isNullOperator(operator) ||
                              variablesFor(column.name).length > 0
                            )
                        )}
                      >
                        {t("sqlPage.structure.addFilter")}
                      </button>
                    </div>
                  </Show>

                  <Show when={activeAction() === "sort"}>
                    <div class="sql-structure-sort-list">
                      <For each={sorts()}>
                        {(sort, index) => (
                          <div class="sql-structure-sort-row">
                            <label class="ui-field">
                              <span>{t("sqlPage.structure.column")}</span>
                              <select
                                class="ui-input"
                                value={sort.column}
                                onChange={(event) =>
                                  setSorts((items) =>
                                    items.map((item, i) =>
                                      i === index()
                                        ? {
                                          ...item,
                                          column: event.currentTarget.value,
                                        }
                                        : item
                                    )
                                  )}
                              >
                                <For
                                  each={[
                                    ...new Set([
                                      sort.column,
                                      ...sortableColumns().map((column) =>
                                        column.name
                                      ),
                                    ]),
                                  ]}
                                >
                                  {(name) => (
                                    <option value={name}>{name}</option>
                                  )}
                                </For>
                              </select>
                            </label>
                            <label class="ui-field">
                              <span>{t("sqlPage.structure.direction")}</span>
                              <select
                                class="ui-input"
                                value={sort.direction}
                                onChange={(event) =>
                                  setSorts((items) =>
                                    items.map((item, i) =>
                                      i === index()
                                        ? {
                                          ...item,
                                          direction: event.currentTarget
                                            .value as "asc" | "desc",
                                        }
                                        : item
                                    )
                                  )}
                              >
                                <option value="asc">
                                  {t("sqlPage.structure.ascending")}
                                </option>
                                <option value="desc">
                                  {t("sqlPage.structure.descending")}
                                </option>
                              </select>
                            </label>
                            <IconButton
                              icon="trash"
                              label={t("sqlPage.structure.removeSort")}
                              onClick={() =>
                                setSorts((items) =>
                                  items.filter((_, i) => i !== index())
                                )}
                            />
                          </div>
                        )}
                      </For>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        onClick={addSort}
                        disabled={!sortableColumns().some((column) =>
                          !sorts().some((sort) => sort.column === column.name)
                        )}
                      >
                        {t("sqlPage.structure.addSort")}
                      </button>
                    </div>
                  </Show>

                  <Show when={activeAction() === "aggregate"}>
                    <div class="sql-structure-aggregate-list">
                      <For each={aggregates()}>
                        {(aggregate, index) => {
                          const functions = () =>
                            aggregateFunctionsAvailable().includes(
                                aggregate.function,
                              )
                              ? aggregateFunctionsAvailable()
                              : [
                                aggregate.function,
                                ...aggregateFunctionsAvailable(),
                              ];
                          const columns = () => {
                            const allowed = aggregateColumnsFor(
                              aggregate.function,
                              aggregate.distinct,
                            ).map((column) => column.name);
                            return aggregate.column &&
                                !allowed.includes(aggregate.column)
                              ? [aggregate.column, ...allowed]
                              : allowed;
                          };
                          const distinctable = () =>
                            Boolean(
                              aggregate.column &&
                                columnCapability(aggregate.column)
                                  ?.distinctable,
                            );
                          return (
                            <div
                              class={`sql-structure-aggregate-row${
                                aggregate.column
                                  ? ""
                                  : " sql-structure-aggregate-row--count-all"
                              }`}
                            >
                              <label class="ui-field">
                                <span>{t("sqlPage.structure.function")}</span>
                                <select
                                  class="ui-input"
                                  value={aggregate.function}
                                  onChange={(event) =>
                                    updateAggregateFunction(
                                      index(),
                                      event.currentTarget
                                        .value as SqlStructureAggregateFunction,
                                    )}
                                >
                                  <For each={functions()}>
                                    {(fn) => (
                                      <option value={fn}>
                                        {t(`sqlPage.structure.aggregate.${fn}`)}
                                      </option>
                                    )}
                                  </For>
                                </select>
                              </label>
                              <Show
                                when={aggregate.function !== "count" ||
                                  aggregate.column}
                              >
                                <label class="ui-field">
                                  <span>{t("sqlPage.structure.column")}</span>
                                  <select
                                    class="ui-input"
                                    value={aggregate.column ?? ""}
                                    disabled={columns().length === 0}
                                    onChange={(event) =>
                                      updateAggregateColumn(
                                        index(),
                                        event.currentTarget.value,
                                      )}
                                  >
                                    <Show when={aggregate.function === "count"}>
                                      <option value="">
                                        {t("sqlPage.structure.allRows")}
                                      </option>
                                    </Show>
                                    <For each={columns()}>
                                      {(name) => (
                                        <option value={name}>{name}</option>
                                      )}
                                    </For>
                                  </select>
                                </label>
                              </Show>
                              <Show when={aggregate.column}>
                                <label class="sql-structure-option sql-structure-aggregate-distinct">
                                  <input
                                    type="checkbox"
                                    checked={aggregate.distinct}
                                    disabled={!distinctable()}
                                    onChange={(event) =>
                                      updateAggregateDistinct(
                                        index(),
                                        event.currentTarget.checked,
                                      )}
                                  />
                                  <span>
                                    {t("sqlPage.structure.distinctArgument")}
                                  </span>
                                </label>
                              </Show>
                              <IconButton
                                icon="trash"
                                label={t("sqlPage.structure.removeAggregate")}
                                onClick={() =>
                                  setAggregates((items) =>
                                    items.filter((_, i) => i !== index())
                                  )}
                              />
                            </div>
                          );
                        }}
                      </For>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        onClick={addAggregate}
                      >
                        {t("sqlPage.structure.addAggregate")}
                      </button>
                    </div>
                  </Show>

                  <Show when={activeAction() === "group_by"}>
                    <fieldset class="sql-structure-column-list">
                      <legend class="ui-sr-only">
                        {t("sqlPage.structure.groupBy")}
                      </legend>
                      <For
                        each={[
                          ...new Set([
                            ...groupByColumns(),
                            ...groupableColumns().map((column) => column.name),
                          ]),
                        ]}
                      >
                        {(name) => (
                          <label class="sql-structure-option">
                            <input
                              type="checkbox"
                              checked={groupByColumns().includes(name)}
                              disabled={!groupableColumns().some((column) =>
                                column.name === name
                              )}
                              onChange={(event) =>
                                toggleGroupByColumn(
                                  name,
                                  event.currentTarget.checked,
                                )}
                            />
                            <span>{name}</span>
                          </label>
                        )}
                      </For>
                    </fieldset>
                  </Show>

                  <Show when={activeAction() === "distinct"}>
                    <label class="sql-structure-option">
                      <input
                        type="checkbox"
                        checked={selectDistinct()}
                        onChange={(event) =>
                          setSelectDistinct(event.currentTarget.checked)}
                      />
                      <span>{t("sqlPage.structure.selectDistinct")}</span>
                    </label>
                  </Show>
                </div>
              }
            >
              {(currentProposal) => (
                <div class="sql-structure-diff">
                  <section>
                    <h3>{t("sqlPage.structure.currentSql")}</h3>
                    <pre>{currentProposal().base_sql}</pre>
                  </section>
                  <section>
                    <h3>{t("sqlPage.structure.proposedSql")}</h3>
                    <pre>{currentProposal().proposed_sql}</pre>
                  </section>
                  <Show
                    when={currentProposal().added_stable_tie_breakers.length >
                      0}
                  >
                    <span class="ui-muted">
                      {t("sqlPage.structure.stableTieBreakers", {
                        columns: currentProposal().added_stable_tie_breakers
                          .join(", "),
                      })}
                    </span>
                  </Show>
                </div>
              )}
            </Show>

            <Show when={error()}>
              <p class="ui-text-danger" role="alert">{error()}</p>
            </Show>
            <footer class="ui-dialog-actions">
              <button
                type="button"
                class="ui-button ui-button-secondary"
                onClick={close}
              >
                {t("common.cancel")}
              </button>
              <Show
                when={proposal()}
                fallback={
                  <button
                    type="button"
                    class="ui-button ui-button-primary"
                    onClick={() => void applyOperation()}
                    disabled={busy() || (
                      activeAction() === "columns" &&
                      selectedColumns().length === 0
                    ) || (activeAction() === "filter" && !filterRowsValid()) ||
                      (activeAction() === "sort" && !sortRowsValid()) ||
                      (activeAction() === "aggregate" &&
                        !aggregateOperationValid()) ||
                      (activeAction() === "group_by" && !groupByRowsValid()) ||
                      (activeAction() === "distinct" &&
                        !distinctProjectionIsEditable())}
                    aria-label={operationBlockedReason()
                      ? `${
                        t("sqlPage.structure.previewChange")
                      }: ${operationBlockedReason()}`
                      : undefined}
                    title={operationBlockedReason()}
                    aria-busy={busy() || undefined}
                  >
                    {busy()
                      ? t("sqlPage.structure.preparing")
                      : t("sqlPage.structure.previewChange")}
                  </button>
                }
              >
                <button
                  type="button"
                  class="ui-button ui-button-primary"
                  onClick={confirmProposal}
                  disabled={busy() || props.sql() !== proposal()?.base_sql ||
                    ready()?.identity !== proposalContextIdentity()}
                >
                  {t("sqlPage.structure.apply")}
                </button>
              </Show>
            </footer>
          </div>
        </div>
      </Show>
    </>
  );
}
