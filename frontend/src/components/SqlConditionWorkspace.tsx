import {
  type Accessor,
  createEffect,
  createSignal,
  For,
  onCleanup,
  type Setter,
  Show,
  untrack,
} from "solid-js";
import type {
  SqlStructureAdapterContext,
  SqlStructureFilterOperator,
} from "~/lib/sql";
import {
  applySqlDocumentEditTransaction,
  applySqlDocumentSemanticEditTransaction,
  parseSqlDocument,
  type SqlDocumentConditionNode,
  type SqlDocumentConditionScope,
  type SqlDocumentEditTransaction,
  type SqlDocumentFilterCondition,
  type SqlDocumentFilterOperator,
  type SqlDocumentIdentifier,
  type SqlDocumentNodeKind,
  type SqlDocumentSemanticEdit,
  type SqlDocumentSemanticLiteral,
} from "~/lib/ugoite-client";
import type { SqlVariable } from "~/lib/types";
import { parseSqlVariableDefault } from "~/lib/sql";
import { t } from "~/lib/i18n";
import { IconButton } from "~/components/IconButton";

type Binding = { name: string; variableId: string };
type Props = {
  sql: () => string;
  scope?: () => string;
  disabled?: () => boolean;
  contexts: () => readonly SqlStructureAdapterContext[];
  onSqlChange: (sql: string) => void;
  onAdvanced: () => void;
  argumentDefinitions?: () => readonly SqlVariable[];
  onArgumentAdded?: (variable: SqlVariable) => boolean | void;
  scalarVariables?: readonly { id: string; name: string; type: string }[];
  argumentBindings?: () => readonly Binding[];
  reservedVariableNames?: () => readonly string[];
  historyState?: SqlConditionWorkspaceHistoryState;
  onFilterEdit?: (change: {
    expectedSql: string;
    expectedScope: string | undefined;
    resultSql: string;
    createdArgument?: SqlVariable;
    binding?: Binding;
    expectedBindings?: readonly Binding[];
    resultBindings?: readonly Binding[];
    expectedArguments?: readonly SqlVariable[];
    resultArguments?: readonly SqlVariable[];
  }) => boolean | void;
};
type History = {
  undo: SqlDocumentEditTransaction;
  beforeBindings: readonly Binding[];
  afterBindings: readonly Binding[];
  beforeArguments: readonly SqlVariable[];
  afterArguments: readonly SqlVariable[];
};
export type SqlConditionWorkspaceHistoryEntry = History;
export type SqlConditionWorkspaceHistoryState = {
  history: Accessor<History[]>;
  setHistory: Setter<History[]>;
  redo: Accessor<History[]>;
  setRedo: Setter<History[]>;
};
type LeafDraft = {
  column: string;
  operator: SqlDocumentFilterOperator;
  source: "literal" | "argument";
  value: string;
  valueKind: "string" | "number" | "boolean" | "null";
  argument: string;
};
type GroupTarget = { parentNodeId: number | null; queryNodeId: number };

function selectAncestor(
  nodes: readonly { id: number; parent: number | null; kind: string }[],
  nodeId: number,
) {
  let current = nodes.find((node) => node.id === nodeId);
  while (current) {
    if (current.kind === "select") return current.id;
    current = nodes.find((node) => node.id === current?.parent);
  }
  return undefined;
}

const supportedOperators: readonly SqlDocumentFilterOperator[] = [
  "eq",
  "not_eq",
  "lt",
  "lte",
  "gt",
  "gte",
  "like",
  "not_like",
  "is_null",
  "is_not_null",
];
const operatorName = (operator: SqlDocumentFilterOperator) => {
  const labels: Record<SqlDocumentFilterOperator, () => string> = {
    eq: () => t("sqlPage.structure.operator.eq"),
    not_eq: () => t("sqlPage.structure.operator.not_eq"),
    lt: () => t("sqlPage.structure.operator.lt"),
    lte: () => t("sqlPage.structure.operator.lt_eq"),
    gt: () => t("sqlPage.structure.operator.gt"),
    gte: () => t("sqlPage.structure.operator.gt_eq"),
    like: () => t("sqlPage.structure.operator.like"),
    not_like: () => t("sqlPage.structure.operator.not_like"),
    is_null: () => t("sqlPage.structure.operator.is_null"),
    is_not_null: () => t("sqlPage.structure.operator.is_not_null"),
  };
  return labels[operator]();
};
const valueInputOperators = new Set<SqlDocumentFilterOperator>([
  "eq",
  "not_eq",
  "lt",
  "lte",
  "gt",
  "gte",
  "like",
  "not_like",
]);
const isGroup = (
  node: SqlDocumentConditionNode,
): node is Extract<SqlDocumentConditionNode, { kind: "group" }> =>
  node.kind === "group";
const isComparison = (
  node: SqlDocumentConditionNode,
): node is Extract<SqlDocumentConditionNode, { kind: "comparison" }> =>
  node.kind === "comparison";

