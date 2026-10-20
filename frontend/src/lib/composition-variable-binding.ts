/** Strict authoring contract for Composition's variable-only graph. */
export type CompositionScalarType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "decimal"
  | "date"
  | "timestamp"
  | "json";

/** SQL argument types currently supported by the shared executor binder. */
export type CompositionSqlArgumentType = Exclude<
  CompositionScalarType,
  "decimal" | "json"
>;

export interface CompositionScalarDescriptor {
  scalar_type: CompositionScalarType;
  nullable: boolean;
}

export type CompositionLiteral =
  | { kind: "string"; value: string }
  | { kind: "boolean"; value: boolean }
  | { kind: "integer"; value: number }
  | { kind: "float"; value: number }
  | { kind: "decimal"; value: string }
  | { kind: "date"; value: string }
  | { kind: "timestamp"; value: string }
  | { kind: "json"; value: unknown }
  | { kind: "null" };

export interface CompositionQueryArgument {
  name: string;
  variable: string;
}

export interface CompositionQueryBase {
  arguments: CompositionQueryArgument[];
  /** Resolved by Core when the query variable is committed. */
  form_bindings?: Array<{ relation: string; form_id: string }>;
  /** Resolved by Core when the query variable is committed. */
  schema_fingerprint?: string;
}

export type CompositionQueryDefinition =
  | (CompositionQueryBase & { kind: "inline_sql"; sql: string })
  | (CompositionQueryBase & {
    kind: "saved_sql";
    entry_id: string;
    revision_id: string;
    column?: string;
  });

export type CompositionQueryReturn =
  | { kind: "scalar"; value_type: CompositionScalarDescriptor }
  | {
    kind: "table";
    columns: Array<{ name: string; value_type: CompositionScalarDescriptor }>;
  };

export type CompositionVariableNode =
  | {
    id: string;
    kind: "constant";
    value_type: CompositionScalarDescriptor;
    value: CompositionLiteral;
  }
  | {
    id: string;
    kind: "input";
    value_type: CompositionScalarDescriptor;
    required: boolean;
    default?: CompositionLiteral;
  }
  | {
    id: string;
    kind: "query";
    returns: CompositionQueryReturn;
    query: CompositionQueryDefinition;
  };

export type CompositionComponent =
  | { id: string; kind: "table"; variable: string }
  | {
    id: string;
    kind: "metric";
    variable: string;
    presentation?: {
      kind: "number";
      unit?: string;
      fraction_digits?: number;
    };
  }
  | { id: string; kind: "input"; variable: string }
  | {
    id: string;
    kind: "text";
    content: { kind: "literal"; text: string } | {
      kind: "variable";
      variable: string;
    };
    style: "title" | "heading" | "body" | "caption";
  };

export interface CompositionGraphDraft {
  name: string;
  variables: CompositionVariableNode[];
  components: CompositionComponent[];
  layout: {
    kind: "flow";
    rows: Array<{
      id: string;
      items: Array<{ kind: "component"; component: string }>;
    }>;
  };
}

/** Exact spec payload for canonical save; transient editor counters are absent. */
export type CompositionGraphSpecDraft = Pick<
  CompositionGraphDraft,
  "variables" | "components" | "layout"
>;

export interface CompositionGraphDocumentDraft {
  format: "ugoite.composition";
  format_version: 1;
  kind: "dashboard";
  name: string;
  spec: CompositionGraphSpecDraft;
}

export const compositionGraphSpecDraft = (
  draft: CompositionGraphDraft,
): CompositionGraphSpecDraft => ({
  variables: draft.variables,
  components: draft.components,
  layout: draft.layout,
});

export const compositionGraphDocumentDraft = (
  draft: CompositionGraphDraft,
): CompositionGraphDocumentDraft => ({
  format: "ugoite.composition",
  format_version: 1,
  kind: "dashboard",
  name: draft.name,
  spec: compositionGraphSpecDraft(draft),
});

