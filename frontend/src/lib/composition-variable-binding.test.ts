import { describe, expect, it } from "vitest";
import {
  bindCompositionComponent,
  commitResolvedCompositionBinding,
  compositionBindingDiagnostics,
  compositionGraphDocumentDraft,
  compositionGraphDraftFromDocument,
  type CompositionVariableNode,
  createCompositionGraphDraft,
  rebindResolvedCompositionComponent,
  renameCompositionVariable,
  setCompositionComponentVariable,
} from "./composition-variable-binding";

const tableNode: CompositionVariableNode = {
  id: "rows",
  kind: "query",
  returns: {
    kind: "table",
    columns: [{
      name: "amount",
      value_type: { scalar_type: "decimal", nullable: false },
    }],
  },
  query: { kind: "inline_sql", sql: 'SELECT amount FROM "G"', arguments: [] },
};
const scalarNode: CompositionVariableNode = {
  id: "total",
  kind: "query",
  returns: {
    kind: "scalar",
    value_type: { scalar_type: "decimal", nullable: true },
  },
  query: {
    kind: "saved_sql",
    entry_id: "saved-entry",
    revision_id: "saved-revision",
    column: "total",
    arguments: [],
  },
};

describe("strict variable-only Composition bindings", () => {
  it.each([
    ["legacy source list", { sources: [] }],
    ["legacy source reference", {
      components: [{ id: "table", kind: "table", source: "rows" }],
    }],
    ["direct Form reference", {
      components: [{ id: "table", kind: "table", form_id: "form-id" }],
    }],
    ["legacy source id", {
      components: [{ id: "table", kind: "table", source_id: "rows" }],
    }],
  ])("rejects %s in the graph-only editor decoder", (_label, change) => {
    const document = {
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Report",
      spec: {
        variables: [],
        components: [],
        layout: { kind: "flow", rows: [] },
        ...change,
      },
    };
    expect(() => compositionGraphDraftFromDocument(document)).toThrow();
  });

  it("serializes only the strict Composition document envelope and spec", () => {
    const draft = {
      ...createCompositionGraphDraft("Report"),
      variables: [tableNode],
      components: [{ id: "table_1", kind: "table" as const, variable: "rows" }],
      layout: {
        kind: "flow" as const,
        rows: [{
          id: "main",
          items: [{ kind: "component" as const, component: "table_1" }],
        }],
      },
    };
    const saved = compositionGraphDocumentDraft(draft);
    expect(saved).toEqual({
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Report",
      spec: {
        variables: [tableNode],
        components: [{ id: "table_1", kind: "table", variable: "rows" }],
        layout: draft.layout,
      },
    });
    expect(JSON.stringify(saved)).not.toMatch(/source|form_id/);
  });

  it("inserts a compatible variable reference and layout item in one draft", () => {
    const draft = {
      ...createCompositionGraphDraft("Report"),
      variables: [tableNode, scalarNode],
    };
    const inserted = bindCompositionComponent(draft, "table", "rows");
    expect(inserted?.components).toEqual([{
      id: "component_1",
      kind: "table",
      variable: "rows",
    }]);
    expect(inserted?.layout).toEqual({
      kind: "flow",
      rows: [{
        id: "main",
        items: [{ kind: "component", component: "component_1" }],
      }],
    });
    expect(JSON.stringify(inserted)).not.toContain("source_id");
    expect(JSON.stringify(inserted)).not.toContain('"source"');
  });

  it("rejects a table/scalar shape mismatch without changing the draft", () => {
    const draft = {
      ...createCompositionGraphDraft(),
      variables: [tableNode, scalarNode],
    };
    expect(bindCompositionComponent(draft, "table", "total")).toBeUndefined();
    expect(bindCompositionComponent(draft, "metric", "rows")).toBeUndefined();
    expect(draft.components).toEqual([]);
    expect(draft.layout.rows[0].items).toEqual([]);
  });

  it("adds a resolved Form or Saved SQL query and component together", () => {
    const draft = createCompositionGraphDraft();
    const inserted = commitResolvedCompositionBinding(draft, "metric", {
      variable: scalarNode,
    });
    expect(inserted?.variables).toEqual([scalarNode]);
    expect(inserted?.components).toEqual([{
      id: "component_1",
      kind: "metric",
      variable: "total",
    }]);
    expect(inserted?.layout.rows[0].items).toEqual([{
      kind: "component",
      component: "component_1",
    }]);
  });

  it("changes one component reference without mutating its shared query", () => {
    const draft = {
      ...createCompositionGraphDraft(),
      variables: [scalarNode, { ...scalarNode, id: "other" }],
      components: [{
        id: "metric_1",
        kind: "metric" as const,
        variable: "total",
      }],
      layout: {
        kind: "flow" as const,
        rows: [{
          id: "main",
          items: [{ kind: "component" as const, component: "metric_1" }],
        }],
      },
    };
    const changed = setCompositionComponentVariable(draft, "metric_1", "other");
    expect(changed?.components[0]).toEqual({
      id: "metric_1",
      kind: "metric",
      variable: "other",
    });
    expect(changed?.variables).toEqual(draft.variables);
  });

  it("rejects a resolver that reuses a query with different meaning", () => {
    const draft = {
      ...createCompositionGraphDraft(),
      variables: [scalarNode],
      components: [{
        id: "metric-1",
        kind: "metric" as const,
        variable: "total",
      }],
      layout: {
        kind: "flow" as const,
        rows: [{
          id: "main",
          items: [{ kind: "component" as const, component: "metric-1" }],
        }],
      },
    };
    expect(rebindResolvedCompositionComponent(draft, "metric-1", {
      variable: {
        ...scalarNode,
        query: {
          kind: "inline_sql",
          sql: "SELECT 2",
          arguments: [],
        },
      },
      reuse_variable_id: "total",
    })).toBeUndefined();
  });

  it("renames a variable and every query argument and component reference", () => {
    const query: CompositionVariableNode = {
      id: "summary",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
      query: {
        kind: "inline_sql",
        sql: "SELECT COUNT(*) WHERE day = $day",
        arguments: [{ name: "day", variable: "day" }],
      },
    };
    const draft = {
      ...createCompositionGraphDraft(),
      variables: [{
        id: "day",
        kind: "input" as const,
        value_type: { scalar_type: "date" as const, nullable: false },
        required: true,
      }, query],
      components: [{ id: "input_1", kind: "input" as const, variable: "day" }],
      layout: {
        kind: "flow" as const,
        rows: [{
          id: "main",
          items: [{ kind: "component" as const, component: "input_1" }],
        }],
      },
    };
    const renamed = renameCompositionVariable(draft, "day", "report_day");
    expect(renamed?.variables[0].id).toBe("report_day");
    expect(
      renamed?.variables[1].kind === "query" &&
        renamed.variables[1].query.arguments[0].variable,
    ).toBe("report_day");
    expect(renamed?.components[0]).toEqual({
      id: "input_1",
      kind: "input",
      variable: "report_day",
    });
  });

  it("reports non-placed components and incompatible bindings", () => {
    const draft = {
      ...createCompositionGraphDraft(),
      variables: [scalarNode],
      components: [{
        id: "table_1",
        kind: "table" as const,
        variable: "total",
      }],
    };
    expect(compositionBindingDiagnostics(draft)).toEqual([
      "unplaced_component",
      "variable_type_mismatch",
    ]);
  });
});
