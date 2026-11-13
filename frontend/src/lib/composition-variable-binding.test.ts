import { describe, expect, it } from "vitest";
import { canonicalizeCompositionDocument } from "./ugoite-client";
import {
  appendCompositionGraphComponent,
  compositionBindingDiagnostics,
  compositionGraphDocumentDraft,
  compositionGraphAuthoringDocument,
  compositionGraphDraftFromDocument,
  createCompositionGraphDraft,
  removeCompositionGraphComponent,
  renameCompositionVariable,
  setCompositionComponentVariable,
  type CompositionGraphDraft,
  type CompositionVariableNode,
} from "./composition-variable-binding";

const rows: CompositionVariableNode = {
  id: "rows",
  kind: "query",
  returns: {
    kind: "table",
    columns: [{
      name: "amount",
      value_type: { scalar_type: "integer", nullable: false },
    }],
  },
  query: { kind: "inline_sql", sql: "SELECT amount", arguments: [] },
};
const total: CompositionVariableNode = {
  id: "total",
  kind: "query",
  returns: {
    kind: "scalar",
    value_type: { scalar_type: "integer", nullable: false },
  },
  query: { kind: "inline_sql", sql: "SELECT COUNT(*)", arguments: [] },
};

describe("Composition graph authoring model", () => {
  it("uses a temporary name only for authoring preview and keeps save strict", async () => {
    const draft = {
      ...createCompositionGraphDraft(" "),
      components: [{
        id: "text_1",
        kind: "text" as const,
        content: "Draft",
        style: "body" as const,
      }],
      layout: {
        kind: "flow" as const,
        rows: [{
          id: "main",
          items: [{ kind: "component" as const, component: "text_1" }],
        }],
      },
    };
    await expect(canonicalizeCompositionDocument(
      compositionGraphAuthoringDocument(draft),
    )).resolves.toHaveProperty("canonical_yaml");
    await expect(canonicalizeCompositionDocument(
      compositionGraphDocumentDraft(draft),
    )).rejects.toThrow();
    expect(draft.name).toBe(" ");
  });

  it("serializes Text content as a template string and accepts only current component kinds", () => {
    const draft: CompositionGraphDraft = {
      ...createCompositionGraphDraft("Report"),
      variables: [rows],
      components: [
        { id: "table_1", kind: "table", variable: "rows" },
        { id: "text_1", kind: "text", content: "Total: {{total}}", style: "body" },
      ],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [
            { kind: "component", component: "table_1" },
            { kind: "component", component: "text_1" },
          ],
        }],
      },
    };
    const document = compositionGraphDocumentDraft(draft);

    expect(document.spec.components).toEqual(draft.components);
    expect(compositionGraphDraftFromDocument(document)).toEqual(draft);
  });

  it("rejects a Saved SQL query variant", () => {
    const query = {
      kind: "saved_sql",
      entry_id: "sql",
      revision_id: "rev",
      arguments: [],
    };
    const document = compositionGraphDocumentDraft({
      ...createCompositionGraphDraft("Report"),
      variables: [{
        ...rows,
        query,
      } as unknown as CompositionVariableNode],
    });
    expect(() => compositionGraphDraftFromDocument(document)).toThrow();
  });

  it.each([
    ["Metric", { id: "metric_1", kind: "metric", variable: "total" }],
    ["Saved SQL display", { id: "saved_1", kind: "saved_sql", variable: "rows" }],
    ["unknown component", { id: "unknown_1", kind: "chart", variable: "rows" }],
    ["object Text content", { id: "text_1", kind: "text", content: { kind: "literal", text: "x" }, style: "body" }],
    ["Table presentation", { id: "table_1", kind: "table", variable: "rows", presentation: {} }],
  ])("rejects the removed %s component shape", (_label, component) => {
    const document = {
      ...compositionGraphDocumentDraft(createCompositionGraphDraft("Report")),
      spec: {
        ...compositionGraphDocumentDraft(createCompositionGraphDraft("Report")).spec,
        components: [component],
      },
    };
    expect(() => compositionGraphDraftFromDocument(document)).toThrow();
  });

  it("adds, rebinds, and removes a Table component without changing query definitions", () => {
    let draft: CompositionGraphDraft = {
      ...createCompositionGraphDraft("Report"),
      variables: [rows, { ...rows, id: "other_rows" }],
    };
    draft = appendCompositionGraphComponent(draft, {
      id: "table_1",
      kind: "table",
      variable: "rows",
    })!;
    const rebound = setCompositionComponentVariable(
      draft,
      "table_1",
      "other_rows",
    );
    expect(rebound?.components[0]).toEqual({
      id: "table_1",
      kind: "table",
      variable: "other_rows",
    });
    expect(rebound?.variables).toEqual(draft.variables);
    expect(removeCompositionGraphComponent(rebound!, "table_1")?.components)
      .toEqual([]);
  });

  it("renames graph IDs in SQL bindings and placed references", () => {
    const draft: CompositionGraphDraft = {
      ...createCompositionGraphDraft("Report"),
      variables: [
        {
          id: "day",
          kind: "input",
          value_type: { scalar_type: "date", nullable: false },
          required: true,
        },
        {
          ...total,
          query: {
            kind: "inline_sql",
            sql: "SELECT COUNT(*) WHERE day = {{day}}",
            arguments: [{ name: "day", variable: "day" }],
          },
        },
      ],
      components: [{ id: "input_1", kind: "input", variable: "day" }],
      layout: {
        kind: "flow",
        rows: [{
          id: "main",
          items: [{ kind: "component", component: "input_1" }],
        }],
      },
    };

    const renamed = renameCompositionVariable(draft, "day", "report_day");

    expect(renamed?.variables[0].id).toBe("report_day");
    expect(renamed?.variables[1].kind === "query" &&
      renamed.variables[1].query.arguments[0].variable).toBe("report_day");
    expect(renamed?.components[0]).toEqual({
      id: "input_1",
      kind: "input",
      variable: "report_day",
    });
  });

  it("keeps structural diagnostics for missing variables and unplaced components", () => {
    const draft: CompositionGraphDraft = {
      ...createCompositionGraphDraft("Report"),
      variables: [rows],
      components: [
        { id: "table_1", kind: "table", variable: "missing" },
        { id: "text_1", kind: "text", content: "unplaced", style: "body" },
      ],
    };

    expect(compositionBindingDiagnostics(draft)).toEqual([
      "unplaced_component",
      "missing_variable",
    ]);
  });
});