/** Project a Rust-canonical graph document into editor Work, fail-closed. */
export const compositionGraphDraftFromDocument = (
  value: unknown,
): CompositionGraphDraft => {
  if (!value || typeof value !== "object") {
    throw new Error("Invalid Composition document");
  }
  const document = value as Partial<CompositionGraphDocumentDraft>;
  const spec = document.spec as
    | (Partial<CompositionGraphSpecDraft> & Record<string, unknown>)
    | undefined;
  if (
    document.format !== "ugoite.composition" ||
    document.format_version !== 1 || document.kind !== "dashboard" ||
    typeof document.name !== "string" || !spec ||
    Object.hasOwn(spec, "sources") ||
    !Array.isArray(spec.variables) || !Array.isArray(spec.components) ||
    !spec.layout || spec.layout.kind !== "flow" ||
    !Array.isArray(spec.layout.rows)
  ) throw new Error("Invalid graph-only Composition document");
  for (const component of spec.components) {
    if (
      !component || typeof component !== "object" ||
      Object.hasOwn(component, "source") ||
      Object.hasOwn(component, "form_id") ||
      Object.hasOwn(component, "source_id")
    ) throw new Error("Invalid graph component binding");
  }
  for (const node of spec.variables) {
    if (
      !node || typeof node !== "object" ||
      !["constant", "input", "query"].includes(node.kind)
    ) throw new Error("Invalid graph variable");
  }
  return {
    name: document.name,
    variables: spec.variables,
    components: spec.components,
    layout: spec.layout,
  } as CompositionGraphDraft;
};

export type BindingComponentKind = "table" | "metric";

export interface CompositionFormChoice {
  id: string;
  name: string;
  /** Server-owned relation name used only to author the inline query. */
  relation: string;
  fields: Array<{ name: string; type: CompositionScalarType }>;
}

export interface CompositionSavedSqlChoice {
  entry_id: string;
  revision_id: string;
  name: string;
  arguments: Array<{ name: string; type: CompositionSqlArgumentType }>;
}

/** Exact Saved SQL revision selected by a navigation entry point. */
export interface CompositionSavedSqlRevisionRef {
  entry_id: string;
  revision_id: string;
}

export type CompositionFormAggregate = "count" | "sum" | "avg" | "min" | "max";

/** Immutable authoring context used to reject stale asynchronous picker work. */
export interface CompositionBindingSession {
  session_id: string;
  space_id: string;
  document_revision: string | null;
  principal_generation: string;
}

/** A resolved query plus an optional exact reusable graph node. */
export interface CompositionResolvedBinding {
  variable: CompositionVariableNode & { kind: "query" };
  /** Existing node selected by the authoritative resolver for exact reuse. */
  reuse_variable_id?: string;
}

export interface CompositionPreparedSavedSql {
  table?: CompositionResolvedBinding;
  /** Only planner-confirmed scalar outputs with exactly one result row. */
  scalars: Array<{ column: string; binding: CompositionResolvedBinding }>;
}

/**
 * Host-owned lookups/resolution. Implementations use existing Core/API
 * contracts; the picker does not invent transport routes or infer SQL schema.
 */
export interface CompositionBindingProvider {
  resolveForm(input: {
    session: CompositionBindingSession;
    form_id: string;
    component: BindingComponentKind;
    aggregate?: { kind: CompositionFormAggregate; column?: string };
    draft: CompositionGraphDraft;
  }): Promise<CompositionResolvedBinding>;
  prepareSavedSql(input: {
    session: CompositionBindingSession;
    entry_id: string;
    revision_id: string;
    component: BindingComponentKind;
    arguments: CompositionQueryArgument[];
    draft: CompositionGraphDraft;
  }): Promise<CompositionPreparedSavedSql>;
  inspectSavedSql?(input: {
    session: CompositionBindingSession;
    entry_id: string;
    revision_id: string;
    component: BindingComponentKind;
    draft: CompositionGraphDraft;
  }): Promise<CompositionPreparedSavedSql>;
}

export const createCompositionGraphDraft = (
  name = "",
): CompositionGraphDraft => ({
  name,
  variables: [],
  components: [],
  layout: { kind: "flow", rows: [{ id: "main", items: [] }] },
});

export const compositionVariableKindIcon = (
  node: CompositionVariableNode,
): "canvas-input" | "appearance" | "sql" =>
  node.kind === "input"
    ? "canvas-input"
    : node.kind === "constant"
    ? "appearance"
    : "sql";

export const compositionNodeReturnsTable = (
  node: CompositionVariableNode,
): boolean => node.kind === "query" && node.returns.kind === "table";

export const compositionNodeReturnsScalar = (
  node: CompositionVariableNode,
): boolean => {
  const type = node.kind === "query"
    ? node.returns.kind === "scalar"
      ? node.returns.value_type.scalar_type
      : undefined
    : node.value_type.scalar_type;
  return type !== undefined && isCompositionMetricType(type);
};

/** Types rendered as a Metric; booleans and JSON remain queryable values. */
export const isCompositionMetricType = (
  type: CompositionScalarType,
): boolean =>
  type === "string" || type === "integer" || type === "float" ||
  type === "decimal" || type === "date" || type === "timestamp";