/** Beginner-facing editor for the parser-derived WHERE condition tree. */
export function SqlConditionWorkspace(props: Props) {
  const [conditions, setConditions] = createSignal<
    readonly SqlDocumentConditionScope[]
  >([]);
  const [selectIdsByQuery, setSelectIdsByQuery] = createSignal<
    Record<number, number>
  >({});
  const [nodeKindsById, setNodeKindsById] = createSignal<
    Record<number, SqlDocumentNodeKind>
  >({});
  const [qualifierByQuery, setQualifierByQuery] = createSignal<
    Record<number, SqlDocumentIdentifier | null>
  >({});
  const [relationsByQuery, setRelationsByQuery] = createSignal<
    Record<number, readonly string[]>
  >({});
  const [supportedSelectId, setSupportedSelectId] = createSignal<number>();
  const [drafts, setDrafts] = createSignal<Record<number, LeafDraft>>({});
  const [groupDrafts, setGroupDrafts] = createSignal<
    Record<number, "and" | "or">
  >({});
  const [editingNodeId, setEditingNodeId] = createSignal<number | null>(null);
  const [newColumn, setNewColumn] = createSignal("");
  const [newOperator, setNewOperator] = createSignal<SqlDocumentFilterOperator>(
    "eq",
  );
  const [newValue, setNewValue] = createSignal("");
  const [newValueKind, setNewValueKind] = createSignal<
    "string" | "number" | "boolean"
  >("string");
  const [newValueSource, setNewValueSource] = createSignal<
    "literal" | "argument"
  >("literal");
  const [newArgument, setNewArgument] = createSignal("");
  const [newArgumentName, setNewArgumentName] = createSignal("");
  const [newArgumentDefault, setNewArgumentDefault] = createSignal("");
  const [newBinding, setNewBinding] = createSignal("");
  const [addTarget, setAddTarget] = createSignal<GroupTarget | null>(null);
  const [groupTarget, setGroupTarget] = createSignal<GroupTarget | null>(null);
  const [groupConnector, setGroupConnector] = createSignal<"and" | "or">("and");
  const [groupSecondColumn, setGroupSecondColumn] = createSignal("");
  const [groupSecondOperator, setGroupSecondOperator] = createSignal<
    SqlDocumentFilterOperator
  >("eq");
  const [groupSecondValue, setGroupSecondValue] = createSignal("");
  const [groupSecondSource, setGroupSecondSource] = createSignal<
    "literal" | "argument"
  >("literal");
  const [groupSecondValueKind, setGroupSecondValueKind] = createSignal<
    "string" | "number" | "boolean"
  >("string");
  const [groupSecondArgument, setGroupSecondArgument] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [localHistory, setLocalHistory] = createSignal<History[]>([]);
  const [localRedo, setLocalRedo] = createSignal<History[]>([]);
  const history = props.historyState?.history ?? localHistory;
  const setHistory = props.historyState?.setHistory ?? setLocalHistory;
  const redo = props.historyState?.redo ?? localRedo;
  const setRedo = props.historyState?.setRedo ?? setLocalRedo;
  let generation = 0;
  let lastConditionFocusKey: string | undefined;
  let pendingConditionFocusKey: string | undefined;
  onCleanup(() => {
    generation++;
  });
  let priorScope = props.scope?.();
  let priorSource = props.sql();
  let nextInternalSource: string | undefined;
  const locked = () => props.disabled?.() === true;
  const current = (
    request: number,
    source: string,
    scope: string | undefined,
  ) =>
    request === generation && props.sql() === source &&
    props.scope?.() === scope && !locked();

  createEffect(() => {
    const source = props.sql();
    const scope = props.scope?.();
    if (source !== priorSource) {
      const internal = source === nextInternalSource;
      nextInternalSource = undefined;
      if (!internal) {
        setDrafts({});
        setGroupDrafts({});
        setGroupTarget(null);
        setAddTarget(null);
        setEditingNodeId(null);
      }
      priorSource = source;
    }
    if (scope !== priorScope) {
      priorScope = scope;
      setHistory([]);
      setRedo([]);
      setDrafts({});
      setGroupDrafts({});
      setGroupDrafts({});
      setGroupTarget(null);
      setAddTarget(null);
      setEditingNodeId(null);
    } else if (
      untrack(history).at(-1)?.undo.expected_source !== source &&
      untrack(redo).at(-1)?.undo.expected_source !== source
    ) {
      setHistory([]);
      setRedo([]);
    }
    let active = true;
    onCleanup(() => {
      active = false;
    });
    void parseSqlDocument(source).then((document) => {
      if (!active || props.sql() !== source || props.scope?.() !== scope) {
        return;
      }
      if (!document.conditions) {
        throw new Error(t("sqlPage.structure.documentParseFailed"));
      }
      const view = document.conditions;
      setConditions(view);
      setNodeKindsById(
        Object.fromEntries(document.nodes.map((node) => [node.id, node.kind])),
      );
      const selectIds: Record<number, number> = {};
      for (const node of document.nodes) {
        if (node.kind === "select") {
          selectIds[node.id] = node.id;
        }
      }
      setSelectIdsByQuery(selectIds);
      const qualifiers: Record<number, SqlDocumentIdentifier | null> = {};
      const relations: Record<number, string[]> = {};
      for (const node of document.nodes) {
        if (
          node.kind === "table_factor" && node.semantic?.kind === "table_factor"
        ) {
          const selectId = selectAncestor(document.nodes, node.id);
          if (selectId !== undefined) {
            qualifiers[selectId] = node.semantic.alias;
          }
        }
        if (node.kind === "relation" && node.semantic?.kind === "relation") {
          const selectId = selectAncestor(document.nodes, node.id);
          if (selectId !== undefined) {
            const names = node.semantic.name.map((part) => part.value);
            const name = names.join(".");
            if (name) {
              relations[selectId] = [...(relations[selectId] ?? []), name];
            }
          }
        }
      }
      setQualifierByQuery(qualifiers);
      setRelationsByQuery(relations);
      const selects = document.nodes.filter((node) => node.kind === "select");
      const factors = document.nodes.filter((node) =>
        node.kind === "table_factor"
      );
      const queryCount = document.nodes.filter((node) =>
        node.kind === "query"
      ).length;
      const singleSelectId = selects.length === 1 ? selects[0].id : undefined;
      const singleSelectRelations = singleSelectId === undefined
        ? []
        : relations[singleSelectId] ?? [];
      setSupportedSelectId(
        queryCount === 1 && selects.length === 1 && factors.length === 1 &&
          !document.nodes.some((node) => node.kind === "with_clause") &&
          singleSelectRelations.length === 1
          ? singleSelectId
          : undefined,
      );
      const emptyRoot = queryCount === 1 && selects.length === 1 &&
          factors.length === 1 &&
          !document.nodes.some((node) => node.kind === "with_clause") &&
          singleSelectRelations.length === 1
        ? view.find((item) =>
          item.query_node_id === singleSelectId && item.expression === null
        )
        : undefined;
      if (emptyRoot && !addTarget()) {
        setAddTarget({
          parentNodeId: null,
          queryNodeId: emptyRoot.query_node_id,
        });
      } else if (
        addTarget() &&
        !view.some((item) => item.query_node_id === addTarget()?.queryNodeId)
      ) {
        setAddTarget(null);
      }
      const ids = new Set<number>();
      const visit = (node: SqlDocumentConditionNode | null) => {
        if (!node) return;
        ids.add(node.node_id);
        if (isGroup(node)) node.children.forEach(visit);
      };
      view.forEach((item) => visit(item.expression));
      setDrafts((old) =>
        Object.fromEntries(
          Object.entries(old).filter(([id]) => ids.has(Number(id))),
        )
      );
      setGroupDrafts((old) =>
        Object.fromEntries(
          Object.entries(old).filter(([id]) => ids.has(Number(id))),
        )
      );
      const focusKey = pendingConditionFocusKey;
      pendingConditionFocusKey = undefined;
      if (focusKey) {
        setTimeout(() => {
          globalThis.document?.querySelector<HTMLElement>(
            `[data-condition-focus="${focusKey}"]`,
          )?.focus();
        });
      }
    }).catch((cause) => {
      if (active && props.sql() === source && props.scope?.() === scope) {
        setError(
          cause instanceof Error
            ? cause.message
            : t("sqlPage.structure.documentParseFailed"),
        );
      }
    });
  });
  const contextFor = (scope: SqlDocumentConditionScope) => {
    if (scope.query_node_id !== supportedSelectId()) return undefined;
    const relationNames = [
      ...new Set(relationsByQuery()[scope.query_node_id] ?? []),
    ];
    if (relationNames.length !== 1) return undefined;
    const parts = relationNames[0].split(".");
    const basename = parts.at(-1);
    const matches = props.contexts().filter((candidate) =>
      candidate.parser.relation === relationNames[0] ||
      candidate.stableRelation === relationNames[0] ||
      (parts.length === 1 && candidate.parser.relation === basename)
    );
    return matches.length === 1 ? matches[0] : undefined;
  };
  const columnLabel = (scope: SqlDocumentConditionScope, name: string) =>
    contextFor(scope)?.columnLabels?.[name] ??
      (name === "_ugoite_id"
        ? t("entryInfo.entryId")
        : name === "_ugoite_created_at"
        ? t("entryInfo.createdAt")
        : name === "_ugoite_updated_at"
        ? t("entryInfo.updatedAt")
        : name);
  const columns = (scope: SqlDocumentConditionScope) =>
    contextFor(scope)?.parser.columns ?? [];
  const filterColumns = (scope: SqlDocumentConditionScope) =>
    columns(scope).filter((item) =>
      contextFor(scope)?.columnCapabilities[item.name]?.filterable === true
    );
  const operatorOptions = (name: string, scope: SqlDocumentConditionScope) => {
    const supported = new Set<SqlStructureFilterOperator>(
      contextFor(scope)?.columnCapabilities[name]?.supportedOperators ?? [],
    );
    const mapping: Record<
      SqlDocumentFilterOperator,
      SqlStructureFilterOperator
    > = {
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
    return supportedOperators.filter((item) => supported.has(mapping[item]));
  };
  const typeFor = (name: string, scope: SqlDocumentConditionScope) => {
    const raw = columns(scope).find((column) => column.name === name)?.type
      ?.toLowerCase();
    if (["string", "text", "varchar", "char"].includes(raw ?? "")) {
      return "string";
    }
    if (["int", "integer", "bigint", "smallint"].includes(raw ?? "")) {
      return "integer";
    }
    if (["float", "double", "real"].includes(raw ?? "")) return "float";
    if (["bool", "boolean"].includes(raw ?? "")) return "boolean";
    if (raw === "date") return "date";
    if (["timestamp", "datetime"].includes(raw ?? "")) return "timestamp";
    return undefined;
  };
  const argumentsFor = (column: string, scope: SqlDocumentConditionScope) =>
    (props.argumentDefinitions?.() ?? []).filter((item) =>
      item.type === typeFor(column, scope)
    );
  const scalarsFor = (column: string, scope: SqlDocumentConditionScope) =>
    (props.scalarVariables ?? []).filter((item) =>
      item.type === typeFor(column, scope)
    );
  const isValidDraft = (
    draft: LeafDraft,
    canCreateArgument: boolean,
    scope: SqlDocumentConditionScope,
  ) => {
    if (!filterColumns(scope).some((item) => item.name === draft.column)) {
      return false;
    }
    if (!operatorOptions(draft.column, scope).includes(draft.operator)) {
      return false;
    }
    if (!valueInputOperators.has(draft.operator)) return true;
    if (draft.source === "argument") {
      if (
        argumentsFor(draft.column, scope).some((item) =>
          item.name === draft.argument
        )
      ) return true;
      if (!canCreateArgument) return false;
      const name = newArgumentName().trim();
      if (!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(name)) return false;
      if (
        (props.argumentDefinitions?.() ?? []).some((item) =>
          item.name === name
        ) || (props.reservedVariableNames?.() ?? []).includes(name)
      ) return false;
      if (
        props.scalarVariables &&
        !scalarsFor(draft.column, scope).some((item) =>
          item.id === newBinding()
        )
      ) return false;
      if (
        newArgumentDefault() &&
        !parseSqlVariableDefault(
          typeFor(draft.column, scope) ?? "",
          newArgumentDefault(),
        ).valid
      ) return false;
      return true;
    }
    if (draft.valueKind === "number") {
      return /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(
        draft.value.trim(),
      );
    }
    if (draft.valueKind === "boolean") {
      return draft.value === "true" || draft.value === "false";
    }
    if (draft.valueKind === "null") return true;
    return true;
  };
  const isRevokedDraft = (draft: LeafDraft, scope: SqlDocumentConditionScope) =>
    columns(scope).some((item) => item.name === draft.column) &&
    contextFor(scope)?.columnCapabilities[draft.column]?.filterable !== true;
  const draftFor = (
    node: Extract<SqlDocumentConditionNode, { kind: "comparison" }>,
  ): LeafDraft => {
    const saved = drafts()[node.node_id];
    if (saved) return saved;
    const col = node.column.column.value;
    const rawValue = node.value;
    const source = rawValue?.kind === "named_parameter"
      ? "argument"
      : "literal";
    const valueKind = rawValue?.kind === "number"
      ? "number"
      : rawValue?.kind === "boolean"
      ? "boolean"
      : rawValue?.kind === "null"
      ? "null"
      : "string";
    return {
      column: col,
      operator: node.operator,
      source,
      value: rawValue?.kind === "named_parameter"
        ? ""
        : rawValue?.kind === "string" || rawValue?.kind === "number"
        ? rawValue.value
        : rawValue?.kind === "boolean"
        ? String(rawValue.value)
        : "",
      valueKind,
      argument: rawValue?.kind === "named_parameter" ? rawValue.name : "",
    };
  };
  const makeCondition = (
    draft: LeafDraft,
    addArgument: boolean,
    scope: SqlDocumentConditionScope,
    qualifier: SqlDocumentIdentifier | null = null,
  ): {
    condition: SqlDocumentFilterCondition;
    created?: SqlVariable;
    binding?: Binding;
    nextArgs: readonly SqlVariable[];
    nextBindings: readonly Binding[];
  } => {
    const selectedColumn = filterColumns(scope).find((item) =>
      item.name === draft.column
    );
    if (!selectedColumn) {
      throw new Error(t("sqlPage.structure.documentChooseColumn"));
    }
    if (!operatorOptions(draft.column, scope).includes(draft.operator)) {
      throw new Error(t("sqlPage.structure.documentNoOperationTarget"));
    }
    const priorArgs = props.argumentDefinitions?.() ?? [];
    const priorBindings = props.argumentBindings?.() ?? [];
    let literal: SqlDocumentSemanticLiteral | null = null;
    let created: SqlVariable | undefined;
    let binding: Binding | undefined;
    if (valueInputOperators.has(draft.operator)) {
      if (draft.source === "argument") {
        const existing = argumentsFor(draft.column, scope).find((item) =>
          item.name === draft.argument
        );
        if (existing) {
          literal = { kind: "named_parameter", name: existing.name };
        } else if (addArgument) {
          const name = newArgumentName().trim();
          if (
            !/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(name) ||
            priorArgs.some((item) => item.name === name) ||
            (props.reservedVariableNames?.() ?? []).includes(name)
          ) throw new Error(t("sqlPage.structure.argumentNameInvalid"));
          const type = scalarsFor(draft.column, scope).find((item) =>
            item.id === newBinding()
          )?.type ?? typeFor(draft.column, scope);
          if (!type) {
            throw new Error(
              t("sqlPage.structure.filterArgumentTypeUnsupported"),
            );
          }
          const parsedDefault = newArgumentDefault()
            ? parseSqlVariableDefault(type, newArgumentDefault())
            : undefined;
          if (newArgumentDefault() && !parsedDefault?.valid) {
            throw new Error(
              t("sqlPage.structure.filterArgumentDefaultInvalid"),
            );
          }
          created = {
            name,
            type,
            description: "",
            ...(parsedDefault?.valid ? { default: parsedDefault.value } : {}),
          };
          literal = { kind: "named_parameter", name };
          if (newBinding()) {
            binding = { name, variableId: newBinding() };
          }
        } else throw new Error(t("sqlPage.structure.documentChooseColumn"));
      } else if (draft.valueKind === "number") {
        if (!draft.value.trim()) {
          throw new Error(t("sqlPage.structure.documentEnterValue"));
        }
        literal = { kind: "number", value: draft.value };
      } else if (draft.valueKind === "boolean") {
        if (draft.value !== "true" && draft.value !== "false") {
          throw new Error(t("sqlPage.structure.documentEnterValue"));
        }
        literal = { kind: "boolean", value: draft.value === "true" };
      } else if (draft.valueKind === "null") literal = { kind: "null" };
      else literal = { kind: "string", value: draft.value };
    }
    const columnRef = {
      ...(qualifier ? { qualifier } : {}),
      column: {
        value: selectedColumn.name,
        quote_style: /^[A-Za-z_][A-Za-z0-9_$]*$/.test(selectedColumn.name)
          ? null
          : '"',
      },
    };
    const condition: SqlDocumentFilterCondition = {
      kind: "comparison",
      column: columnRef,
      operator: draft.operator,
      value: literal,
    };
    return {
      condition,
      created,
      binding,
      nextArgs: created ? [...priorArgs, created] : priorArgs,
      nextBindings: binding ? [...priorBindings, binding] : priorBindings,
    };
  };

  const commit = async (
    edits: SqlDocumentSemanticEdit[],
    created?: SqlVariable,
    binding?: Binding,
    nextArgs?: readonly SqlVariable[],
    nextBindings?: readonly Binding[],
    restoreFocusKey?: string,
  ) => {
    if (busy() || locked()) return;
    const source = props.sql();
    const scope = props.scope?.();
    const request = generation;
    if (props.sql() !== source || locked()) return;
    setBusy(true);
    setError(undefined);
    const beforeArgs = props.argumentDefinitions?.() ?? [];
    const beforeBindings = props.argumentBindings?.() ?? [];
    const focusKey = typeof document !== "undefined" &&
        document.activeElement instanceof HTMLElement
      ? document.activeElement.dataset.conditionFocus
      : undefined;
    try {
      const parsedDocument = await parseSqlDocument(source);
      if (!current(request, source, scope)) return;
      const result = await applySqlDocumentSemanticEditTransaction(source, {
        contract_version: parsedDocument.contract_version,
        expected_source: source,
        edits,
      });
      if (!current(request, source, scope)) return;
      if (created && !props.onFilterEdit && !props.onArgumentAdded) {
        throw new Error(t("sqlPage.structure.filterArgumentInvalid"));
      }
      nextInternalSource = result.document.source;
      if (props.onFilterEdit) {
        const accepted = props.onFilterEdit({
          expectedSql: source,
          expectedScope: scope,
          resultSql: result.document.source,
          createdArgument: created,
          binding,
          expectedBindings: beforeBindings,
          resultBindings: nextBindings ?? beforeBindings,
          expectedArguments: beforeArgs,
          resultArguments: nextArgs ?? beforeArgs,
        });
        if (accepted === false) {
          nextInternalSource = undefined;
          throw new Error(t("sqlPage.structure.editFailed"));
        }
      } else {
        if (created && props.onArgumentAdded?.(created) === false) {
          nextInternalSource = undefined;
          throw new Error(t("sqlPage.structure.editFailed"));
        }
        props.onSqlChange(result.document.source);
      }
      setHistory(
        [...history(), {
          undo: result.undo,
          beforeBindings,
          afterBindings: nextBindings ?? beforeBindings,
          beforeArguments: beforeArgs,
          afterArguments: nextArgs ?? beforeArgs,
        }].slice(-32),
      );
      setRedo([]);
      const targetFocusKey = focusKey ?? lastConditionFocusKey ??
        restoreFocusKey;
      // Applying drafts clears their local controls after this async call. Let
      // that caller restore focus after the controls have been reconciled.
      pendingConditionFocusKey = restoreFocusKey ? undefined : targetFocusKey;
    } catch (cause) {
      if (current(request, source, scope)) {
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

  const applyDrafts = async () => {
    const edits: SqlDocumentSemanticEdit[] = [];
    let restoreFocusKey: string | undefined;
    for (const scope of conditions()) {
      const visit = (node: SqlDocumentConditionNode | null): void => {
        if (!node) return;
        if (isGroup(node)) {
          const operator = groupDrafts()[node.node_id];
          if (operator && operator !== node.operator) {
            restoreFocusKey ??= lastConditionFocusKey ??
              `${node.node_id}:group-operator`;
            edits.push({
              kind: "set_filter_group_operator",
              node_id: node.node_id,
              expected_kind: nodeKindsById()[node.node_id] ??
                "binary_expression",
              operator,
            });
          }
          node.children.forEach(visit);
        } else if (isComparison(node) && drafts()[node.node_id]) {
          restoreFocusKey ??= lastConditionFocusKey ?? `${node.node_id}:column`;
          const built = makeCondition(
            draftFor(node),
            false,
            scope,
            node.column.qualifier ?? null,
          );
          edits.push({
            kind: "update_filter_condition",
            node_id: node.node_id,
            expected_kind: nodeKindsById()[node.node_id] ?? "binary_expression",
            condition: built.condition,
          });
        }
      };
      visit(scope.expression);
    }
    if (edits.length) {
      await commit(
        edits,
        undefined,
        undefined,
        undefined,
        undefined,
        restoreFocusKey,
      );
      setDrafts({});
      setGroupDrafts({});
      if (restoreFocusKey) {
        setTimeout(() => {
          globalThis.document?.querySelector<HTMLElement>(
            `[data-condition-focus="${restoreFocusKey}"]`,
          )?.focus();
        });
      }
    }
  };

  const addCondition = async (
    parentNodeId: number | null,
    scope: SqlDocumentConditionScope,
  ) => {
    try {
      const draft: LeafDraft = {
        column: newColumn(),
        operator: newOperator(),
        source: newValueSource(),
        value: newValue(),
        valueKind: newValueKind(),
        argument: newArgument(),
      };
      const built = makeCondition(
        draft,
        newValueSource() === "argument" && !newArgument(),
        scope,
        qualifierByQuery()[scope.query_node_id] ?? null,
      );
      const targetKind: SqlDocumentNodeKind = parentNodeId === null
        ? "select"
        : nodeKindsById()[parentNodeId] ?? "binary_expression";
      const edit: SqlDocumentSemanticEdit =
        parentNodeId === null && built.condition.kind === "comparison"
          ? {
            kind: "add_filter_comparison",
            node_id: selectIdsByQuery()[scope.query_node_id],
            expected_kind: "select",
            column: built.condition.column,
            operator: built.condition.operator,
            literal: built.condition.value,
          }
          : {
            kind: "insert_filter_condition",
            node_id: parentNodeId ?? selectIdsByQuery()[scope.query_node_id],
            expected_kind: targetKind,
            condition: built.condition,
          };
      await commit(
        [edit],
        built.created,
        built.binding,
        built.nextArgs,
        built.nextBindings,
      );
      setAddTarget(null);
      setNewValue("");
      setNewArgument("");
      setNewArgumentName("");
      setNewArgumentDefault("");
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : t("sqlPage.structure.editFailed"),
      );
    }
  };
  const removeNode = (nodeId: number) =>
    void commit([{
      kind: "remove_filter_condition",
      node_id: nodeId,
      expected_kind: nodeKindsById()[nodeId] ?? "binary_expression",
    }]);
  const openGroupComposer = (
    scope: SqlDocumentConditionScope,
    parentNodeId: number | null = null,
  ) => {
    const target = { parentNodeId, queryNodeId: scope.query_node_id };
    setAddTarget(target);
    setGroupTarget(target);
    setGroupConnector("and");
    setGroupSecondColumn("");
    setGroupSecondValue("");
    setGroupSecondArgument("");
    setGroupSecondSource("literal");
  };
  const createGroup = async () => {
    const target = groupTarget();
    if (!target) return;
    try {
      const scope = conditions().find((item) =>
        item.query_node_id === target.queryNodeId
      );
      if (!scope) throw new Error(t("sqlPage.structure.editFailed"));
      const draft: LeafDraft = {
        column: newColumn(),
        operator: newOperator(),
        source: newValueSource(),
        value: newValue(),
        valueKind: newValueKind(),
        argument: newArgument(),
      };
      const first = makeCondition(
        draft,
        newValueSource() === "argument" && !newArgument(),
        scope,
        qualifierByQuery()[scope.query_node_id] ?? null,
      );
      const second = makeCondition(
        {
          column: groupSecondColumn(),
          operator: groupSecondOperator(),
          source: groupSecondSource(),
          value: groupSecondValue(),
          valueKind: groupSecondValueKind(),
          argument: groupSecondArgument(),
        },
        false,
        scope,
        qualifierByQuery()[scope.query_node_id] ?? null,
      );
      const edit: SqlDocumentSemanticEdit = {
        kind: "insert_filter_condition",
        node_id: target.parentNodeId ?? selectIdsByQuery()[scope.query_node_id],
        expected_kind: target.parentNodeId === null
          ? "select"
          : nodeKindsById()[target.parentNodeId] ?? "binary_expression",
        condition: {
          kind: "group",
          operator: groupConnector(),
          children: [first.condition, second.condition],
        },
      };
      await commit(
        [edit],
        first.created,
        first.binding,
        first.nextArgs,
        first.nextBindings,
      );
      setGroupTarget(null);
      setAddTarget(null);
      const focusKey = `add-filter:${scope.query_node_id}:${
        target.parentNodeId ?? "root"
      }`;
      setTimeout(() => {
        globalThis.document?.querySelector<HTMLElement>(
          `[data-condition-focus="${focusKey}"]`,
        )?.focus();
      });
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : t("sqlPage.structure.editFailed"),
      );
    }
  };
  const applyHistory = async (direction: "undo" | "redo") => {
    if (busy() || locked()) return;
    const source = props.sql();
    const scope = props.scope?.();
    const request = generation;
    const entry = (direction === "undo" ? history() : redo()).at(-1);
    if (!entry || entry.undo.expected_source !== source) return;
    setBusy(true);
    setError(undefined);
    try {
      const result = await applySqlDocumentEditTransaction(source, entry.undo);
      if (!current(request, source, scope)) return;
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
      if (props.onFilterEdit) {
        if (
          JSON.stringify(props.argumentBindings?.() ?? []) !==
            JSON.stringify(expectedBindings) ||
          JSON.stringify(props.argumentDefinitions?.() ?? []) !==
            JSON.stringify(expectedArguments)
        ) throw new Error(t("sqlPage.structure.editFailed"));
        if (
          props.onFilterEdit({
            expectedSql: source,
            expectedScope: scope,
            resultSql: result.document.source,
            expectedBindings,
            resultBindings,
            expectedArguments,
            resultArguments,
          }) === false
        ) throw new Error(t("sqlPage.structure.editFailed"));
      } else props.onSqlChange(result.document.source);
      const nextEntry = { ...entry, undo: result.undo };
      if (direction === "undo") {
        setHistory(history().slice(0, -1));
        setRedo([...redo(), nextEntry].slice(-32));
      } else {
        setRedo(redo().slice(0, -1));
        setHistory([...history(), nextEntry].slice(-32));
      }
      setDrafts({});
      setGroupDrafts({});
    } catch (cause) {
      if (current(request, source, scope)) {
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

  const renderCondition = (
    node: SqlDocumentConditionNode,
    scope: SqlDocumentConditionScope,
    depth = 0,
  ) => {
    if (isGroup(node)) {
      return (
        <div
          class="sql-condition-group"
          style={{ "--condition-depth": depth }}
          role="group"
          aria-label={t("sqlPage.structure.filterGroup")}
        >
          <div class="sql-condition-group__heading">
            <label class="ui-field">
              <span>{t("sqlPage.structure.filterGroup")}</span>
              <select
                class="ui-input"
                data-condition-focus={`${node.node_id}:group-operator`}
                aria-label={t("sqlPage.structure.filterGroupOperator")}
                value={groupDrafts()[node.node_id] ?? node.operator}
                disabled={busy() || locked()}
                onChange={(event) =>
                  setGroupDrafts({
                    ...groupDrafts(),
                    [node.node_id]: event.currentTarget.value as "and" | "or",
                  })}
              >
                <option
                  value="and"
                  selected={(groupDrafts()[node.node_id] ?? node.operator) ===
                    "and"}
                >
                  {t("sqlPage.structure.filterAll")}
                </option>
                <option
                  value="or"
                  selected={(groupDrafts()[node.node_id] ?? node.operator) ===
                    "or"}
                >
                  {t("sqlPage.structure.filterAny")}
                </option>
              </select>
            </label>
            <button
              type="button"
              class="ui-button ui-button-secondary"
              disabled={busy() || locked()}
              onClick={() => openGroupComposer(scope, node.node_id)}
            >
              {t("sqlPage.structure.addFilterGroup")}
            </button>
            <IconButton
              icon="close"
              label={t("sqlPage.structure.removeFilterGroup")}
              disabled={busy() || locked()}
              onClick={() => removeNode(node.node_id)}
            />
          </div>
          <Show
            when={groupTarget()?.parentNodeId === node.node_id &&
              groupTarget()?.queryNodeId === scope.query_node_id}
          >
            <GroupComposer scope={scope} />
          </Show>
          <For each={node.children}>
            {(child) => renderCondition(child, scope, depth + 1)}
          </For>
          <Show when={depth > 0 && contextFor(scope)}>
            <AddRow parentNodeId={node.node_id} scope={scope} />
          </Show>
        </div>
      );
    }
    if (isComparison(node)) {
      const draft = () => draftFor(node);
      const setDraft = (patch: Partial<LeafDraft>) =>
        setDrafts({ ...drafts(), [node.node_id]: { ...draft(), ...patch } });
      const label = columnLabel(scope, node.column.column.value);
      return (
        <div class="sql-condition-row" role="group" aria-label={label}>
          <Show
            when={editingNodeId() === node.node_id}
            fallback={
              <div class="sql-condition-row__summary">
                <span>
                  {t("sqlPage.structure.conditionSummary", {
                    column: label,
                    operator: operatorName(draft().operator),
                    value: draft().source === "argument"
                      ? t("sqlPage.structure.conditionArgumentValue", {
                        name: draft().argument,
                      })
                      : draft().valueKind === "string"
                      ? `“${draft().value}”`
                      : draft().valueKind === "boolean"
                      ? draft().value.toUpperCase()
                      : draft().valueKind === "null"
                      ? "NULL"
                      : draft().value,
                  })}
                </span>
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  data-condition-focus={`${node.node_id}:edit`}
                  disabled={busy() || locked()}
                  onClick={() => {
                    setEditingNodeId(node.node_id);
                    setTimeout(() => {
                      globalThis.document?.querySelector<HTMLElement>(
                        `[data-condition-focus="${node.node_id}:column"]`,
                      )?.focus();
                    });
                  }}
                >
                  {t("sqlPage.structure.editCondition")}
                </button>
                <IconButton
                  icon="close"
                  label={t("sqlPage.structure.removeFilter")}
                  disabled={busy() || locked()}
                  onClick={() => removeNode(node.node_id)}
                />
              </div>
            }
          >
            <label class="ui-field">
              <span>{t("sqlPage.structure.column")}</span>
              <select
                class="ui-input"
                data-condition-focus={`${node.node_id}:column`}
                aria-label={t("sqlPage.structure.column")}
                value={draft().column}
                disabled={busy() || locked()}
                onChange={(event) =>
                  setDraft({
                    column: event.currentTarget.value,
                    operator:
                      operatorOptions(event.currentTarget.value, scope)[0] ??
                        "eq",
                    argument: "",
                  })}
              >
                <For each={filterColumns(scope)}>
                  {(column) => (
                    <option
                      value={column.name}
                      selected={column.name === draft().column}
                    >
                      {columnLabel(scope, column.name)}
                    </option>
                  )}
                </For>
              </select>
            </label>
            <label class="ui-field">
              <span>{t("sqlPage.structure.operator")}</span>
              <select
                class="ui-input"
                data-condition-focus={`${node.node_id}:operator`}
                aria-label={t("sqlPage.structure.operator")}
                value={draft().operator}
                disabled={busy() || locked()}
                onChange={(event) => {
                  lastConditionFocusKey = `${node.node_id}:operator`;
                  setDraft({
                    operator: event.currentTarget
                      .value as SqlDocumentFilterOperator,
                  });
                }}
              >
                <For each={operatorOptions(draft().column, scope)}>
                  {(operator) => (
                    <option value={operator}>{operatorName(operator)}</option>
                  )}
                </For>
              </select>
            </label>
            <Show when={valueInputOperators.has(draft().operator)}>
              <Show when={props.argumentDefinitions}>
                <label class="ui-field">
                  <span>{t("sqlPage.structure.filterValueSource")}</span>
                  <select
                    class="ui-input"
                    value={draft().source}
                    aria-label={t("sqlPage.structure.filterValueSource")}
                    disabled={busy() || locked()}
                    onChange={(event) =>
                      setDraft({
                        source: event.currentTarget.value as
                          | "literal"
                          | "argument",
                      })}
                  >
                    <option value="literal">
                      {t("sqlPage.structure.filterValueFixed")}
                    </option>
                    <option value="argument">
                      {t("sqlPage.structure.filterValueArgument")}
                    </option>
                  </select>
                </label>
              </Show>
              <Show
                when={draft().source === "argument"}
                fallback={
                  <>
                    <label class="ui-field">
                      <span>{t("sqlPage.structure.documentValueKind")}</span>
                      <select
                        class="ui-input"
                        data-condition-focus={`${node.node_id}:value-kind`}
                        aria-label={t("sqlPage.structure.documentValueKind")}
                        value={draft().valueKind}
                        disabled={busy() || locked()}
                        onChange={(event) =>
                          setDraft({
                            valueKind: event.currentTarget
                              .value as LeafDraft["valueKind"],
                            value: event.currentTarget.value === "null"
                              ? ""
                              : draft().value,
                          })}
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
                        <option value="null">NULL</option>
                      </select>
                    </label>
                    <label class="ui-field">
                      <span>{t("sqlPage.structure.value")}</span>
                      <Show
                        when={draft().valueKind === "boolean" ||
                          draft().valueKind === "null"}
                        fallback={
                          <input
                            class="ui-input"
                            data-condition-focus={`${node.node_id}:value`}
                            aria-label={t("sqlPage.structure.value")}
                            value={draft().value}
                            disabled={busy() || locked()}
                            onInput={(event) =>
                              setDraft({ value: event.currentTarget.value })}
                          />
                        }
                      >
                        <select
                          class="ui-input"
                          data-condition-focus={`${node.node_id}:value`}
                          aria-label={t("sqlPage.structure.value")}
                          value={draft().valueKind === "null"
                            ? "null"
                            : draft().value}
                          disabled={busy() || locked()}
                          onChange={(event) =>
                            setDraft({
                              value: event.currentTarget.value === "null"
                                ? ""
                                : event.currentTarget.value,
                            })}
                        >
                          <option value="">
                            {t("sqlPage.structure.documentChooseValue")}
                          </option>
                          <option value="true">TRUE</option>
                          <option value="false">FALSE</option>
                          <Show when={draft().valueKind === "null"}>
                            <option value="null">NULL</option>
                          </Show>
                        </select>
                      </Show>
                    </label>
                  </>
                }
              >
                <label class="ui-field">
                  <span>{t("sqlPage.structure.filterDeclaredArgument")}</span>
                  <select
                    class="ui-input"
                    value={draft().argument}
                    aria-label={t("sqlPage.structure.filterDeclaredArgument")}
                    disabled={busy() || locked()}
                    onChange={(event) =>
                      setDraft({ argument: event.currentTarget.value })}
                  >
                    <option value="">
                      {t("sqlPage.structure.filterChooseArgument")}
                    </option>
                    <For each={argumentsFor(draft().column, scope)}>
                      {(argument) => (
                        <option
                          value={argument.name}
                          selected={argument.name === draft().argument}
                        >
                          {argument.name} ({argument.type})
                        </option>
                      )}
                    </For>
                  </select>
                </label>
              </Show>
            </Show>
            <IconButton
              icon="close"
              label={t("sqlPage.structure.removeFilter")}
              disabled={busy() || locked()}
              onClick={() => removeNode(node.node_id)}
            />
            <button
              type="button"
              class="ui-button ui-button-secondary"
              data-condition-focus={`${node.node_id}:edit`}
              disabled={busy() || locked()}
              onClick={() => {
                setEditingNodeId(null);
                setTimeout(() => {
                  globalThis.document?.querySelector<HTMLElement>(
                    `[data-condition-focus="${node.node_id}:edit"]`,
                  )?.focus();
                });
              }}
            >
              {t("sqlPage.structure.doneEditingCondition")}
            </button>
          </Show>
        </div>
      );
    }
    return (
      <div
        class="sql-condition-advanced"
        role="group"
        aria-label={t("sqlPage.structure.advancedCondition")}
      >
        <span>{t("sqlPage.structure.advancedCondition")}</span>
        <code>{node.sql}</code>
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={busy() || locked()}
          onClick={props.onAdvanced}
        >
          {t("sqlPage.structure.advancedStructure")}
        </button>
      </div>
    );
  };
  const AddRow = (
    p: { parentNodeId: number | null; scope: SqlDocumentConditionScope },
  ) => {
    const target = () => addTarget();
    const expanded = () =>
      target()?.parentNodeId === p.parentNodeId &&
      target()?.queryNodeId === p.scope.query_node_id &&
      !(groupTarget()?.parentNodeId === p.parentNodeId &&
        groupTarget()?.queryNodeId === p.scope.query_node_id);
    return (
      <div class="sql-condition-row sql-condition-row--add">
        <Show
          when={expanded()}
          fallback={
            <button
              type="button"
              class="ui-button ui-button-secondary"
              data-condition-focus={`add-filter:${p.scope.query_node_id}:${
                p.parentNodeId ?? "root"
              }`}
              disabled={busy() || locked()}
              onClick={() => {
                setNewColumn("");
                setNewValue("");
                setNewArgument("");
                setGroupTarget(null);
                setAddTarget({
                  parentNodeId: p.parentNodeId,
                  queryNodeId: p.scope.query_node_id,
                });
              }}
            >
              {t("sqlPage.structure.addCondition")}
            </button>
          }
        >
          <label class="ui-field">
            <span>{t("sqlPage.structure.column")}</span>
            <select
              class="ui-input"
              data-condition-focus={`add-filter:${p.scope.query_node_id}:${
                p.parentNodeId ?? "root"
              }`}
              aria-label={t("sqlPage.structure.addFilterColumn")}
              value={newColumn()}
              disabled={busy() || locked()}
              onChange={(event) => {
                const name = event.currentTarget.value;
                setNewColumn(name);
                setNewOperator(operatorOptions(name, p.scope)[0] ?? "eq");
                const kind = typeFor(name, p.scope) === "boolean"
                  ? "boolean"
                  : typeFor(name, p.scope) === "integer" ||
                      typeFor(name, p.scope) === "float"
                  ? "number"
                  : "string";
                setNewValueKind(kind);
                setNewValue(
                  kind === "number" ? "0" : kind === "boolean" ? "true" : "",
                );
              }}
            >
              <option value="">
                {t("sqlPage.structure.documentChooseColumn")}
              </option>
              <For each={filterColumns(p.scope)}>
                {(column) => (
                  <option
                    value={column.name}
                    selected={column.name === newColumn()}
                  >
                    {columnLabel(p.scope, column.name)}
                  </option>
                )}
              </For>
            </select>
          </label>
          <label class="ui-field">
            <span>{t("sqlPage.structure.operator")}</span>
            <select
              class="ui-input"
              aria-label={t("sqlPage.structure.addFilterOperator")}
              value={newOperator()}
              disabled={busy() || locked()}
              onChange={(event) =>
                setNewOperator(
                  event.currentTarget.value as SqlDocumentFilterOperator,
                )}
            >
              <For each={operatorOptions(newColumn(), p.scope)}>
                {(operator) => (
                  <option value={operator}>{operatorName(operator)}</option>
                )}
              </For>
            </select>
          </label>
          <Show when={valueInputOperators.has(newOperator())}>
            <Show when={props.argumentDefinitions}>
              <label class="ui-field">
                <span>{t("sqlPage.structure.filterValueSource")}</span>
                <select
                  class="ui-input"
                  aria-label={t("sqlPage.structure.filterValueSource")}
                  value={newValueSource()}
                  onChange={(event) =>
                    setNewValueSource(
                      event.currentTarget.value as "literal" | "argument",
                    )}
                >
                  <option value="literal">
                    {t("sqlPage.structure.filterValueFixed")}
                  </option>
                  <option value="argument">
                    {t("sqlPage.structure.filterValueArgument")}
                  </option>
                </select>
              </label>
            </Show>
            <Show
              when={newValueSource() === "argument"}
              fallback={
                <>
                  <label class="ui-field">
                    <span>{t("sqlPage.structure.documentValueKind")}</span>
                    <select
                      class="ui-input"
                      aria-label={t("sqlPage.structure.documentValueKind")}
                      value={newValueKind()}
                      disabled={busy() || locked()}
                      onChange={(event) =>
                        setNewValueKind(
                          event.currentTarget.value as
                            | "string"
                            | "number"
                            | "boolean",
                        )}
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
                  <Show
                    when={newValueKind() === "boolean"}
                    fallback={
                      <label class="ui-field">
                        <span>{t("sqlPage.structure.value")}</span>
                        <input
                          class="ui-input"
                          aria-label={t("sqlPage.structure.addFilterValue")}
                          value={newValue()}
                          disabled={busy() || locked()}
                          onInput={(event) =>
                            setNewValue(event.currentTarget.value)}
                        />
                      </label>
                    }
                  >
                    <label class="ui-field">
                      <span>{t("sqlPage.structure.value")}</span>
                      <select
                        class="ui-input"
                        aria-label={t("sqlPage.structure.addFilterValue")}
                        value={newValue()}
                        disabled={busy() || locked()}
                        onChange={(event) =>
                          setNewValue(event.currentTarget.value)}
                      >
                        <option value="">
                          {t("sqlPage.structure.documentChooseValue")}
                        </option>
                        <option value="true">TRUE</option>
                        <option value="false">FALSE</option>
                      </select>
                    </label>
                  </Show>
                </>
              }
            >
              <label class="ui-field">
                <span>{t("sqlPage.structure.filterDeclaredArgument")}</span>
                <select
                  class="ui-input"
                  aria-label={t("sqlPage.structure.filterDeclaredArgument")}
                  value={newArgument()}
                  disabled={busy() || locked()}
                  onChange={(event) =>
                    setNewArgument(event.currentTarget.value)}
                >
                  <option value="">
                    {t("sqlPage.structure.filterNewArgumentName")}
                  </option>
                  <For each={argumentsFor(newColumn(), p.scope)}>
                    {(item) => (
                      <option
                        value={item.name}
                        selected={item.name === newArgument()}
                      >
                        {item.name} ({item.type})
                      </option>
                    )}
                  </For>
                </select>
              </label>
              <Show when={!newArgument()}>
                <span class="ui-text-muted">
                  {typeFor(newColumn(), p.scope)}
                </span>
                <label class="ui-field">
                  <span>{t("sqlPage.structure.filterNewArgumentName")}</span>
                  <input
                    class="ui-input"
                    aria-label={t("sqlPage.structure.filterNewArgumentName")}
                    value={newArgumentName()}
                    disabled={busy() || locked()}
                    onInput={(event) =>
                      setNewArgumentName(event.currentTarget.value)}
                  />
                </label>
                <Show when={props.scalarVariables}>
                  <label class="ui-field">
                    <span>{t("sqlPage.structure.filterBindToVariable")}</span>
                    <select
                      class="ui-input"
                      aria-label={t("sqlPage.structure.filterBindToVariable")}
                      value={newBinding()}
                      disabled={busy() || locked()}
                      onChange={(event) =>
                        setNewBinding(event.currentTarget.value)}
                    >
                      <option value="">
                        {t("sqlPage.structure.filterNoBinding")}
                      </option>
                      <For each={scalarsFor(newColumn(), p.scope)}>
                        {(item) => (
                          <option value={item.id}>
                            {item.name} ({item.type})
                          </option>
                        )}
                      </For>
                    </select>
                  </label>
                </Show>
                <Show when={!props.scalarVariables}>
                  <label class="ui-field">
                    <span>{t("sqlPage.structure.filterDefaultOptional")}</span>
                    <input
                      class="ui-input"
                      aria-label={t("sqlPage.structure.filterDefaultOptional")}
                      value={newArgumentDefault()}
                      onInput={(event) =>
                        setNewArgumentDefault(event.currentTarget.value)}
                    />
                  </label>
                </Show>
              </Show>
            </Show>
          </Show>
          <button
            type="button"
            class="ui-button ui-button-primary"
            disabled={busy() || locked() ||
              (!isValidDraft(
                {
                  column: newColumn(),
                  operator: newOperator(),
                  source: newValueSource(),
                  value: newValue(),
                  valueKind: newValueKind(),
                  argument: newArgument(),
                },
                true,
                p.scope,
              ) && !isRevokedDraft({
                column: newColumn(),
                operator: newOperator(),
                source: newValueSource(),
                value: newValue(),
                valueKind: newValueKind(),
                argument: newArgument(),
              }, p.scope))}
            onClick={() => void addCondition(p.parentNodeId, p.scope)}
          >
            {t("sqlPage.structure.addFilter")}
          </button>
          <Show when={p.parentNodeId === null}>
            <button
              type="button"
              class="ui-button ui-button-secondary"
              disabled={busy() || locked()}
              onClick={() => openGroupComposer(p.scope, null)}
            >
              {t("sqlPage.structure.addFilterGroup")}
            </button>
          </Show>
          <button
            type="button"
            class="ui-button ui-button-secondary"
            disabled={busy() || locked()}
            onClick={() => {
              setGroupTarget(null);
              setAddTarget(null);
            }}
          >
            {t("sqlPage.structure.cancelCondition")}
          </button>
        </Show>
        <Show
          when={p.parentNodeId === null &&
            groupTarget()?.parentNodeId === p.parentNodeId &&
            groupTarget()?.queryNodeId === p.scope.query_node_id}
        >
          <GroupComposer scope={p.scope} />
        </Show>
      </div>
    );
  };

  const GroupComposer = (p: { scope: SqlDocumentConditionScope }) => (
    <div
      class="sql-condition-group-composer"
      role="group"
      aria-label={t("sqlPage.structure.newFilterGroup")}
    >
      <label class="ui-field">
        <span>{t("sqlPage.structure.filterGroupOperator")}</span>
        <select
          class="ui-input"
          aria-label={t("sqlPage.structure.filterGroupOperator")}
          value={groupConnector()}
          disabled={busy() || locked()}
          onChange={(event) =>
            setGroupConnector(event.currentTarget.value as "and" | "or")}
        >
          <option value="and">{t("sqlPage.structure.filterAll")}</option>
          <option value="or">{t("sqlPage.structure.filterAny")}</option>
        </select>
      </label>
      <p>{t("sqlPage.structure.newGroupFirstCondition")}</p>
      <p>{t("sqlPage.structure.newGroupSecondCondition")}</p>
      <label class="ui-field">
        <span>{t("sqlPage.structure.column")}</span>
        <select
          class="ui-input"
          aria-label={t("sqlPage.structure.secondGroupColumn")}
          value={groupSecondColumn()}
          disabled={busy() || locked()}
          onChange={(event) => {
            const name = event.currentTarget.value;
            setGroupSecondColumn(name);
            setGroupSecondOperator(operatorOptions(name, p.scope)[0] ?? "eq");
            setGroupSecondValueKind(
              typeFor(name, p.scope) === "boolean"
                ? "boolean"
                : typeFor(name, p.scope) === "integer" ||
                    typeFor(name, p.scope) === "float"
                ? "number"
                : "string",
            );
            setGroupSecondArgument("");
          }}
        >
          <option value="">
            {t("sqlPage.structure.documentChooseColumn")}
          </option>
          <For each={filterColumns(p.scope)}>
            {(column) => (
              <option
                value={column.name}
                selected={column.name === groupSecondColumn()}
              >
                {columnLabel(p.scope, column.name)}
              </option>
            )}
          </For>
        </select>
      </label>
      <label class="ui-field">
        <span>{t("sqlPage.structure.operator")}</span>
        <select
          class="ui-input"
          aria-label={t("sqlPage.structure.secondGroupOperator")}
          value={groupSecondOperator()}
          disabled={busy() || locked()}
          onChange={(event) =>
            setGroupSecondOperator(
              event.currentTarget.value as SqlDocumentFilterOperator,
            )}
        >
          <For each={operatorOptions(groupSecondColumn(), p.scope)}>
            {(operator) => (
              <option value={operator}>{operatorName(operator)}</option>
            )}
          </For>
        </select>
      </label>
      <Show when={valueInputOperators.has(groupSecondOperator())}>
        <Show when={props.argumentDefinitions}>
          <label class="ui-field">
            <span>{t("sqlPage.structure.filterValueSource")}</span>
            <select
              class="ui-input"
              aria-label={t("sqlPage.structure.secondGroupValueSource")}
              value={groupSecondSource()}
              disabled={busy() || locked()}
              onChange={(event) =>
                setGroupSecondSource(
                  event.currentTarget.value as "literal" | "argument",
                )}
            >
              <option value="literal">
                {t("sqlPage.structure.filterValueFixed")}
              </option>
              <option value="argument">
                {t("sqlPage.structure.filterValueArgument")}
              </option>
            </select>
          </label>
        </Show>
        <Show
          when={groupSecondSource() === "argument"}
          fallback={
            <>
              <label class="ui-field">
                <span>{t("sqlPage.structure.documentValueKind")}</span>
                <select
                  class="ui-input"
                  aria-label={t("sqlPage.structure.secondGroupValueKind")}
                  value={groupSecondValueKind()}
                  disabled={busy() || locked()}
                  onChange={(event) =>
                    setGroupSecondValueKind(
                      event.currentTarget.value as
                        | "string"
                        | "number"
                        | "boolean",
                    )}
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
              <label class="ui-field">
                <span>{t("sqlPage.structure.value")}</span>
                <Show
                  when={groupSecondValueKind() === "boolean"}
                  fallback={
                    <input
                      class="ui-input"
                      aria-label={t("sqlPage.structure.secondGroupValue")}
                      value={groupSecondValue()}
                      disabled={busy() || locked()}
                      onInput={(event) =>
                        setGroupSecondValue(event.currentTarget.value)}
                    />
                  }
                >
                  <select
                    class="ui-input"
                    aria-label={t("sqlPage.structure.secondGroupValue")}
                    value={groupSecondValue()}
                    disabled={busy() || locked()}
                    onChange={(event) =>
                      setGroupSecondValue(event.currentTarget.value)}
                  >
                    <option value="">
                      {t("sqlPage.structure.documentChooseValue")}
                    </option>
                    <option value="true">TRUE</option>
                    <option value="false">FALSE</option>
                  </select>
                </Show>
              </label>
            </>
          }
        >
          <label class="ui-field">
            <span>{t("sqlPage.structure.filterDeclaredArgument")}</span>
            <select
              class="ui-input"
              aria-label={t("sqlPage.structure.secondGroupArgument")}
              value={groupSecondArgument()}
              disabled={busy() || locked()}
              onChange={(event) =>
                setGroupSecondArgument(event.currentTarget.value)}
            >
              <option value="">
                {t("sqlPage.structure.filterChooseArgument")}
              </option>
              <For each={argumentsFor(groupSecondColumn(), p.scope)}>
                {(argument) => (
                  <option
                    value={argument.name}
                    selected={argument.name === groupSecondArgument()}
                  >
                    {argument.name} ({argument.type})
                  </option>
                )}
              </For>
            </select>
          </label>
        </Show>
      </Show>
      <button
        type="button"
        class="ui-button ui-button-primary"
        disabled={busy() || locked() ||
          (!isValidDraft(
            {
              column: newColumn(),
              operator: newOperator(),
              source: newValueSource(),
              value: newValue(),
              valueKind: newValueKind(),
              argument: newArgument(),
            },
            true,
            p.scope,
          ) && !isRevokedDraft({
            column: newColumn(),
            operator: newOperator(),
            source: newValueSource(),
            value: newValue(),
            valueKind: newValueKind(),
            argument: newArgument(),
          }, p.scope)) || (!isValidDraft(
            {
              column: groupSecondColumn(),
              operator: groupSecondOperator(),
              source: groupSecondSource(),
              value: groupSecondValue(),
              valueKind: groupSecondValueKind(),
              argument: groupSecondArgument(),
            },
            false,
            p.scope,
          ) && !isRevokedDraft({
            column: groupSecondColumn(),
            operator: groupSecondOperator(),
            source: groupSecondSource(),
            value: groupSecondValue(),
            valueKind: groupSecondValueKind(),
            argument: groupSecondArgument(),
          }, p.scope))}
        onClick={() => void createGroup()}
      >
        {t("sqlPage.structure.createFilterGroup")}
      </button>
      <button
        type="button"
        class="ui-button ui-button-secondary"
        disabled={busy() || locked()}
        onClick={() => setGroupTarget(null)}
      >
        {t("sqlPage.structure.cancelFilterGroup")}
      </button>
    </div>
  );

  return (
    <div
      class="sql-filter-sort-editor sql-condition-workspace"
      aria-busy={busy() || undefined}
      onFocusIn={(event) => {
        const target = event.target;
        if (target instanceof HTMLElement && target.dataset.conditionFocus) {
          lastConditionFocusKey = target.dataset.conditionFocus;
        }
      }}
    >
      <For each={conditions()}>
        {(scope) => (
          <section
            class="sql-condition-scope"
            aria-label={t("sqlPage.structure.filters")}
          >
            <Show
              when={contextFor(scope)}
              fallback={
                <div>
                  <p role="status">
                    {t("sqlPage.structure.documentNoOperationTarget")}
                  </p>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary"
                    onClick={props.onAdvanced}
                  >
                    {t("sqlPage.structure.advancedStructure")}
                  </button>
                </div>
              }
            >
              <Show
                when={scope.expression}
                fallback={<p>{t("sqlPage.structure.noFilters")}</p>}
              >
                {(expression) => renderCondition(expression(), scope)}
              </Show>
              <AddRow parentNodeId={null} scope={scope} />
            </Show>
          </section>
        )}
      </For>
      <Show
        when={Object.keys(drafts()).length || Object.keys(groupDrafts()).length}
      >
        <button
          type="button"
          class="ui-button ui-button-primary"
          disabled={busy() || locked()}
          onClick={() => void applyDrafts()}
        >
          {t("sqlPage.structure.applyFilterChanges")}
        </button>
      </Show>
      <Show when={error()}>
        <p role="alert" class="ui-text-danger">{error()}</p>
      </Show>
      <div class="sql-filter-sort-editor__history">
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={busy() || locked() ||
            history().at(-1)?.undo.expected_source !== props.sql()}
          onClick={() => void applyHistory("undo")}
        >
          {t("sqlPage.columns.undo")}
        </button>
        <button
          type="button"
          class="ui-button ui-button-secondary"
          disabled={busy() || locked() ||
            redo().at(-1)?.undo.expected_source !== props.sql()}
          onClick={() => void applyHistory("redo")}
        >
          {t("sqlPage.columns.redo")}
        </button>
      </div>
      <button
        type="button"
        class="ui-button ui-button-secondary"
        disabled={busy() || locked()}
        onClick={props.onAdvanced}
      >
        {t("sqlPage.structure.advancedStructure")}
      </button>
    </div>
  );
}
