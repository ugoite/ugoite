import {
  createEffect,
  createSignal,
  createUniqueId,
  For,
  Index,
  Show,
  untrack,
} from "solid-js";
import { IconButton } from "~/components/IconButton";
import { UiIcon } from "~/components/UiIcon";
import { handleDialogKeyDown, useDialogFocus } from "~/components/dialog-focus";
import { t } from "~/lib/i18n";
import { normalizeSqlVariables } from "~/lib/sql";
import { SqlDocumentStructureEditor } from "~/components/SqlDocumentStructureEditor";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import {
  analyzeSqlStructure,
  applySqlDocumentEditTransaction,
  applySqlDocumentNodeEditTransaction,
  applySqlDocumentReplacementTransaction,
  parseSqlDocument,
  prepareSqlStructureParser,
  proposeSqlStructureOperation,
} from "~/lib/ugoite-client";
import type {
  SqlDocument,
  SqlDocumentNodeEdit,
  SqlStructureAggregate,
  SqlStructureAggregateFunction,
  SqlStructureAlias,
  SqlStructureAnalysis,
  SqlStructureFilter,
  SqlStructureFilterExpression,
  SqlStructureFilterOperator,
  SqlStructureLiteral,
  SqlStructureOperation,
  SqlStructureProjectionSlotInput,
  SqlStructureProposal,
  SqlStructureSort,
  SqlStructureVariableType,
} from "~/lib/ugoite-client";

type StructureAction =
  | "form"
  | "join"
  | "columns"
  | "filter"
  | "sort"
  | "aggregate"
  | "group_by"
  | "distinct";
type StructureEditorMode = "join" | "row_limit";
type FilterPredicateDraft = {
  kind: "predicate";
  id: string;
  column: string;
  qualifier?: SqlStructureAlias;
  operator: SqlStructureFilterOperator;
  parameter: string;
  createdVariable?: SqlStructureVariableType;
};
type SortDraft = {
  column: string;
  qualifier?: SqlStructureAlias;
  projection_alias?: SqlStructureAlias;
  direction: "asc" | "desc";
};
type ColumnChoice = {
  column: string;
  qualifier?: SqlStructureAlias;
  projection_alias?: SqlStructureAlias;
  value: string;
  label: string;
};
type ProjectionSlotDraft = {
  /** Stable Solid key, separate from the server slot identity. */
  key: string;
  id?: string;
  column: string;
  alias: SqlStructureAlias;
};

type FilterDraft =
  | FilterPredicateDraft
  | { kind: "and" | "or"; id: string; expressions: FilterDraft[] }
  | { kind: "not"; id: string; expression: FilterDraft };
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
type SqlHistoryEntry = {
  sql: string;
  variablesAdded: readonly { name: string; type: SqlStructureVariableType }[];
};

interface SqlStructureToolbarProps {
  sql: () => string;
  /** Owner identity for transient undo/redo history in multi-document hosts. */
  historyScopeKey?: () => string;
  contexts: () => readonly SqlStructureAdapterContext[];
  onSqlChange?: (sql: string) => boolean | void;
  onVariablesAdded?: (
    variables: readonly { name: string; type: SqlStructureVariableType }[],
  ) => boolean | void;
  /** Remove declarations created by the matching structure edit during undo. */
  onVariablesRemoved?: (names: readonly string[]) => boolean | void;
  /** Names already owned by the host, including variables not yet used in SQL. */
  reservedVariableNames?: () => readonly string[];
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

const filterExpressionToDraft = (
  expression: SqlStructureFilterExpression,
): FilterDraft => {
  const id = createUniqueId();
  switch (expression.kind) {
    case "and":
    case "or":
      return {
        kind: expression.kind,
        id,
        expressions: expression.expressions.map(filterExpressionToDraft),
      };
    case "not":
      return {
        kind: "not",
        id,
        expression: filterExpressionToDraft(expression.expression),
      };
    case "predicate":
      return {
        kind: "predicate",
        id,
        column: expression.column,
        ...(expression.qualifier
          ? { qualifier: { ...expression.qualifier } }
          : {}),
        operator: expression.operator,
        parameter: expression.parameter ?? "",
      };
  }
};

const flatFiltersToDraft = (
  filters: readonly SqlStructureFilter[],
): FilterDraft | undefined => {
  const predicates = filters.map((filter): FilterPredicateDraft => ({
    kind: "predicate",
    id: createUniqueId(),
    column: filter.column,
    ...(filter.qualifier ? { qualifier: { ...filter.qualifier } } : {}),
    operator: filter.operator,
    parameter: filter.parameter ?? "",
  }));
  if (predicates.length === 0) return undefined;
  if (predicates.length === 1) return predicates[0];
  return { kind: "and", id: createUniqueId(), expressions: predicates };
};

const filterDraftToExpression = (
  draft: FilterDraft,
): SqlStructureFilterExpression => {
  switch (draft.kind) {
    case "and":
    case "or":
      return {
        kind: draft.kind,
        expressions: draft.expressions.map(filterDraftToExpression),
      };
    case "not":
      return {
        kind: "not",
        expression: filterDraftToExpression(draft.expression),
      };
    case "predicate":
      return {
        kind: "predicate",
        column: draft.column,
        ...(draft.qualifier ? { qualifier: { ...draft.qualifier } } : {}),
        operator: draft.operator,
        ...(isNullOperator(draft.operator)
          ? {}
          : { parameter: draft.parameter }),
      };
  }
};

const filterDraftToFlatFilters = (
  draft?: FilterDraft,
): SqlStructureFilter[] | undefined => {
  if (!draft) return [];
  switch (draft.kind) {
    case "predicate":
      return [{
        column: draft.column,
        ...(draft.qualifier ? { qualifier: { ...draft.qualifier } } : {}),
        operator: draft.operator,
        ...(isNullOperator(draft.operator)
          ? {}
          : { parameter: draft.parameter }),
      }];
    case "and": {
      const filters: SqlStructureFilter[] = [];
      for (const child of draft.expressions) {
        const childFilters = filterDraftToFlatFilters(child);
        if (!childFilters) return undefined;
        filters.push(...childFilters);
      }
      return filters;
    }
    case "or":
    case "not":
      return undefined;
  }
};

const filterDraftLeaves = (draft?: FilterDraft): FilterPredicateDraft[] => {
  if (!draft) return [];
  switch (draft.kind) {
    case "predicate":
      return [draft];
    case "not":
      return filterDraftLeaves(draft.expression);
    case "and":
    case "or":
      return draft.expressions.flatMap(filterDraftLeaves);
  }
};

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
  const [sqlDocument, setSqlDocument] = createSignal<SqlDocument>();
  const [sqlDocumentLoading, setSqlDocumentLoading] = createSignal(false);
  const [undoSql, setUndoSql] = createSignal<SqlHistoryEntry[]>([]);
  const [redoSql, setRedoSql] = createSignal<SqlHistoryEntry[]>([]);
  const [activeAction, setActiveAction] = createSignal<StructureAction | null>(
    null,
  );
  const [literalProjection, setLiteralProjection] = createSignal<
    SqlStructureLiteral[]
  >([]);
  const [projectionSlots, setProjectionSlots] = createSignal<
    ProjectionSlotDraft[]
  >([]);
  const [filters, setFilters] = createSignal<FilterDraft>();
  const [sorts, setSorts] = createSignal<SortDraft[]>([]);
  const [aggregates, setAggregates] = createSignal<SqlStructureAggregate[]>([]);
  const [groupByColumns, setGroupByColumns] = createSignal<string[]>([]);
  const [selectDistinct, setSelectDistinct] = createSignal(false);
  const [structureEditorMode, setStructureEditorMode] =
    createSignal<StructureEditorMode>("join");
  const [rowLimitDraft, setRowLimitDraft] = createSignal("");
  const [rowOffsetDraft, setRowOffsetDraft] = createSignal("");
  const [rowLimitAllDraft, setRowLimitAllDraft] = createSignal(false);
  const [rowLimitDraftIdentity, setRowLimitDraftIdentity] = createSignal<
    string
  >();
  const [joinLeftColumn, setJoinLeftColumn] = createSignal("");
  const [joinRightColumn, setJoinRightColumn] = createSignal("");
  const [proposal, setProposal] = createSignal<SqlStructureProposal>();
  const [proposalVariables, setProposalVariables] = createSignal<
    { name: string; type: SqlStructureVariableType }[]
  >([]);
  const [proposalContextIdentity, setProposalContextIdentity] = createSignal<
    string
  >();
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  let analysisGeneration = 0;
  let proposalGeneration = 0;
  let documentGeneration = 0;
  let dialog: HTMLDivElement | undefined;
  let observedHistorySql: string | undefined;
  let observedHistoryScopeKey: string | undefined;
  let expectedHistorySql: string | undefined;
  let historyCurrentSql: string | undefined;

  const clearSqlHistory = (currentSql: string) => {
    setUndoSql([]);
    setRedoSql([]);
    historyCurrentSql = currentSql;
    expectedHistorySql = undefined;
  };

  const commitStructuredSql = (
    baseSql: string,
    nextSql: string,
    variablesAdded: readonly {
      name: string;
      type: SqlStructureVariableType;
    }[] = [],
  ) => {
    if (baseSql === nextSql || props.sql() !== baseSql || !props.onSqlChange) {
      return;
    }
    expectedHistorySql = nextSql;
    historyCurrentSql = nextSql;
    const accepted = props.onSqlChange(nextSql);
    if (accepted === false) {
      expectedHistorySql = baseSql;
      historyCurrentSql = baseSql;
      if (variablesAdded.length > 0) {
        props.onVariablesRemoved?.(variablesAdded.map(({ name }) => name));
      }
      return false;
    }
    setUndoSql((history) =>
      [...history, { sql: baseSql, variablesAdded }].slice(-20)
    );
    setRedoSql([]);
    return true;
  };

  const undoStructuredSql = () => {
    const currentSql = props.sql();
    const history = undoSql();
    const entry = history[history.length - 1];
    if (
      props.disabled?.() || !props.onSqlChange || !entry ||
      historyCurrentSql !== currentSql
    ) {
      if (historyCurrentSql !== currentSql) clearSqlHistory(currentSql);
      return;
    }
    const variableNames = entry.variablesAdded.map(({ name }) => name);
    const previousSql = entry.sql;
    expectedHistorySql = previousSql;
    historyCurrentSql = previousSql;
    if (props.onSqlChange(previousSql) === false) {
      expectedHistorySql = currentSql;
      historyCurrentSql = currentSql;
      return;
    }
    if (
      variableNames.length > 0 &&
      (!props.onVariablesRemoved ||
        props.onVariablesRemoved(variableNames) === false)
    ) {
      expectedHistorySql = currentSql;
      historyCurrentSql = currentSql;
      props.onSqlChange(currentSql);
      return;
    }
    setUndoSql(history.slice(0, -1));
    setRedoSql((redo) =>
      [...redo, {
        sql: currentSql,
        variablesAdded: entry.variablesAdded,
      }].slice(-20)
    );
  };