export const compositionVariableFitsComponent = (
  node: CompositionVariableNode,
  kind: BindingComponentKind,
): boolean =>
  kind === "table"
    ? compositionNodeReturnsTable(node)
    : compositionNodeReturnsScalar(node);

const nextComponentId = (
  components: readonly CompositionComponent[],
): string => {
  let index = 1;
  while (
    components.some((component) => component.id === `component_${index}`)
  ) {
    index += 1;
  }
  return `component_${index}`;
};

/** Place one already-complete strict component and its layout item together. */
export const appendCompositionGraphComponent = (
  draft: CompositionGraphDraft,
  component: CompositionComponent,
): CompositionGraphDraft | undefined => {
  if (
    draft.components.some((entry) => entry.id === component.id) ||
    draft.layout.rows.some((row) =>
      row.items.some((item) => item.component === component.id)
    )
  ) return undefined;
  const rows = draft.layout.rows.length > 0
    ? draft.layout.rows.map((row) => ({ ...row, items: [...row.items] }))
    : [{ id: "main", items: [] }];
  rows[rows.length - 1].items.push({
    kind: "component",
    component: component.id,
  });
  return {
    ...draft,
    components: [...draft.components, component],
    layout: { kind: "flow", rows },
  };
};

/** Remove a view component without deleting any graph variable it shares. */
export const removeCompositionGraphComponent = (
  draft: CompositionGraphDraft,
  componentId: string,
): CompositionGraphDraft | undefined => {
  if (!draft.components.some((component) => component.id === componentId)) {
    return undefined;
  }
  return {
    ...draft,
    components: draft.components.filter((component) =>
      component.id !== componentId
    ),
    layout: {
      kind: "flow",
      rows: draft.layout.rows.map((row) => ({
        ...row,
        items: row.items.filter((item) => item.component !== componentId),
      })),
    },
  };
};

/**
 * Atomic local authoring operation: insert/reuse one query node, bind one
 * display component to that variable, and place it exactly once.
 */
export const bindCompositionComponent = (
  draft: CompositionGraphDraft,
  kind: BindingComponentKind,
  variableId: string,
): CompositionGraphDraft | undefined => {
  const node = draft.variables.find((item) => item.id === variableId);
  if (!node || !compositionVariableFitsComponent(node, kind)) return undefined;
  const componentId = nextComponentId(draft.components);
  const component: CompositionComponent = {
    id: componentId,
    kind,
    variable: variableId,
  };
  return appendCompositionGraphComponent(draft, component);
};

/** Change only a display's graph reference; its shared query node is untouched. */
export const setCompositionComponentVariable = (
  draft: CompositionGraphDraft,
  componentId: string,
  variableId: string,
): CompositionGraphDraft | undefined => {
  const component = draft.components.find((entry) => entry.id === componentId);
  const node = draft.variables.find((entry) => entry.id === variableId);
  if (
    !component || !node ||
    (component.kind !== "table" && component.kind !== "metric")
  ) return undefined;
  if (!compositionVariableFitsComponent(node, component.kind)) return undefined;
  return {
    ...draft,
    components: draft.components.map((entry) =>
      entry.id === componentId && entry.kind !== "text"
        ? { ...entry, variable: variableId }
        : entry
    ),
  };
};

/**
 * Commit resolver output and component placement together. A reused node is
 * never copied or rewritten; a new query and display are emitted as one draft.
 */
export const commitResolvedCompositionBinding = (
  draft: CompositionGraphDraft,
  kind: BindingComponentKind,
  resolved: CompositionResolvedBinding,
): CompositionGraphDraft | undefined => {
  const existing = resolved.reuse_variable_id
    ? draft.variables.find((node) => node.id === resolved.reuse_variable_id)
    : undefined;
  if (resolved.reuse_variable_id && !existing) return undefined;
  if (existing && !sameResolvedQueryMeaning(existing, resolved.variable)) {
    return undefined;
  }
  if (
    !existing &&
    draft.variables.some((node) => node.id === resolved.variable.id)
  ) {
    return undefined;
  }
  const node = existing ?? resolved.variable;
  if (node.kind !== "query" || !compositionVariableFitsComponent(node, kind)) {
    return undefined;
  }
  const withNode = existing ? draft : {
    ...draft,
    variables: [...draft.variables, resolved.variable],
  };
  return bindCompositionComponent(withNode, kind, node.id);
};

