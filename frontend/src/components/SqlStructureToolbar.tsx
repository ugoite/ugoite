import {
  createEffect,
  createSignal,
  createUniqueId,
  For,
  Index,
  Show,
} from "solid-js";
import { IconButton } from "~/components/IconButton";
import { UiIcon } from "~/components/UiIcon";
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
  direction: "asc" | "desc";
};
type ColumnChoice = {
  column: string;
  qualifier?: SqlStructureAlias;
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

interface SqlStructureToolbarProps {
  sql: () => string;
  contexts: () => readonly SqlStructureAdapterContext[];
  onSqlChange?: (sql: string) => void;
  onVariablesAdded?: (
    variables: readonly { name: string; type: SqlStructureVariableType }[],
  ) => boolean | void;
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
    setProposalVariables([]);
    setProposalContextIdentity(undefined);
    setActiveAction(null);
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
      // boolean-tree capability when it is available.
      return current?.capabilities.filter_expression ??
        current?.capabilities.filter;
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
    if (action === "form") {
      return Boolean(
        props.disabled?.() || !props.onSqlChange ||
          analysisState().status !== "ready" || formOptions().length === 0,
      );
    }
    return Boolean(
      props.disabled?.() || !props.onSqlChange ||
        analysisState().status !== "ready" ||
        ready()?.analysis.status !== "supported" ||
        !capability(action)?.enabled || !hasEditableCapabilities(action),
    );
  };

  const actionReason = (action: StructureAction) => {
    if (action === "form") {
      if (props.disabled?.() || !props.onSqlChange) {
        return t("sqlPage.structureReadOnly");
      }
      const state = analysisState();
      if (state.status === "loading") return t("sqlPage.structureAnalyzing");
      if (state.status === "error") return t("sqlPage.structureUnavailable");
      if (state.status === "no_context") return t("sqlPage.structureNoForm");
      return formOptions().length === 0
        ? t("sqlPage.structureUnavailable")
        : undefined;
    }
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
      return columns.some((left) =>
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
    if (action === "form") {
      if (actionDisabled(action)) return;
      setError(null);
      setProposal(undefined);
      setProposalContextIdentity(undefined);
      setActiveAction(action);
      return;
    }
    const current = ready();
    if (!current || actionDisabled(action)) return;
    setError(null);
    setProposal(undefined);
    setProposalContextIdentity(undefined);
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

  const renderFilterNode = (filter: FilterDraft) => {
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
        <div class="sql-structure-filter-node" data-filter-node={filter.id}>
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
                onClick={() =>
                  addNot(filter.id)}
              >
                NOT
              </button>
              <IconButton
                icon="trash"
                label={t("sqlPage.structure.removeFilter")}
                disabled={busy() || Boolean(proposal())}
                onClick={() =>
                  removeFilter(filter.id)}
              />
            </div>
          </div>
        </div>
      );
    }

    if (filter.kind === "not") {
      return (
        <div class="sql-structure-filter-not" data-filter-node={filter.id}>
          <span class="sql-structure-filter-operator">NOT</span>
          {renderFilterNode(filter.expression)}
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
        class="sql-structure-filter-group"
        role="group"
        aria-label={t("sqlPage.structure.filterGroup")}
        data-filter-node={filter.id}
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
          <For each={filter.expressions}>{renderFilterNode}</For>
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
      sortableColumns().some((column) => column.name === sort.column) &&
      (activeJoin()
        ? [activeJoin()?.left_alias, activeJoin()?.right_alias].some((alias) =>
          alias?.name === sort.qualifier?.name &&
          alias.quoted === sort.qualifier?.quoted
        )
        : !sort.qualifier)
    );

  const addSort = () => {
    const choices = columnChoices(sortableColumns());
    const available = choices.find((choice) =>
      !sorts().some((sort) =>
        selectedColumnChoice(sort.column, sort.qualifier) === choice.value
      )
    );
    if (!available) return;
    setSorts((items) => [...items, {
      column: available.column,
      ...(available.qualifier ? { qualifier: available.qualifier } : {}),
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
    if (
      !current || !action || action === "form" || actionDisabled(action) ||
      busy()
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
    const operation: SqlStructureOperation = action === "columns"
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
      action === "join" && !joinOperationValid() ||
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
    const createdVariables = proposalVariables();
    if (createdVariables.length > 0) {
      if (props.onVariablesAdded?.(createdVariables) === false) {
        setError(t("sqlPage.structureUnavailable"));
        return;
      }
    }
    props.onSqlChange(current.proposed_sql);
    setProposal(undefined);
    setProposalVariables([]);
    setProposalContextIdentity(undefined);
    setActiveAction(null);
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
                  <Show when={activeAction() === "form"}>
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

                  <Show when={activeAction() === "columns"}>
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

                  <Show when={activeAction() === "join"}>
                    <div class="sql-structure-join-fields">
                      <label class="ui-field">
                        <span>{t("sqlPage.structure.leftColumn")}</span>
                        <select
                          class="ui-input"
                          value={joinLeftColumn()}
                          disabled={busy() || Boolean(proposal())}
                          onChange={(event) => {
                            const value = event.currentTarget.value;
                            const selectedType = ready()?.context.parser.columns
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
                              <option value={column.name}>{column.name}</option>
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
                            const selectedType = ready()?.context.parser.columns
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
                              <option value={column.name}>{column.name}</option>
                            )}
                          </For>
                        </select>
                      </label>
                    </div>
                  </Show>

                  <Show when={activeAction() === "filter"}>
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

                  <Show when={activeAction() === "sort"}>
                    <div class="sql-structure-sort-list">
                      <For each={sorts()}>
                        {(sort, index) => (
                          <div class="sql-structure-sort-row">
                            <label class="ui-field">
                              <span>{t("sqlPage.structure.column")}</span>
                              <select
                                class="ui-input"
                                value={selectedColumnChoice(
                                  sort.column,
                                  sort.qualifier,
                                )}
                                onChange={(event) =>
                                  (() => {
                                    const selected = choiceForValue(
                                      event.currentTarget.value,
                                      columnChoices(sortableColumns()),
                                    );
                                    if (!selected) return;
                                    setSorts((items) =>
                                      items.map((item, i) =>
                                        i === index()
                                          ? {
                                            ...item,
                                            column: selected.column,
                                            qualifier: selected.qualifier,
                                          }
                                          : item
                                      )
                                    );
                                  })()}
                              >
                                <For each={columnChoices(sortableColumns())}>
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
                        disabled={!columnChoices(sortableColumns()).some(
                          (choice) =>
                            !sorts().some((sort) =>
                              selectedColumnChoice(
                                sort.column,
                                sort.qualifier,
                              ) ===
                                choice.value
                            ),
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
                              <label class="ui-field">
                                <span>{t("sqlPage.structure.column")}</span>
                                <select
                                  class="ui-input"
                                  value={aggregate.column ?? ""}
                                  disabled={aggregate.function !== "count" &&
                                    columns().length === 0}
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
                  <Show when={activeAction() !== "form"}>
                    <button
                      type="button"
                      class="ui-button ui-button-primary"
                      onClick={() => void applyOperation()}
                      disabled={busy() || (
                        activeAction() === "columns" &&
                        !projectionEditValid()
                      ) ||
                        (activeAction() === "join" && !joinOperationValid()) ||
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