  const redoStructuredSql = () => {
    const currentSql = props.sql();
    const history = redoSql();
    const entry = history[history.length - 1];
    if (
      props.disabled?.() || !props.onSqlChange || !entry ||
      historyCurrentSql !== currentSql
    ) {
      if (historyCurrentSql !== currentSql) clearSqlHistory(currentSql);
      return;
    }
    if (
      entry.variablesAdded.length > 0 &&
      (!props.onVariablesAdded ||
        props.onVariablesAdded(entry.variablesAdded) === false)
    ) return;
    const nextSql = entry.sql;
    expectedHistorySql = nextSql;
    historyCurrentSql = nextSql;
    if (props.onSqlChange(nextSql) === false) {
      expectedHistorySql = currentSql;
      historyCurrentSql = currentSql;
      if (entry.variablesAdded.length > 0) {
        props.onVariablesRemoved?.(
          entry.variablesAdded.map(({ name }) => name),
        );
      }
      return;
    }
    setRedoSql(history.slice(0, -1));
    setUndoSql((undo) =>
      [...undo, {
        sql: currentSql,
        variablesAdded: entry.variablesAdded,
      }].slice(-20)
    );
  };

  createEffect(() => {
    const sourceSql = props.sql();
    const generation = ++documentGeneration;
    setSqlDocument(undefined);
    setSqlDocumentLoading(true);
    void parseSqlDocument(sourceSql).then((document) => {
      if (generation !== documentGeneration) return;
      setSqlDocument(document);
    }).catch(() => {
      if (generation !== documentGeneration) return;
      setSqlDocument(undefined);
    }).finally(() => {
      if (generation === documentGeneration) setSqlDocumentLoading(false);
    });
  });