/**
 * Structural identity helper used only for in-memory suggestions. Final
 * reuse eligibility comes from the host resolver after ACL and schema checks.
 */
export const sameResolvedQueryMeaning = (
  left: CompositionVariableNode,
  right: CompositionVariableNode,
): boolean =>
  left.kind === "query" && right.kind === "query" &&
  JSON.stringify(left.returns) === JSON.stringify(right.returns) &&
  JSON.stringify(left.query) === JSON.stringify(right.query);

/** Resolve and rebind an existing display without mutating shared variables. */
export const rebindResolvedCompositionComponent = (
  draft: CompositionGraphDraft,
  componentId: string,
  resolved: CompositionResolvedBinding,
): CompositionGraphDraft | undefined => {
  const existing = resolved.reuse_variable_id
    ? draft.variables.find((node) => node.id === resolved.reuse_variable_id)
    : undefined;
  if (resolved.reuse_variable_id && !existing) return undefined;
  if (existing && !sameResolvedQueryMeaning(existing, resolved.variable)) {
    return undefined;
  }
  if (
    !existing &&
    draft.variables.some((node) => node.id === resolved.variable.id)
  ) {
    return undefined;
  }
  const variable = existing ?? resolved.variable;
  if (variable.kind !== "query") return undefined;
  const withVariable = existing
    ? draft
    : { ...draft, variables: [...draft.variables, resolved.variable] };
  return setCompositionComponentVariable(
    withVariable,
    componentId,
    variable.id,
  );
};

/** Rename graph identity and every explicit query argument/component ref. */
export const renameCompositionVariable = (
  draft: CompositionGraphDraft,
  from: string,
  to: string,
): CompositionGraphDraft | undefined => {
  const next = to.trim();
  if (
    !next || (next !== from && draft.variables.some((node) => node.id === next))
  ) return undefined;
  if (!draft.variables.some((node) => node.id === from)) return undefined;
  return {
    ...draft,
    variables: draft.variables.map((node) => {
      const renamed = node.id === from ? { ...node, id: next } : node;
      if (renamed.kind !== "query") return renamed;
      return {
        ...renamed,
        query: {
          ...renamed.query,
          arguments: renamed.query.arguments.map((argument) =>
            argument.variable === from
              ? { ...argument, variable: next }
              : argument
          ),
        },
      };
    }),
    components: draft.components.map((component) => {
      if (component.kind === "text" && component.content.kind === "variable") {
        return {
          ...component,
          content: {
            ...component.content,
            variable: component.content.variable === from
              ? next
              : component.content.variable,
          },
        };
      }
      return "variable" in component && component.variable === from
        ? { ...component, variable: next }
        : component;
    }),
  };
};

/** Strict UI-side validation for the reference shape; Core remains authoritative. */
export const compositionBindingDiagnostics = (
  draft: CompositionGraphDraft,
): string[] => {
  const diagnostics: string[] = [];
  const nodes = new Map(draft.variables.map((node) => [node.id, node]));
  const components = new Map(
    draft.components.map((component) => [component.id, component]),
  );
  const ids = draft.layout.rows.flatMap((row) =>
    row.items.map((item) => item.component)
  );
  if (new Set(ids).size !== ids.length) {
    diagnostics.push("duplicate_layout_component");
  }
  for (const id of ids) {
    if (!components.has(id)) diagnostics.push("unknown_layout_component");
  }
  for (const component of draft.components) {
    if (!ids.includes(component.id)) diagnostics.push("unplaced_component");
    const variable = component.kind === "text"
      ? component.content.kind === "variable"
        ? component.content.variable
        : undefined
      : component.variable;
    if (!variable) continue;
    const node = nodes.get(variable);
    if (!node) {
      diagnostics.push("missing_variable");
      continue;
    }
    if (component.kind === "table" && !compositionNodeReturnsTable(node)) {
      diagnostics.push("variable_type_mismatch");
    }
    if (component.kind === "metric" && !compositionNodeReturnsScalar(node)) {
      diagnostics.push("variable_type_mismatch");
    }
    if (component.kind === "input" && node.kind !== "input") {
      diagnostics.push("variable_type_mismatch");
    }
    if (
      component.kind === "text" &&
      !(node.kind === "query"
        ? node.returns.kind === "scalar" &&
          node.returns.value_type.scalar_type === "string"
        : node.value_type.scalar_type === "string")
    ) {
      diagnostics.push("variable_type_mismatch");
    }
  }
  return [...new Set(diagnostics)];
};
