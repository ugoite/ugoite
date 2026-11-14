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
  | { kind: "integer"; value: string }
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
  CompositionQueryBase & { kind: "inline_sql"; sql: string };

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
  | {
    id: string;
    kind: "table";
    variable: string;
  }
  | { id: string; kind: "input"; variable: string }
  | {
    id: string;
    kind: "text";
    content: string;
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

/** Authoring-only identity; never use this document for save/history/export. */
export const compositionGraphAuthoringDocument = (
  draft: CompositionGraphDraft,
): CompositionGraphDocumentDraft => ({
  ...compositionGraphDocumentDraft(draft),
  name: draft.name.trim() ? draft.name : "Untitled Composition",
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
    (spec.variables !== undefined && !Array.isArray(spec.variables)) ||
    !Array.isArray(spec.components) ||
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
  const variables = (spec.variables ?? []).map((node) => {
    if (
      !node || typeof node !== "object" ||
      !["constant", "input", "query"].includes(node.kind)
    ) throw new Error("Invalid graph variable");
    if (node.kind !== "query") return node;
    if (
      !node.query || typeof node.query !== "object" ||
      node.query.kind !== "inline_sql" ||
      (node.query.arguments !== undefined &&
        !Array.isArray(node.query.arguments))
    ) throw new Error("Invalid graph query arguments");
    // Rust's canonical representation omits empty argument collections.
    // Editor Work keeps a uniform array without changing the source document.
    return {
      ...node,
      query: { ...node.query, arguments: node.query.arguments ?? [] },
    };
  });
  for (const component of spec.components) {
    if (
      !component || typeof component !== "object" ||
      !["table", "text", "input"].includes(component.kind)
    ) throw new Error("Invalid graph component kind");
    if (
      component.kind === "text" &&
      (typeof component.content !== "string" ||
        Object.hasOwn(component, "presentation"))
    ) throw new Error("Invalid graph text template");
    if (
      component.kind === "table" && Object.hasOwn(component, "presentation")
    ) throw new Error("Invalid graph table component");
  }
  return {
    name: document.name,
    variables,
    components: spec.components,
    layout: spec.layout,
  } as CompositionGraphDraft;
};

export type BindingComponentKind = "table";

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
  return type !== undefined && type !== "json";
};

/** Types supported by plain text interpolation. */
export const isCompositionDisplayableType = (
  type: CompositionScalarType,
): boolean =>
  type !== "json";

export const compositionVariableFitsComponent = (
  node: CompositionVariableNode,
  kind: BindingComponentKind,
): boolean =>
  kind === "table" && compositionNodeReturnsTable(node);

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
    !component || !node || component.kind !== "table"
  ) return undefined;
  if (!compositionVariableFitsComponent(node, component.kind)) return undefined;
  return {
    ...draft,
    components: draft.components.map((entry) => {
      if (entry.id !== componentId || entry.kind !== "table") return entry;
      return { ...entry, variable: variableId };
    }),
  };
};

export const sameResolvedQueryMeaning = (
  left: CompositionVariableNode,
  right: CompositionVariableNode,
): boolean =>
  left.kind === "query" && right.kind === "query" &&
  JSON.stringify(left.returns) === JSON.stringify(right.returns) &&
  JSON.stringify(left.query) === JSON.stringify(right.query);

/** Rename graph identity and every explicit query/input/table reference. */
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
    components: draft.components.map((component) =>
      "variable" in component && component.variable === from
        ? { ...component, variable: next }
        : component
    ),
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
    if (component.kind === "text") continue;
    const node = nodes.get(component.variable);
    if (!node) {
      diagnostics.push("missing_variable");
      continue;
    }
    if (component.kind === "table" && !compositionNodeReturnsTable(node)) {
      diagnostics.push("variable_type_mismatch");
    }
    if (component.kind === "input" && node.kind !== "input") {
      diagnostics.push("variable_type_mismatch");
    }
  }
  return [...new Set(diagnostics)];
};