  createEffect(() => {
    const historyScopeKey = props.historyScopeKey?.() ?? "";
    const sourceSql = props.sql();
    const historyScopeChanged = observedHistoryScopeKey !== undefined &&
      historyScopeKey !== observedHistoryScopeKey;
    observedHistoryScopeKey = historyScopeKey;
    const historyUpdate = !historyScopeChanged &&
      expectedHistorySql !== undefined &&
      expectedHistorySql === sourceSql;
    if (observedHistorySql === undefined) {
      observedHistorySql = sourceSql;
      historyCurrentSql = sourceSql;
    } else if (historyScopeChanged) {
      clearSqlHistory(sourceSql);
    } else if (sourceSql !== observedHistorySql && !historyUpdate) {
      clearSqlHistory(sourceSql);
    }
    observedHistorySql = sourceSql;
    if (historyUpdate) {
      historyCurrentSql = sourceSql;
      expectedHistorySql = undefined;
    }
    const sql = normalizeSqlVariables(sourceSql).sql;
    const contexts = [...props.contexts()];
    const contextsKey = JSON.stringify(contexts);
    const inputIdentity = JSON.stringify({ sourceSql, sql, contextsKey });
    const generation = ++analysisGeneration;
    proposalGeneration += 1;
    setBusy(false);
    setProposal(undefined);
    setProposalVariables([]);
    setProposalContextIdentity(undefined);
    if (untrack(activeAction) !== "join" && !historyUpdate) {
      setActiveAction(null);
    }
    setLiteralProjection([]);
    setProjectionSlots([]);
    setFilters(undefined);
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
    if (action === "form") return undefined;
    const current = ready()?.analysis;
    if (action === "filter") {
      // Older WASM adapters expose only the flat capability. Prefer the
      // boolean-tree capability only when it is enabled; a disabled tree
      // capability must not hide a usable flat conjunction editor.
      const expression = current?.capabilities.filter_expression;
      return expression?.enabled ? expression : current?.capabilities.filter;
    }
    return current?.capabilities[action];
  };

  const supportsFilterExpression = () =>
    ready()?.analysis.capabilities.filter_expression?.enabled === true;

  const formOptions = () => {
    const seen = new Set<string>();
    return props.contexts().flatMap((context) => {
      const relation = context.parser.relation;
      if (!context.relationSelectable || !relation || seen.has(relation)) {
        return [];
      }
      seen.add(relation);
      return [context];
    });
  };

  const actionDisabled = (action: StructureAction) => {
    if (props.disabled?.() || !props.onSqlChange) return true;
    if (action === "form") {
      return sqlDocumentLoading() || analysisState().status === "loading" ||
        formOptions().length === 0;
    }
    if (action === "join") {
      return sqlDocumentLoading() || sqlDocument()?.status !== "parsed";
    }
    const documentFallbackReady = sqlDocument()?.status === "parsed" &&
      analysisState().status !== "loading";
    return !structuredEditorAvailable(action) && !documentFallbackReady;
  };

  const actionReason = (action: StructureAction) => {
    if (props.disabled?.() || !props.onSqlChange) {
      return t("sqlPage.structureReadOnly");
    }
    if (action === "form") {
      if (sqlDocumentLoading()) return t("sqlPage.structureAnalyzing");
      if (sqlDocument()?.status === "parsed") {
        if (analysisState().status === "loading") {
          return t("sqlPage.structureAnalyzing");
        }
        return formOptions().length === 0
          ? t("sqlPage.structureUnavailable")
          : undefined;
      }
      const state = analysisState();
      if (state.status === "loading") return t("sqlPage.structureAnalyzing");
      if (state.status === "error") return t("sqlPage.structureUnavailable");
      if (state.status === "no_context") return t("sqlPage.structureNoForm");
      return formOptions().length === 0
        ? t("sqlPage.structureUnavailable")
        : undefined;
    }
    if (action === "join") {
      if (sqlDocumentLoading()) return t("sqlPage.structureAnalyzing");
      return sqlDocument()?.status === "parsed"
        ? undefined
        : unavailableReason();
    }
    if (
      sqlDocument()?.status === "parsed" &&
      !structuredEditorAvailable(action)
    ) {
      return analysisState().status === "loading"
        ? t("sqlPage.structureAnalyzing")
        : undefined;
    }
    const commonReason = analysisReason(analysisState());
    if (commonReason) return commonReason;
    const current = capability(action);
    return current && (!current.enabled || !hasEditableCapabilities(action))
      ? unavailableReason()
      : undefined;
  };

  const actionLabel = (action: StructureAction) =>
    t(`sqlPage.structure.${action}`);

  const columnCapability = (columnName: string) =>
    ready()?.context.columnCapabilities[columnName];

  const projectableColumns = () =>
    (ready()?.context.parser.columns ?? []).filter((column) =>
      columnCapability(column.name)?.projectable
    );

  const projectionSlotsValid = () =>
    projectionSlots().length > 0 &&
    projectionSlots().every((slot) =>
      Boolean(slot.column) &&
      Boolean(columnCapability(slot.column)?.projectable)
    );

  const updateProjectionSlot = (
    index: number,
    update: Partial<Pick<ProjectionSlotDraft, "column" | "alias">>,
  ) => {
    setProjectionSlots((slots) =>
      slots.map((slot, slotIndex) =>
        slotIndex === index ? { ...slot, ...update } : slot
      )
    );
  };

  const addProjectionSlot = () => {
    const column = projectableColumns()[0];
    if (!column || busy() || proposal()) return;
    setProjectionSlots((slots) => [...slots, {
      key: `projection-new-${createUniqueId()}`,
      column: column.name,
      alias: { name: "", quoted: false },
    }]);
  };

  const moveProjectionSlot = (index: number, direction: -1 | 1) => {
    if (busy() || proposal()) return;
    setProjectionSlots((slots) => {
      const target = index + direction;
      if (target < 0 || target >= slots.length) return slots;
      const next = [...slots];
      [next[index], next[target]] = [next[target], next[index]];
      return next;
    });
  };

  const activeJoin = () => ready()?.analysis.joins[0];

  const columnChoiceValue = (
    column: string,
    qualifier?: SqlStructureAlias,
  ) =>
    qualifier
      ? JSON.stringify([qualifier.name, qualifier.quoted, column])
      : column;

  const columnChoices = (
    columns: readonly { name: string }[],
  ): ColumnChoice[] => {
    const join = activeJoin();
    if (!join) {
      return columns.map((column) => ({
        column: column.name,
        value: column.name,
        label: column.name,
      }));
    }
    return [join.left_alias, join.right_alias].flatMap((qualifier) =>
      columns.map((column) => ({
        column: column.name,
        qualifier,
        value: columnChoiceValue(column.name, qualifier),
        label: `${qualifier.name}.${column.name}`,
      }))
    );
  };

  const selectedColumnChoice = (
    column: string,
    qualifier?: SqlStructureAlias,
  ) => columnChoiceValue(column, qualifier);

  const sortChoiceValue = (sort: SortDraft) =>
    sort.projection_alias
      ? JSON.stringify([
        "projection_alias",
        sort.projection_alias.name,
        sort.projection_alias.quoted,
      ])
      : selectedColumnChoice(sort.column, sort.qualifier);

  const sortChoices = (): ColumnChoice[] => {
    const current = ready();
    const columns = sortableColumns();
    const choices = columnChoices(columns);
    if (!current || activeJoin()) return choices;
    const sortable = new Set(columns.map((column) => column.name));
    const projectionChoices = current.analysis.projection_aliases.flatMap(
      (slot): ColumnChoice[] =>
        slot.alias && sortable.has(slot.column)
          ? [{
            column: slot.column,
            projection_alias: slot.alias,
            value: JSON.stringify([
              "projection_alias",
              slot.alias.name,
              slot.alias.quoted,
            ]),
            label: t("sqlPage.structure.sortProjectionAliasChoice", {
              alias: slot.alias.name,
              column: slot.column,
            }),
          }]
          : [],
    );
    return [...choices, ...projectionChoices];
  };

  const choiceForValue = (value: string, choices: ColumnChoice[]) =>
    choices.find((choice) => choice.value === value);

  const joinableColumns = () =>
    (ready()?.context.parser.columns ?? []).filter((column) =>
      ["string", "boolean", "integer", "float", "date", "timestamp", "decimal"]
        .includes(column.type)
    );

  const joinColumnChoices = () => joinableColumns();

  const joinOperationValid = () => {
    const columns = ready()?.context.parser.columns ?? [];
    const left = columns.find((column) => column.name === joinLeftColumn());
    const right = columns.find((column) => column.name === joinRightColumn());
    return Boolean(
      left && right && left.type === right.type &&
        joinableColumns().some((column) => column.name === left.name) &&
        joinableColumns().some((column) => column.name === right.name),
    );
  };

  const validRowLimitDraftValue = (value: string) => {
    if (value === "") return true;
    if (!/^\d{1,20}$/.test(value)) return false;
    return BigInt(value) <= 18_446_744_073_709_551_615n;
  };

  const rowLimitDraftValid = () =>
    (rowLimitAllDraft() || validRowLimitDraftValue(rowLimitDraft())) &&
    validRowLimitDraftValue(rowOffsetDraft());

  const rowLimitDraftChanged = () => {
    const current = ready()?.analysis.row_limit;
    return rowLimitAllDraft() !== (current?.limit_all === true) ||
      (!rowLimitAllDraft() && rowLimitDraft() !== (current?.limit ?? "")) ||
      rowOffsetDraft() !== (current?.offset ?? "");
  };

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
      "mean",
      "min",
      "max",
    ];
    return functions.filter((fn) =>
      fn === "count" || aggregateColumnsFor(fn).length > 0
    );
  };

  const aggregateColumnsOf = (aggregate: SqlStructureAggregate) => [
    ...(aggregate.column ? [aggregate.column] : []),
    ...(aggregate.arguments ?? []),
  ];

  const aggregateWithColumns = (
    aggregate: SqlStructureAggregate,
    columns: string[],
  ): SqlStructureAggregate => {
    const next = { ...aggregate };
    if (columns.length > 0) next.column = columns[0];
    else delete next.column;
    if (columns.length > 1) next.arguments = columns.slice(1);
    else delete next.arguments;
    return next;
  };

  const aggregateIsEditable = (aggregate: SqlStructureAggregate) => {
    const columns = aggregateColumnsOf(aggregate);
    if (aggregate.arguments?.length && aggregate.function !== "count") {
      return false;
    }
    if (aggregate.distinct && columns.length > 1) return false;
    if (columns.length === 0) {
      return aggregate.function === "count" && !aggregate.arguments?.length;
    }
    if (aggregate.function !== "count" && columns.length !== 1) return false;
    return columns.every((column) => {
      const capability = columnCapability(column);
      return Boolean(
        capability?.projectable &&
          capability.aggregateFunctions.includes(aggregate.function) &&
          (!aggregate.distinct || capability.distinctable),
      );
    });
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
      // The Rust proposal replaces `*` with the selected grouping keys in one
      // source-preserving edit. The browser only validates trusted column
      // capabilities and sends the selected names to that authority.
      return current.analysis.aggregates.length === 0;
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
    if (current.context.parser.relation === "") {
      return current.analysis.capabilities.distinct.enabled &&
        current.analysis.projection.length > 0;
    }
    const projected = current.analysis.projection.includes("*")
      ? current.context.parser.columns.map((column) => column.name)
      : current.analysis.projection;
    return projected.every((column) => {
      const capability = current.context.columnCapabilities[column];
      return capability?.projectable && capability.distinctable;
    }) && existingGroupByIsEditable() && existingAggregatesAreEditable();
  };

  const projectionEditValid = () => {
    const current = ready();
    if (!current) return false;
    return current.context.parser.relation === ""
      ? literalProjection().length > 0 && literalProjection().length <= 256
      : projectionSlotsValid();
  };

  const updateLiteralKind = (
    index: number,
    kind: SqlStructureLiteral["kind"],
  ) => {
    setLiteralProjection((values) =>
      values.map((value, itemIndex) => {
        if (itemIndex !== index) return value;
        switch (kind) {
          case "integer":
            return { kind, value: "0" };
          case "string":
            return { kind, value: "" };
          case "boolean":
            return { kind, value: false };
          case "null":
            return { kind };
        }
      })
    );
  };

  const updateLiteralValue = (index: number, value: string) => {
    setLiteralProjection((values) =>
      values.map((item, itemIndex) => {
        if (
          itemIndex !== index ||
          (item.kind !== "integer" && item.kind !== "string")
        ) return item;
        return { ...item, value };
      })
    );
  };

  const updateLiteralBoolean = (index: number, value: boolean) => {
    setLiteralProjection((values) =>
      values.map((item, itemIndex) =>
        itemIndex === index && item.kind === "boolean"
          ? { kind: "boolean", value }
          : item
      )
    );
  };

  const hasEditableCapabilities = (action: StructureAction): boolean => {
    const current = ready();
    if (!current) return false;
    if (action === "join") {
      const columns = joinableColumns();
      return current.analysis.capabilities.join?.enabled === true &&
        columns.some((left) =>
        columns.some((right) => left.type === right.type)
      );
    }
    if (action === "columns") {
      return current.context.parser.columns.some((column) =>
        current.context.columnCapabilities[column.name]?.projectable
      ) || (current.context.parser.relation === "" &&
        current.analysis.literal_projection.length > 0);
    }
    if (action === "filter") {
      return current.context.parser.columns.some((column) =>
        current.context.columnCapabilities[column.name]?.filterable &&
        operatorsForColumn(current.context, column.name).some((operator) =>
          isNullOperator(operator) || variablesFor(column.name).length > 0 ||
          (props.onVariablesAdded !== undefined &&
            variableTypeForColumn(column.type) !== undefined)
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
        current.analysis.aggregates.length === 0;
      return projectionCanBeGrouped && groupableColumns().length > 0 &&
        existingGroupByIsEditable() && existingAggregatesAreEditable();
    }
    return distinctProjectionIsEditable();
  };

  const structuredEditorAvailable = (action: StructureAction): boolean => {
    const current = ready();
    if (!current || current.analysis.status !== "supported") return false;
    if (action === "form") return formOptions().length > 0;
    if (action === "join") {
      return hasEditableCapabilities("join") ||
        current.analysis.capabilities.row_limit?.enabled === true;
    }
    return Boolean(
      capability(action)?.enabled && hasEditableCapabilities(action),
    );
  };

  const documentEditorFallback = (action: StructureAction | null): boolean =>
    action !== null && action !== "join" &&
    !structuredEditorAvailable(action);

  const documentEditorVisible = (action: StructureAction | null): boolean =>
    action === "join" || documentEditorFallback(action);

  useDialogFocus(() => activeAction() !== null, {
    dialog: () => dialog,
    inert: false,
    onClose: () => setActiveAction(null),
  });

  const openAction = (action: StructureAction) => {
    if (action === "form") {
      if (actionDisabled(action)) return;
      setError(null);
      setProposal(undefined);
      setProposalContextIdentity(undefined);
      setActiveAction(action);
      return;
    }
    if (action === "join") {
      if (actionDisabled(action)) return;
      setError(null);
      setProposal(undefined);
      setProposalContextIdentity(undefined);
      setRowLimitDraftIdentity(undefined);
      const current = ready();
      setStructureEditorMode(
        !current || hasEditableCapabilities("join") ? "join" : "row_limit",
      );
      setActiveAction(action);
      if (current?.analysis.status === "supported") {
        const available = current.context.parser.columns.find((column) =>
          [
            "string",
            "boolean",
            "integer",
            "float",
            "date",
            "timestamp",
            "decimal",
          ]
            .includes(column.type)
        )?.name ?? "";
        const existing = current.analysis.joins[0];
        setJoinLeftColumn(existing?.left_column ?? available);
        setJoinRightColumn(
          existing?.right_column ?? existing?.left_column ?? available,
        );
      }
      return;
    }
    const current = ready();
    if (actionDisabled(action)) return;
    setError(null);
    setProposal(undefined);
    setProposalContextIdentity(undefined);
    if (!current || !structuredEditorAvailable(action)) {
      setActiveAction(action);
      return;
    }
    const projection = current.analysis.projection;
    setLiteralProjection([...current.analysis.literal_projection]);
    const slots = current.context.parser.relation === ""
      ? []
      : projection.includes("*")
      ? projectableColumns().map((column) => ({ column: column.name }))
      : current.analysis.projection_aliases.map((slot, index) => ({
        id: `projection-${index}`,
        column: slot.column,
        alias: slot.alias,
      }));
    setProjectionSlots(slots.map((slot, index) => ({
      key: slot.id ?? `projection-draft-${index}-${createUniqueId()}`,
      ...(slot.id ? { id: slot.id } : {}),
      column: slot.column,
      alias: {
        name: slot.alias?.name ?? "",
        quoted: slot.alias?.quoted ?? false,
      },
    })));
    setFilters(
      current.analysis.filter_expression
        ? filterExpressionToDraft(current.analysis.filter_expression)
        : flatFiltersToDraft(current.analysis.filters),
    );
    setSorts(current.analysis.order_by.map((sort) => ({ ...sort })));
    setAggregates(current.analysis.aggregates.map((aggregate) => ({
      ...aggregate,
      ...(aggregate.alias ? { alias: { ...aggregate.alias } } : {}),
    })));
    setGroupByColumns([...current.analysis.group_by]);
    setSelectDistinct(current.analysis.distinct);
    const availableJoinColumn = joinableColumns()[0]?.name ?? "";
    const existingJoin = current.analysis.joins[0];
    setJoinLeftColumn(existingJoin?.left_column ?? availableJoinColumn);
    setJoinRightColumn(
      existingJoin?.right_column ?? existingJoin?.left_column ??
        availableJoinColumn,
    );
    setActiveAction(action);
  };

  createEffect(() => {
    if (activeAction() !== "join") return;
    const current = ready();
    if (current?.analysis.status !== "supported") return;
    if (rowLimitDraftIdentity() === current.identity) return;
    setRowLimitDraft(current.analysis.row_limit?.limit ?? "");
    setRowOffsetDraft(current.analysis.row_limit?.offset ?? "");
    setRowLimitAllDraft(current.analysis.row_limit?.limit_all === true);
    setRowLimitDraftIdentity(current.identity);
    if (
      structureEditorMode() === "join" && !hasEditableCapabilities("join")
    ) {
      setStructureEditorMode("row_limit");
    }
  });

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

  const filterVariableType = (columnName: string) => {
    const column = ready()?.context.parser.columns.find((item) =>
      item.name === columnName
    );
    return column ? variableTypeForColumn(column.type) : undefined;
  };

  const uniqueFilterVariableName = (columnName: string) => {
    const suffix = columnName.replace(/[^A-Za-z0-9_]/g, "_");
    const base = `filter_${suffix || "value"}`;
    const used = new Set([
      ...(ready()?.context.parser.variables ?? []).map((variable) =>
        variable.name
      ),
      ...(props.reservedVariableNames?.() ?? []),
      ...filterDraftLeaves(filters()).map((filter) => filter.parameter),
    ]);
    if (!used.has(base)) return base;
    let index = 2;
    while (used.has(`${base}_${index}`)) index += 1;
    return `${base}_${index}`;
  };

  const variablesCreatedByFilters = (
    existing: readonly { name: string; type: SqlStructureVariableType }[],
    drafts: FilterDraft | undefined = filters(),
  ) => {
    const existingNames = new Set(existing.map((variable) => variable.name));
    const byName = new Map<string, SqlStructureVariableType>();
    for (const filter of filterDraftLeaves(drafts)) {
      if (
        filter.createdVariable && !existingNames.has(filter.parameter)
      ) byName.set(filter.parameter, filter.createdVariable);
    }
    return [...byName].map(([name, type]) => ({ name, type }));
  };

  const filterableColumns = () =>
    (ready()?.context.parser.columns ?? []).filter((column) =>
      ready()?.context.columnCapabilities[column.name]?.filterable
    );

  const canAddFilter = () =>
    filterableColumns().some((column) =>
      operatorsForColumn(ready()?.context, column.name).some((operator) =>
        isNullOperator(operator) || variablesFor(column.name).length > 0 ||
        (props.onVariablesAdded !== undefined &&
          filterVariableType(column.name) !== undefined)
      )
    );

  const makeFilterDraft = (): FilterPredicateDraft | undefined => {
    if (busy() || proposal()) return;
    const candidate = filterableColumns().flatMap((column) => {
      const operators = operatorsForColumn(ready()?.context, column.name);
      const operator = operators.find((item) =>
        !isNullOperator(item) && variablesFor(column.name).length > 0
      ) ?? (props.onVariablesAdded && filterVariableType(column.name)
        ? operators.find((item) =>
          !isNullOperator(item)
        )
        : undefined) ??
        operators.find(isNullOperator);
      return operator
        ? [{ column, operator, qualifier: activeJoin()?.left_alias }]
        : [];
    })[0];
    if (!candidate) return undefined;
    const compatibleVariables = variablesFor(candidate.column.name);
    const createdVariable = !isNullOperator(candidate.operator) &&
        compatibleVariables.length === 0 && props.onVariablesAdded
      ? filterVariableType(candidate.column.name)
      : undefined;
    return {
      kind: "predicate",
      id: createUniqueId(),
      column: candidate.column.name,
      ...(candidate.qualifier ? { qualifier: candidate.qualifier } : {}),
      operator: candidate.operator,
      parameter: isNullOperator(candidate.operator)
        ? ""
        : compatibleVariables[0]?.name ??
          (createdVariable
            ? uniqueFilterVariableName(candidate.column.name)
            : ""),
      ...(createdVariable ? { createdVariable } : {}),
    };
  };

  const updateFilterDraft = (
    draft: FilterDraft,
    id: string,
    update: (node: FilterDraft) => FilterDraft,
  ): FilterDraft => {
    if (draft.id === id) return update(draft);
    if (draft.kind === "not") {
      return {
        ...draft,
        expression: updateFilterDraft(draft.expression, id, update),
      };
    }
    if (draft.kind === "and" || draft.kind === "or") {
      return {
        ...draft,
        expressions: draft.expressions.map((child) =>
          updateFilterDraft(child, id, update)
        ),
      };
    }
    return draft;
  };

  const removeFilterDraft = (
    draft: FilterDraft,
    id: string,
  ): FilterDraft | undefined => {
    if (draft.id === id) return undefined;
    if (draft.kind === "not") {
      const expression = removeFilterDraft(draft.expression, id);
      return expression ? { ...draft, expression } : undefined;
    }
    if (draft.kind === "and" || draft.kind === "or") {
      const expressions = draft.expressions
        .map((child) => removeFilterDraft(child, id))
        .filter((child): child is FilterDraft => child !== undefined);
      if (expressions.length === 0) return undefined;
      if (expressions.length === 1) return expressions[0];
      return { ...draft, expressions };
    }
    return draft;
  };

  const setFilterDraft = (
    next:
      | FilterDraft
      | undefined
      | (
        (previous: FilterDraft | undefined) => FilterDraft | undefined
      ),
  ) => {
    const active = typeof document === "undefined"
      ? undefined
      : document.activeElement instanceof HTMLElement
      ? document.activeElement
      : undefined;
    const filterList = active?.closest<HTMLElement>(
      ".sql-structure-filter-list",
    );
    if (!filterList) {
      setFilters(next);
      return;
    }

    const focusKey = active?.dataset.filterFocusKey;
    const nodeIds: string[] = [];
    let node = active?.closest<HTMLElement>("[data-filter-node]");
    while (node) {
      if (node.dataset.filterNode) nodeIds.push(node.dataset.filterNode);
      node = node.parentElement?.closest<HTMLElement>("[data-filter-node]");
    }
    setFilters(next);
    queueMicrotask(() => {
      const dialog = filterList.closest<HTMLElement>('[role="dialog"]');
      if (!dialog?.isConnected) return;
      const controls = Array.from(
        dialog.querySelectorAll<HTMLElement>("[data-filter-focus-key]"),
      );
      const target = controls.find((control) =>
        control.dataset.filterFocusKey === focusKey
      ) ?? nodeIds.map((id) => {
        const retainedNode = Array.from(
          dialog.querySelectorAll<HTMLElement>("[data-filter-node]"),
        ).find((candidate) =>
          candidate.dataset.filterNode === id
        );
        return retainedNode?.querySelector<HTMLElement>(
          "[data-filter-focus-key]",
        );
      }).find((control) => control !== undefined) ?? controls[0];
      target?.focus();
    });
  };

  const addFilter = () => {
    if (busy() || proposal()) return;
    const filter = makeFilterDraft();
    if (!filter) return;
    const current = filters();
    if (!current) {
      setFilterDraft(filter);
    } else if (current.kind === "and") {
      setFilterDraft({
        ...current,
        expressions: [...current.expressions, filter],
      });
    } else {
      setFilterDraft({
        kind: "and",
        id: createUniqueId(),
        expressions: [current, filter],
      });
    }
  };

  const addFilterToGroup = (id: string) => {
    if (busy() || proposal()) return;
    const filter = makeFilterDraft();
    const current = filters();
    if (!filter || !current) return;
    setFilterDraft(
      updateFilterDraft(
        current,
        id,
        (node) =>
          node.kind === "and" || node.kind === "or"
            ? { ...node, expressions: [...node.expressions, filter] }
            : node,
      ),
    );
  };

  const addGroup = (parentId?: string, kind: "and" | "or" = "and") => {
    if (busy() || proposal()) return;
    const group: FilterDraft = {
      kind,
      id: createUniqueId(),
      expressions: [],
    };
    const current = filters();
    if (!current) {
      setFilterDraft(group);
    } else if (parentId) {
      setFilterDraft(
        updateFilterDraft(
          current,
          parentId,
          (node) =>
            node.kind === "and" || node.kind === "or"
              ? { ...node, expressions: [...node.expressions, group] }
              : node,
        ),
      );
    } else if (current.kind === "and") {
      setFilterDraft({
        ...current,
        expressions: [...current.expressions, group],
      });
    } else {
      setFilterDraft({
        kind: "and",
        id: createUniqueId(),
        expressions: [current, group],
      });
    }
  };

  const addNot = (id: string) => {
    if (busy() || proposal()) return;
    const current = filters();
    if (!current) return;
    setFilterDraft(updateFilterDraft(current, id, (node) => ({
      kind: "not",
      id: createUniqueId(),
      expression: node,
    })));
  };

  const removeNot = (id: string) => {
    if (busy() || proposal()) return;
    const current = filters();
    if (!current) return;
    setFilterDraft(
      updateFilterDraft(
        current,
        id,
        (node) => node.kind === "not" ? node.expression : node,
      ),
    );
  };

  const setFilterGroupKind = (id: string, kind: "and" | "or") => {
    if (busy() || proposal()) return;
    const current = filters();
    if (!current) return;
    setFilterDraft(
      updateFilterDraft(
        current,
        id,
        (node) =>
          node.kind === "and" || node.kind === "or" ? { ...node, kind } : node,
      ),
    );
  };

  const removeFilter = (id: string) => {
    if (busy() || proposal()) return;
    const current = filters();
    if (current) setFilterDraft(removeFilterDraft(current, id));
  };

  const updateFilter = (id: string, update: Partial<FilterPredicateDraft>) => {
    if (busy() || proposal()) return;
    const current = filters();
    if (!current) return;
    setFilterDraft(updateFilterDraft(current, id, (item) => {
      if (item.kind !== "predicate") return item;
      const next: FilterPredicateDraft = { ...item, ...update };
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
        const existingVariable = compatibleVariables.find((variable) =>
          variable.name === next.parameter
        );
        if (existingVariable) {
          delete next.createdVariable;
        } else if (
          next.createdVariable &&
          next.createdVariable === filterVariableType(next.column) &&
          next.parameter
        ) {
          // Keep the locally-created typed parameter while the user edits
          // other parts of the same filter row.
        } else if (compatibleVariables[0]) {
          next.parameter = compatibleVariables[0].name;
          delete next.createdVariable;
        } else if (
          props.onVariablesAdded &&
          filterVariableType(next.column)
        ) {
          next.parameter = uniqueFilterVariableName(next.column);
          next.createdVariable = filterVariableType(next.column);
        } else {
          next.parameter = "";
          delete next.createdVariable;
        }
      } else {
        next.parameter = "";
        delete next.createdVariable;
      }
      return next;
    }));
  };

  const filterRowsValid = () => {
    let nodes = 0;
    const isValid = (filter: FilterDraft, depth = 0): boolean => {
      nodes += 1;
      if (nodes > 128 || depth > 32) return false;
      if (filter.kind === "and" || filter.kind === "or") {
        return filter.expressions.length >= 2 &&
          filter.expressions.every((child) => isValid(child, depth + 1));
      }
      if (filter.kind === "not") return isValid(filter.expression, depth + 1);
      return Boolean(
        ready()?.context.columnCapabilities[filter.column]?.filterable &&
          (activeJoin()
            ? [activeJoin()?.left_alias, activeJoin()?.right_alias].some(
              (alias) =>
                alias?.name === filter.qualifier?.name &&
                alias.quoted === filter.qualifier?.quoted,
            )
            : !filter.qualifier) &&
          operatorsForColumn(ready()?.context, filter.column).includes(
            filter.operator,
          ) &&
          (isNullOperator(filter.operator) ||
            (filter.parameter.length > 0 &&
              (variablesFor(filter.column).some((variable) =>
                variable.name === filter.parameter
              ) ||
                filter.createdVariable === filterVariableType(filter.column)))),
      );
    };
    const filter = filters();
    return !filter || isValid(filter);
  };

  const filterRootIsGroup = () => {
    const filter = filters();
    return filter?.kind === "and" || filter?.kind === "or";
  };

  const filterIndentClass = (depth: number) =>
    depth > 0 && depth <= 3 ? " sql-structure-filter--indented" : "";

  const renderFilterNode = (filter: FilterDraft, depth = 0) => {
    if (filter.kind === "predicate") {
      const context = () => ready()?.context;
      const operators = () => operatorsForColumn(context(), filter.column);
      const variables = () => variablesFor(filter.column);
      const allowedChoices = () => {
        const choices = columnChoices(filterableColumns());
        const selected = selectedColumnChoice(
          filter.column,
          filter.qualifier,
        );
        return choices.some((choice) => choice.value === selected)
          ? choices
          : [{
            column: filter.column,
            qualifier: filter.qualifier,
            value: selected,
            label: filter.qualifier
              ? `${filter.qualifier.name}.${filter.column}`
              : filter.column,
          }, ...choices];
      };
      const rowEditable = () =>
        Boolean(
          context()?.columnCapabilities[filter.column]?.filterable &&
            operators().length > 0,
        );
      return (
        <div
          class="sql-structure-filter-node"
          data-filter-node={filter.id}
          data-filter-depth={depth}
        >
          <div class="sql-structure-filter-row">
            <label class="ui-field">
              <span>{t("sqlPage.structure.column")}</span>
              <select
                class="ui-input"
                data-filter-focus-key={`${filter.id}:column`}
                value={selectedColumnChoice(filter.column, filter.qualifier)}
                disabled={busy() || Boolean(proposal()) || !rowEditable()}
                onChange={(event) => {
                  const selected = choiceForValue(
                    event.currentTarget.value,
                    allowedChoices(),
                  );
                  if (selected) {
                    updateFilter(filter.id, {
                      column: selected.column,
                      qualifier: selected.qualifier,
                    });
                  }
                }}
              >
                <For each={allowedChoices()}>
                  {(choice) => (
                    <option value={choice.value}>{choice.label}</option>
                  )}
                </For>
              </select>
            </label>
            <label class="ui-field">
              <span>{t("sqlPage.structure.operator")}</span>
              <select
                class="ui-input"
                data-filter-focus-key={`${filter.id}:operator`}
                value={filter.operator}
                disabled={busy() || Boolean(proposal()) || !rowEditable()}
                onChange={(event) =>
                  updateFilter(filter.id, {
                    operator: event.currentTarget
                      .value as SqlStructureFilterOperator,
                  })}
              >
                <For each={operators()}>
                  {(operator) => (
                    <option value={operator}>{t(operatorKey(operator))}</option>
                  )}
                </For>
                <Show when={!operators().includes(filter.operator)}>
                  <option value={filter.operator}>
                    {t("sqlPage.structure.unsupportedOperator")}
                  </option>
                </Show>
              </select>
            </label>
            <Show when={!isNullOperator(filter.operator)}>
              <label class="ui-field">
                <span>{t("sqlPage.structure.variable")}</span>
                <select
                  class="ui-input"
                  data-filter-focus-key={`${filter.id}:variable`}
                  value={filter.parameter}
                  disabled={busy() || Boolean(proposal()) || !rowEditable()}
                  onChange={(event) =>
                    updateFilter(filter.id, {
                      parameter: event.currentTarget.value,
                    })}
                >
                  <For each={variables()}>
                    {(variable) => (
                      <option value={variable.name}>{variable.name}</option>
                    )}
                  </For>
                  <Show
                    when={!variables().some((variable) =>
                      variable.name === filter.parameter
                    )}
                  >
                    <option value={filter.parameter}>{filter.parameter}</option>
                  </Show>
                </select>
              </label>
            </Show>
            <div class="sql-structure-filter-node-actions">
              <button
                type="button"
                class="ui-button ui-button-secondary"
                data-filter-focus-key={`${filter.id}:negate`}
                aria-label={t("sqlPage.structure.negateFilter")}
                title={t("sqlPage.structure.negateFilter")}
                disabled={busy() || Boolean(proposal()) ||
                  !supportsFilterExpression()}
                onClick={() => addNot(filter.id)}
              >
                NOT
              </button>
              <IconButton
                icon="trash"
                label={t("sqlPage.structure.removeFilter")}
                disabled={busy() || Boolean(proposal())}
                onClick={() => removeFilter(filter.id)}
              />
            </div>
          </div>
        </div>
      );
    }

    if (filter.kind === "not") {
      return (
        <div
          class={`sql-structure-filter-not${filterIndentClass(depth)}`}
          data-filter-node={filter.id}
          data-filter-depth={depth}
        >
          <span class="sql-structure-filter-operator">NOT</span>
          {renderFilterNode(filter.expression, depth + 1)}
          <IconButton
            icon="trash"
            label={t("sqlPage.structure.removeNot")}
            disabled={busy() || Boolean(proposal())}
            onClick={() => removeNot(filter.id)}
          />
        </div>
      );
    }

    return (
      <section
        class={`sql-structure-filter-group${filterIndentClass(depth)}`}
        role="group"
        aria-label={t("sqlPage.structure.filterGroup")}
        data-filter-node={filter.id}
        data-filter-depth={depth}
      >
        <div class="sql-structure-filter-group-controls">
          <select
            class="ui-input"
            data-filter-focus-key={`${filter.id}:operator`}
            aria-label={t("sqlPage.structure.filterGroupOperator")}
            value={filter.kind}
            disabled={busy() || Boolean(proposal())}
            onChange={(event) =>
              setFilterGroupKind(
                filter.id,
                event.currentTarget.value as "and" | "or",
              )}
          >
            <option value="and">AND</option>
            <Show when={supportsFilterExpression()}>
              <option value="or">OR</option>
            </Show>
          </select>
          <button
            type="button"
            class="ui-button ui-button-secondary"
            data-filter-focus-key={`${filter.id}:add-filter`}
            onClick={() => addFilterToGroup(filter.id)}
            disabled={busy() || Boolean(proposal()) || !canAddFilter()}
          >
            {t("sqlPage.structure.addFilter")}
          </button>
          <button
            type="button"
            class="ui-button ui-button-secondary"
            data-filter-focus-key={`${filter.id}:add-group`}
            onClick={() => addGroup(filter.id)}
            disabled={busy() || Boolean(proposal())}
          >
            {t("sqlPage.structure.addFilterGroup")}
          </button>
          <button
            type="button"
            class="ui-button ui-button-secondary"
            data-filter-focus-key={`${filter.id}:negate`}
            aria-label={t("sqlPage.structure.negateFilter")}
            title={t("sqlPage.structure.negateFilter")}
            onClick={() => addNot(filter.id)}
            disabled={busy() || Boolean(proposal()) ||
              !supportsFilterExpression()}
          >
            NOT
          </button>
          <IconButton
            icon="trash"
            label={t("sqlPage.structure.removeFilterGroup")}
            disabled={busy() || Boolean(proposal())}
            onClick={() => removeFilter(filter.id)}
          />
        </div>
        <div class="sql-structure-filter-group-children">
          <For each={filter.expressions}>
            {(child) => renderFilterNode(child, depth + 1)}
          </For>
        </div>
      </section>
    );
  };

  const sortableColumns = () =>
    (ready()?.context.parser.columns ?? []).filter((column) =>
      ready()?.context.columnCapabilities[column.name]?.sortable &&
      ["string", "boolean", "integer", "float", "date", "timestamp", "decimal"]
        .includes(column.type)
    );

  const sortRowsValid = () =>
    sorts().every((sort) =>
      sortChoices().some((choice) => choice.value === sortChoiceValue(sort))
    );

  const addSort = () => {
    const choices = sortChoices();
    const available = choices.find((choice) =>
      !sorts().some((sort) => sortChoiceValue(sort) === choice.value)
    );
    if (!available) return;
    setSorts((items) => [...items, {
      column: available.column,
      ...(available.qualifier ? { qualifier: available.qualifier } : {}),
      ...(available.projection_alias
        ? { projection_alias: available.projection_alias }
        : {}),
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
        const columns = aggregateColumnsOf(item);
        let distinct = item.distinct;
        if (fn === "count") {
          if (distinct && columns.length > 1) distinct = false;
        } else {
          const validColumns = aggregateColumnsFor(fn, distinct);
          const retained = columns.find((column) =>
            validColumns.some((candidate) => candidate.name === column)
          );
          const fallback = retained ?? validColumns[0]?.name;
          columns.splice(0, columns.length, ...(fallback ? [fallback] : []));
          if (distinct && !fallback) distinct = false;
        }
        if (distinct && columns.length > 1) distinct = false;
        return {
          ...aggregateWithColumns(item, columns),
          function: fn,
          distinct,
        };
      })
    );
  };

  const updateAggregateArgument = (
    index: number,
    argumentIndex: number,
    value: string,
  ) => {
    setAggregates((items) =>
      items.map((item, itemIndex) => {
        if (index !== itemIndex) return item;
        const columns = aggregateColumnsOf(item);
        if (columns.length === 0) columns.push("");
        if (columns.length === 1 && columns[0] === "") {
          if (value) columns[0] = value;
        } else if (value) columns[argumentIndex] = value;
        else columns.splice(argumentIndex, 1);
        const nextColumns = columns.filter(Boolean);
        const distinctAllowed = nextColumns.length === 0
          ? item.function === "count"
          : nextColumns.length === 1 && Boolean(
            columnCapability(nextColumns[0])?.distinctable,
          );
        return {
          ...aggregateWithColumns(item, nextColumns),
          distinct: item.distinct && distinctAllowed,
        };
      })
    );
  };

  const addAggregateArgument = (index: number) => {
    const existingColumns = aggregateColumnsOf(aggregates()[index]);
    const column =
      aggregateColumnsFor("count").find((candidate) =>
        !existingColumns.includes(candidate.name)
      ) ?? aggregateColumnsFor("count")[0];
    if (!column) return;
    setAggregates((items) =>
      items.map((item, itemIndex) => {
        if (index !== itemIndex || item.function !== "count") return item;
        const columns = aggregateColumnsOf(item).filter(Boolean);
        columns.push(column.name);
        return { ...aggregateWithColumns(item, columns), distinct: false };
      })
    );
  };

  const removeAggregateArgument = (index: number, argumentIndex: number) => {
    setAggregates((items) =>
      items.map((item, itemIndex) => {
        if (index !== itemIndex || item.function !== "count") return item;
        const columns = aggregateColumnsOf(item);
        columns.splice(argumentIndex, 1);
        return { ...aggregateWithColumns(item, columns), distinct: false };
      })
    );
  };

  const updateAggregateDistinct = (index: number, distinct: boolean) => {
    setAggregates((items) =>
      items.map((item, itemIndex) => {
        if (index !== itemIndex) return item;
        const columns = aggregateColumnsOf(item);
        const allowed = columns.length === 0
          ? item.function === "count"
          : columns.length === 1 && Boolean(
            columnCapability(columns[0])?.distinctable,
          );
        return {
          ...item,
          distinct: Boolean(distinct && allowed),
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
    if (
      !current || !action || action === "form" || actionDisabled(action) ||
      busy() || (action === "join" && current.analysis.status !== "supported")
    ) return;
    const legacyFilters = action === "filter" &&
        !supportsFilterExpression()
      ? filterDraftToFlatFilters(filters())
      : undefined;
    if (
      action === "filter" && !supportsFilterExpression() && !legacyFilters
    ) {
      setError(unavailableReason());
      return;
    }
    const operation: SqlStructureOperation = action === "join" &&
        structureEditorMode() === "row_limit"
      ? {
        kind: "set_row_limit",
        limit: rowLimitAllDraft() ? null : rowLimitDraft() || null,
        offset: rowOffsetDraft() || null,
        limit_all: rowLimitAllDraft(),
      }
      : action === "columns"
      ? current.context.parser.relation === ""
        ? { kind: "set_literal_projection", values: literalProjection() }
        : {
          kind: "set_projection_slots",
          slots: projectionSlots().map((
            slot,
          ): SqlStructureProjectionSlotInput => ({
            ...(slot.id ? { id: slot.id } : {}),
            column: slot.column,
            alias: slot.alias.name.length
              ? { name: slot.alias.name, quoted: slot.alias.quoted }
              : null,
          })),
        }
      : action === "join"
      ? {
        kind: "set_join",
        left_column: joinLeftColumn(),
        right_column: joinRightColumn(),
      }
      : action === "filter"
      ? supportsFilterExpression()
        ? {
          kind: "set_filter_expression" as const,
          expression: filters() ? filterDraftToExpression(filters()!) : null,
        }
        : { kind: "set_filters", filters: legacyFilters! }
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
      action === "columns" && !projectionEditValid() ||
      action === "join" && structureEditorMode() === "join" &&
        !joinOperationValid() ||
      action === "join" && structureEditorMode() === "row_limit" &&
        (!rowLimitDraftValid() || !rowLimitDraftChanged()) ||
      action === "filter" && !filterRowsValid() ||
      action === "sort" && !sortRowsValid() ||
      action === "aggregate" && !aggregateOperationValid() ||
      action === "group_by" && !groupByRowsValid() ||
      action === "distinct" && !distinctProjectionIsEditable()
    ) return;

    const baseSql = props.sql();
    const parserSql = normalizeSqlVariables(baseSql).sql;
    const filterDrafts = filters();
    const createdVariables = variablesCreatedByFilters(
      current.context.parser.variables,
      filterDrafts,
    );
    const proposalContext = createdVariables.length > 0
      ? {
        ...current.context.parser,
        variables: [...current.context.parser.variables, ...createdVariables],
      }
      : current.context.parser;
    const contextIdentity = current.identity;
    const generation = ++proposalGeneration;
    setBusy(true);
    setError(null);
    try {
      const result = await proposeSqlStructureOperation(
        parserSql,
        proposalContext,
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
      setProposalVariables(createdVariables);
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

  const selectForm = async (target: SqlStructureAdapterContext) => {
    if (actionDisabled("form") || busy() || !target.parser.relation) return;
    const baseSql = props.sql();
    const parserSql = normalizeSqlVariables(baseSql).sql;
    const currentIdentity = ready()?.identity;
    const targetIdentity = target.identity;
    const generation = ++proposalGeneration;
    setBusy(true);
    setError(null);
    setProposal(undefined);
    setProposalVariables([]);
    setProposalContextIdentity(undefined);
    try {
      await prepareSqlStructureParser();
      const latestTarget = formOptions().find((context) =>
        context.identity === targetIdentity
      );
      if (
        generation !== proposalGeneration || props.sql() !== baseSql ||
        !latestTarget
      ) return;
      const result = await proposeSqlStructureOperation(
        parserSql,
        latestTarget.parser,
        { kind: "set_relation", relation: latestTarget.parser.relation },
      );
      if (
        generation !== proposalGeneration || props.sql() !== baseSql ||
        ready()?.identity !== currentIdentity
      ) return;
      if (result.status !== "supported" || result.proposed_sql === undefined) {
        setError(unavailableReason());
        return;
      }
      if (!result.requires_confirmation) {
        setActiveAction(null);
        return;
      }
      setProposal({ ...result, base_sql: baseSql });
      setProposalContextIdentity(currentIdentity);
    } catch {
      if (
        generation === proposalGeneration && props.sql() === baseSql &&
        ready()?.identity === currentIdentity
      ) {
        setError(t("sqlPage.structureUnavailable"));
      }
    } finally {
      if (generation === proposalGeneration) setBusy(false);
    }
  };

  const replaceDocumentNode = async (edit: SqlDocumentNodeEdit) => {
    const document = sqlDocument();
    const action = activeAction();
    if (
      !document || document.status !== "parsed" ||
      document.source !== props.sql() || props.disabled?.() ||
      !props.onSqlChange || busy() || !action ||
      (action !== "join" && !documentEditorFallback(action))
    ) return;

    const baseSql = document.source;
    const generation = ++proposalGeneration;
    setBusy(true);
    setError(null);
    try {
      const result = await applySqlDocumentNodeEditTransaction(baseSql, {
        contract_version: document.contract_version,
        expected_source: baseSql,
        edits: [edit],
      });
      if (
        generation !== proposalGeneration || props.sql() !== baseSql ||
        sqlDocument() !== document || props.disabled?.() ||
        activeAction() !== action ||
        (action !== "join" && !documentEditorFallback(action))
      ) return;

      // Rust resolves the node ID against the guarded source and returns the
      // exact replacement plus a parsed next view. Saving/running remains the
      // host's normal validated SQL path.
      commitStructuredSql(baseSql, result.document.source);
      setSqlDocument(result.document);
    } catch {
      if (generation === proposalGeneration && props.sql() === baseSql) {
        setError(t("sqlPage.structureUnavailable"));
      }
    } finally {
      if (generation === proposalGeneration) setBusy(false);
    }
  };

  const confirmProposal = async () => {
    const current = proposal();
    const contextIdentity = proposalContextIdentity();
    if (
      !current || current.proposed_sql === undefined ||
      props.sql() !== current.base_sql || props.disabled?.() ||
      !props.onSqlChange || ready()?.identity !== contextIdentity
    ) {
      setProposal(undefined);
      setActiveAction(null);
      return;
    }

    const generation = ++proposalGeneration;
    setBusy(true);
    setError(null);
    try {
      const result = await applySqlDocumentReplacementTransaction(
        current.base_sql,
        current.proposed_sql,
      );

      // SQL Document parsing validates and returns the exact draft only. It
      // does not grant executor admission to a query.
      if (
        generation !== proposalGeneration || proposal() !== current ||
        props.sql() !== current.base_sql || props.disabled?.() ||
        ready()?.identity !== contextIdentity
      ) {
        return;
      }
      if (
        result.document.status !== "parsed" ||
        result.document.source !== current.proposed_sql
      ) {
        setError(t("sqlPage.structureUnavailable"));
        return;
      }

      const createdVariables = proposalVariables();
      if (createdVariables.length > 0) {
        if (props.onVariablesAdded?.(createdVariables) === false) {
          setError(t("sqlPage.structureUnavailable"));
          return;
        }
      }

      commitStructuredSql(
        current.base_sql,
        result.document.source,
        createdVariables,
      );
      setProposal(undefined);
      setProposalVariables([]);
      setProposalContextIdentity(undefined);
      setActiveAction(null);
    } catch {
      if (generation === proposalGeneration) {
        setError(t("sqlPage.structureUnavailable"));
      }
    } finally {
      if (generation === proposalGeneration) setBusy(false);
    }
  };

  const close = () => {
    proposalGeneration += 1;
    setBusy(false);
    setError(null);
    setProposal(undefined);
    setProposalVariables([]);
    setProposalContextIdentity(undefined);
    setActiveAction(null);
  };

  const actionTitle = () => {
    const action = activeAction();
    return proposal()
      ? t("sqlPage.structure.proposal")
      : action
      ? action === "join"
        ? t("sqlPage.structure.structure")
        : actionLabel(action)
      : "";
  };

  const operationBlockedReason = () => {
    if (activeAction() === "aggregate" && !aggregateOperationValid()) {
      return t("sqlPage.structure.addAggregateFirst");
    }
    if (activeAction() === "group_by" && !groupByRowsValid()) {
      return t("sqlPage.structure.groupProjectedColumns");
    }
    if (
      activeAction() === "join" && structureEditorMode() === "row_limit" &&
      !rowLimitDraftValid()
    ) {
      return t("sqlPage.structure.invalidRowLimit");
    }
    return undefined;
  };

  createEffect(() => {
    const actions = ([
      "form",
      "join",
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
          kind: action === "join" ? "structure" : action,
          icon: action === "form"
            ? "forms"
            : action === "join"
            ? "group-by"
            : action === "group_by"
            ? "group-by"
            : action,
          label: action === "join" ? t("sqlPage.structure.structure") : label,
          disabled,
          disabledReason: disabled ? reason : undefined,
          active: action === "join"
            ? activeAction() === "join"
            : activeAction() === action,
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
              <Show when={props.onSqlChange}>
                <IconButton
                  icon="undo"
                  label={t("sqlPage.structure.undo")}
                  disabled={undoSql().length === 0 ||
                    (Boolean(undoSql().at(-1)?.variablesAdded.length) &&
                      !props.onVariablesRemoved) ||
                    busy() ||
                    Boolean(props.disabled?.())}
                  onClick={undoStructuredSql}
                />
                <IconButton
                  icon="redo"
                  label={t("sqlPage.structure.redo")}
                  disabled={redoSql().length === 0 ||
                    (Boolean(redoSql().at(-1)?.variablesAdded.length) &&
                      !props.onVariablesAdded) ||
                    busy() ||
                    Boolean(props.disabled?.())}
                  onClick={redoStructuredSql}
                />
              </Show>
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
                  <Show
                    when={activeAction() === "form" &&
                      formOptions().length > 0}
                  >
                    <div class="sql-structure-form-list">
                      <For each={formOptions()}>
                        {(context) => (
                          <button
                            type="button"
                            class="sql-structure-form-option"
                            disabled={busy() || actionDisabled("form")}
                            onClick={() => void selectForm(context)}
                          >
                            <UiIcon name="forms" />
                            <span>
                              {context.label ?? context.parser.relation}
                            </span>
                          </button>
                        )}
                      </For>
                    </div>
                  </Show>

                  <Show
                    when={documentEditorVisible(activeAction()) &&
                      sqlDocument()?.status === "parsed"}
                  >
                    <SqlDocumentStructureEditor
                      document={sqlDocument()!}
                      disabled={busy() || Boolean(props.disabled?.()) ||
                        !props.onSqlChange}
                      onReplaceNode={(edit) => void replaceDocumentNode(edit)}
                    />
                  </Show>

                  <Show when={activeAction() === "join"}>
                    <div
                      class="sql-structure-editor-modes"
                      role="group"
                      aria-label={t("sqlPage.structure.structure")}
                    >
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        aria-pressed={structureEditorMode() === "join"}
                        disabled={!hasEditableCapabilities("join")}
                        onClick={() => {
                          setStructureEditorMode("join");
                          setProposal(undefined);
                        }}
                      >
                        {t("sqlPage.structure.join")}
                      </button>
                      <button
                        type="button"
                        class="ui-button ui-button-secondary"
                        aria-pressed={structureEditorMode() === "row_limit"}
                        onClick={() => {
                          setStructureEditorMode("row_limit");
                          setProposal(undefined);
                        }}
                      >
                        {t("sqlPage.structure.rowLimit")}
                      </button>
                    </div>
                    <Show when={structureEditorMode() === "row_limit"}>
                      <div class="sql-structure-row-limit">
                        <label class="ui-field sql-structure-limit-all">
                          <input
                            type="checkbox"
                            checked={rowLimitAllDraft()}
                            disabled={busy() || Boolean(proposal())}
                            onChange={(event) =>
                              setRowLimitAllDraft(event.currentTarget.checked)}
                          />
                          <span>LIMIT ALL</span>
                        </label>
                        <Show when={!rowLimitAllDraft()}>
                          <label class="ui-field">
                            <span>LIMIT</span>
                            <input
                              class="ui-input"
                              type="text"
                              inputMode="numeric"
                              pattern="[0-9]*"
                              maxLength={20}
                              value={rowLimitDraft()}
                              disabled={busy() || Boolean(proposal())}
                              onInput={(event) =>
                                setRowLimitDraft(event.currentTarget.value)}
                            />
                          </label>
                        </Show>
                        <label class="ui-field">
                          <span>OFFSET</span>
                          <input
                            class="ui-input"
                            type="text"
                            inputMode="numeric"
                            pattern="[0-9]*"
                            maxLength={20}
                            value={rowOffsetDraft()}
                            disabled={busy() || Boolean(proposal())}
                            onInput={(event) =>
                              setRowOffsetDraft(event.currentTarget.value)}
                          />
                        </label>
                      </div>
                      <Show when={!rowLimitDraftValid()}>
                        <p class="ui-text-danger" role="alert">
                          {t("sqlPage.structure.invalidRowLimit")}
                        </p>
                      </Show>
                    </Show>
                    <Show when={structureEditorMode() === "join"}>
                    <Show
                      when={hasEditableCapabilities("join")}
                    >
                      <div class="sql-structure-join-fields">
                        <label class="ui-field">
                          <span>{t("sqlPage.structure.leftColumn")}</span>
                          <select
                            class="ui-input"
                            value={joinLeftColumn()}
                            disabled={busy() || Boolean(proposal())}
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              const selectedType = ready()?.context.parser
                                .columns
                                .find((column) => column.name === value)?.type;
                              setJoinLeftColumn(value);
                              const counterpartType = ready()?.context.parser
                                .columns.find((column) =>
                                  column.name === joinRightColumn()
                                )?.type;
                              if (selectedType !== counterpartType) {
                                setJoinRightColumn(
                                  joinableColumns().find((column) =>
                                    column.type === selectedType
                                  )?.name ?? value,
                                );
                              }
                            }}
                          >
                            <For each={joinColumnChoices()}>
                              {(column) => (
                                <option value={column.name}>
                                  {column.name}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                        <label class="ui-field">
                          <span>{t("sqlPage.structure.rightColumn")}</span>
                          <select
                            class="ui-input"
                            value={joinRightColumn()}
                            disabled={busy() || Boolean(proposal())}
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              const selectedType = ready()?.context.parser
                                .columns
                                .find((column) => column.name === value)?.type;
                              setJoinRightColumn(value);
                              const counterpartType = ready()?.context.parser
                                .columns.find((column) =>
                                  column.name === joinLeftColumn()
                                )?.type;
                              if (selectedType !== counterpartType) {
                                setJoinLeftColumn(
                                  joinableColumns().find((column) =>
                                    column.type === selectedType
                                  )?.name ?? value,
                                );
                              }
                            }}
                          >
                            <For each={joinColumnChoices()}>
                              {(column) => (
                                <option value={column.name}>
                                  {column.name}
                                </option>
                              )}
                            </For>
                          </select>
                        </label>
                      </div>
                    </Show>
                    </Show>
                  </Show>

                  <Show
                    when={activeAction() === "columns" &&
                      structuredEditorAvailable("columns")}
                  >
                    <Show
                      when={ready()?.context.parser.relation === ""}
                      fallback={
                        <div class="sql-structure-columns-editor">
                          <div class="sql-structure-projection-slot-list">
                            <Index each={projectionSlots()}>
                              {(slot, index) => (
                                <div
                                  class="sql-structure-projection-slot"
                                  data-slot-key={slot().key}
                                >
                                  <label class="ui-field">
                                    <span class="ui-sr-only">
                                      {t("sqlPage.structure.projectionColumn", {
                                        position: index + 1,
                                      })}
                                    </span>
                                    <select
                                      class="ui-input"
                                      value={slot().column}
                                      disabled={busy() || Boolean(proposal())}
                                      aria-label={t(
                                        "sqlPage.structure.projectionColumn",
                                        { position: index + 1 },
                                      )}
                                      onChange={(event) =>
                                        updateProjectionSlot(index, {
                                          column: event.currentTarget.value,
                                        })}
                                    >
                                      <Show
                                        when={!projectableColumns().some((
                                          column,
                                        ) => column.name === slot().column)}
                                      >
                                        <option value={slot().column}>
                                          {slot().column}
                                        </option>
                                      </Show>
                                      <For each={projectableColumns()}>
                                        {(column) => (
                                          <option value={column.name}>
                                            {column.name}
                                          </option>
                                        )}
                                      </For>
                                    </select>
                                  </label>
                                  <label class="ui-field">
                                    <span class="ui-sr-only">
                                      {t("sqlPage.structure.aliasForSlot", {
                                        column: slot().column,
                                        position: index + 1,
                                      })}
                                    </span>
                                    <input
                                      class="ui-input"
                                      type="text"
                                      placeholder={t(
                                        "sqlPage.structure.aliasPlaceholder",
                                      )}
                                      value={slot().alias.name}
                                      aria-label={t(
                                        "sqlPage.structure.aliasForSlot",
                                        {
                                          column: slot().column,
                                          position: index + 1,
                                        },
                                      )}
                                      disabled={busy() || Boolean(proposal())}
                                      onInput={(event) =>
                                        updateProjectionSlot(index, {
                                          alias: {
                                            ...slot().alias,
                                            name: event.currentTarget.value,
                                          },
                                        })}
                                    />
                                  </label>
                                  <label class="sql-structure-option sql-structure-column-alias-quoted">
                                    <input
                                      type="checkbox"
                                      checked={slot().alias.quoted}
                                      disabled={busy() || Boolean(proposal())}
                                      aria-label={t(
                                        "sqlPage.structure.quoteAliasForSlot",
                                        {
                                          column: slot().column,
                                          position: index + 1,
                                        },
                                      )}
                                      onChange={(event) =>
                                        updateProjectionSlot(index, {
                                          alias: {
                                            ...slot().alias,
                                            quoted: event.currentTarget.checked,
                                          },
                                        })}
                                    />
                                    <span>
                                      {t("sqlPage.structure.quotedAlias")}
                                    </span>
                                  </label>
                                  <div class="sql-structure-projection-slot-actions">
                                    <IconButton
                                      icon="arrow-up"
                                      label={t(
                                        "sqlPage.structure.moveProjectionUp",
                                        { position: index + 1 },
                                      )}
                                      disabled={busy() || Boolean(proposal()) ||
                                        index === 0}
                                      onClick={() =>
                                        moveProjectionSlot(index, -1)}
                                    />
                                    <IconButton
                                      icon="arrow-down"
                                      label={t(
                                        "sqlPage.structure.moveProjectionDown",
                                        { position: index + 1 },
                                      )}
                                      disabled={busy() || Boolean(proposal()) ||
                                        index === projectionSlots().length - 1}
                                      onClick={() =>
                                        moveProjectionSlot(index, 1)}
                                    />
                                    <IconButton
                                      icon="trash"
                                      label={t(
                                        "sqlPage.structure.removeProjectionSlot",
                                        { position: index + 1 },
                                      )}
                                      disabled={busy() || Boolean(proposal())}
                                      onClick={() =>
                                        setProjectionSlots((slots) =>
                                          slots.filter((_, slotIndex) =>
                                            slotIndex !== index
                                          )
                                        )}
                                    />
                                  </div>
                                </div>
                              )}
                            </Index>
                          </div>
                          <button
                            type="button"
                            class="ui-button ui-button-secondary"
                            disabled={busy() || Boolean(proposal()) ||
                              projectableColumns().length === 0}
                            onClick={addProjectionSlot}
                          >
                            {t("sqlPage.structure.addProjectionSlot")}
                          </button>
                        </div>
                      }
                    >
                      <fieldset class="sql-structure-literal-list">
                        <legend class="ui-sr-only">
                          {t("sqlPage.structure.columns")}
                        </legend>
                        <For each={literalProjection()}>
                          {(literal, index) => (
                            <div class="sql-structure-literal-row">
                              <select
                                class="ui-input"
                                aria-label={t(
                                  "sqlPage.structure.literalType",
                                  { index: index() + 1 },
                                )}
                                value={literal.kind}
                                disabled={busy() || Boolean(proposal())}
                                onChange={(event) =>
                                  updateLiteralKind(
                                    index(),
                                    event.currentTarget
                                      .value as SqlStructureLiteral["kind"],
                                  )}
                              >
                                <option value="integer">
                                  {t("sqlPage.structure.literal.integer")}
                                </option>
                                <option value="string">
                                  {t("sqlPage.structure.literal.string")}
                                </option>
                                <option value="boolean">
                                  {t("sqlPage.structure.literal.boolean")}
                                </option>
                                <option value="null">
                                  {t("sqlPage.structure.literal.null")}
                                </option>
                              </select>
                              <Show
                                when={literal.kind === "integer" ||
                                  literal.kind === "string"}
                                fallback={
                                  <Show when={literal.kind === "boolean"}>
                                    <select
                                      class="ui-input"
                                      aria-label={t(
                                        "sqlPage.structure.literalValue",
                                        { index: index() + 1 },
                                      )}
                                      value={literal.kind === "boolean" &&
                                          literal.value
                                        ? "true"
                                        : "false"}
                                      disabled={busy() || Boolean(proposal())}
                                      onChange={(event) =>
                                        updateLiteralBoolean(
                                          index(),
                                          event.currentTarget.value === "true",
                                        )}
                                    >
                                      <option value="true">true</option>
                                      <option value="false">false</option>
                                    </select>
                                  </Show>
                                }
                              >
                                <input
                                  class="ui-input"
                                  type="text"
                                  aria-label={t(
                                    "sqlPage.structure.literalValue",
                                    { index: index() + 1 },
                                  )}
                                  value={literal.kind === "integer" ||
                                      literal.kind === "string"
                                    ? literal.value
                                    : ""}
                                  disabled={busy() || Boolean(proposal())}
                                  onInput={(event) =>
                                    updateLiteralValue(
                                      index(),
                                      event.currentTarget.value,
                                    )}
                                />
                              </Show>
                              <IconButton
                                icon="close"
                                label={t("sqlPage.structure.removeLiteral", {
                                  index: index() + 1,
                                })}
                                disabled={busy() || Boolean(proposal())}
                                onClick={() =>
                                  setLiteralProjection((values) =>
                                    values.filter((_, itemIndex) =>
                                      itemIndex !== index()
                                    )
                                  )}
                              />
                            </div>
                          )}
                        </For>
                        <button
                          type="button"
                          class="ui-button ui-button-secondary"
                          disabled={busy() || Boolean(proposal()) ||
                            literalProjection().length >= 256}
                          onClick={() =>
                            setLiteralProjection((values) => [
                              ...values,
                              { kind: "integer", value: "0" },
                            ])}
                        >
                          {t("sqlPage.structure.addLiteral")}
                        </button>
                      </fieldset>
                    </Show>
                  </Show>

                  <Show
                    when={activeAction() === "filter" &&
                      structuredEditorAvailable("filter")}
                  >
                    <div class="sql-structure-filter-list">
                      <For each={filters() ? [filters()!] : []}>
                        {(filter) => renderFilterNode(filter)}
                      </For>
                      <Show when={!filterRootIsGroup()}>
                        <button
                          type="button"
                          class="ui-button ui-button-secondary"
                          data-filter-focus-key="root:add-filter"
                          onClick={addFilter}
                          disabled={busy() || Boolean(proposal()) ||
                            !canAddFilter()}
                        >
                          {t("sqlPage.structure.addFilter")}
                        </button>
                        <button
                          type="button"
                          class="ui-button ui-button-secondary"
                          data-filter-focus-key="root:add-group"
                          onClick={() => addGroup()}
                          disabled={busy() || Boolean(proposal())}
                        >
                          {t("sqlPage.structure.addFilterGroup")}
                        </button>
                      </Show>
                    </div>
                  </Show>

                  <Show
                    when={activeAction() === "sort" &&
                      structuredEditorAvailable("sort")}
                  >
                    <div class="sql-structure-sort-list">
                      <For each={sorts()}>
                        {(sort, index) => (
                          <div class="sql-structure-sort-row">
                            <label class="ui-field">
                              <span>{t("sqlPage.structure.column")}</span>
                              <select
                                class="ui-input"
                                value={sortChoiceValue(sort)}
                                onChange={(event) =>
                                  (() => {
                                    const selected = choiceForValue(
                                      event.currentTarget.value,
                                      sortChoices(),
                                    );
                                    if (!selected) return;
                                    setSorts((items) =>
                                      items.map((item, i) =>
                                        i === index()
                                          ? {
                                            direction: item.direction,
                                            column: selected.column,
                                            ...(selected.qualifier
                                              ? {
                                                qualifier: selected.qualifier,
                                              }
                                              : {}),
                                            ...(selected.projection_alias
                                              ? {
                                                projection_alias: selected
                                                  .projection_alias,
                                              }
                                              : {}),
                                          }
                                          : item
                                      )
                                    );
                                  })()}
                              >
                                <For each={sortChoices()}>
                                  {(choice) => (
                                    <option value={choice.value}>
                                      {choice.label}
                                    </option>
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
                        disabled={!sortChoices().some(
                          (choice) =>
                            !sorts().some((sort) =>
                              sortChoiceValue(sort) === choice.value
                            ),
                        )}
                      >
                        {t("sqlPage.structure.addSort")}
                      </button>
                    </div>
                  </Show>

                  <Show
                    when={activeAction() === "aggregate" &&
                      structuredEditorAvailable("aggregate")}
                  >
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
                          const columns = (column?: string) => {
                            const allowed = aggregateColumnsFor(
                              aggregate.function,
                              aggregate.distinct,
                            ).map((column) => column.name);
                            return column && !allowed.includes(column)
                              ? [column, ...allowed]
                              : allowed;
                          };
                          const distinctable = () => {
                            const aggregateColumns = aggregateColumnsOf(
                              aggregate,
                            );
                            if (aggregateColumns.length === 0) {
                              return aggregate.function === "count";
                            }
                            return aggregateColumns.length === 1 && Boolean(
                              columnCapability(aggregateColumns[0])
                                ?.distinctable,
                            );
                          };
                          const argumentsForEditor = () => {
                            const aggregateColumns = aggregateColumnsOf(
                              aggregate,
                            );
                            return aggregateColumns.length > 0
                              ? aggregateColumns
                              : [""];
                          };
                          return (
                            <div
                              class={`sql-structure-aggregate-row${
                                aggregateColumnsOf(aggregate).length > 0
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
                              <div class="sql-structure-aggregate-arguments">
                                <For each={argumentsForEditor()}>
                                  {(column, argumentIndex) => (
                                    <div class="sql-structure-aggregate-argument">
                                      <label class="ui-field">
                                        <span>
                                          {t("sqlPage.structure.column")}
                                        </span>
                                        <select
                                          class="ui-input"
                                          value={aggregateColumnsOf(aggregate)[
                                            argumentIndex()
                                          ] ?? ""}
                                          disabled={aggregate.function !==
                                              "count" &&
                                            columns(column).length === 0}
                                          onChange={(event) =>
                                            updateAggregateArgument(
                                              index(),
                                              argumentIndex(),
                                              event.currentTarget.value,
                                            )}
                                        >
                                          <Show
                                            when={aggregate.function ===
                                              "count"}
                                          >
                                            <option value="">
                                              {t("sqlPage.structure.allRows")}
                                            </option>
                                          </Show>
                                          <For each={columns(column)}>
                                            {(name) => (
                                              <option value={name}>
                                                {name}
                                              </option>
                                            )}
                                          </For>
                                        </select>
                                      </label>
                                      <Show
                                        when={aggregate.function === "count" &&
                                          aggregateColumnsOf(aggregate).length >
                                            0}
                                      >
                                        <IconButton
                                          icon="trash"
                                          label={t(
                                            "sqlPage.structure.removeAggregateArgument",
                                          )}
                                          onClick={() =>
                                            removeAggregateArgument(
                                              index(),
                                              argumentIndex(),
                                            )}
                                        />
                                      </Show>
                                    </div>
                                  )}
                                </For>
                              </div>
                              <Show when={aggregate.function === "count"}>
                                <IconButton
                                  icon="plus"
                                  label={t(
                                    "sqlPage.structure.addAggregateArgument",
                                  )}
                                  disabled={aggregateColumnsFor("count")
                                    .length === 0}
                                  onClick={() => addAggregateArgument(index())}
                                />
                              </Show>
                              <Show
                                when={aggregate.function === "count" ||
                                  aggregateColumnsOf(aggregate).length === 1}
                              >
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

                  <Show
                    when={activeAction() === "group_by" &&
                      structuredEditorAvailable("group_by")}
                  >
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

                  <Show
                    when={activeAction() === "distinct" &&
                      structuredEditorAvailable("distinct")}
                  >
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
                  <Show
                    when={activeAction() !== null &&
                      activeAction() !== "form" &&
                      structuredEditorAvailable(activeAction()!)}
                  >
                    <button
                      type="button"
                      class="ui-button ui-button-primary"
                      onClick={() => void applyOperation()}
                      disabled={busy() || (
                        activeAction() === "columns" &&
                        !projectionEditValid()
                      ) ||
                        (activeAction() === "join" &&
                          structureEditorMode() === "join" &&
                          !joinOperationValid()) ||
                        (activeAction() === "join" &&
                          structureEditorMode() === "row_limit" &&
                          (!rowLimitDraftValid() ||
                            !rowLimitDraftChanged())) ||
                        (activeAction() === "filter" && !filterRowsValid()) ||
                        (activeAction() === "sort" && !sortRowsValid()) ||
                        (activeAction() === "aggregate" &&
                          !aggregateOperationValid()) ||
                        (activeAction() === "group_by" &&
                          !groupByRowsValid()) ||
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
                  </Show>
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
