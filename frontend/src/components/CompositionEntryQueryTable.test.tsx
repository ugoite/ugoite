import { describe, expect, it, vi } from "vitest";
import { entryQueryDisplayColumns } from "./CompositionEntryQueryTable";
import type { CompositionResolvedSource } from "~/lib/composition-api";
import type { EntryQueryResult } from "~/lib/entry-query";

const source = (
  projection: Extract<
    CompositionResolvedSource,
    { kind: "entry_query" }
  >["request"]["projection"],
): Extract<CompositionResolvedSource, { kind: "entry_query" }> => ({
  kind: "entry_query",
  source_id: "entries",
  request: {
    query: {
      scope: { kind: "form", form_id: "form-1" },
      filters: [],
      sort: [],
    },
    projection,
    limit: 100,
  },
  source_schema_fingerprint: "fingerprint",
});

const entryRow = (): EntryQueryResult => ({
  id: "entry-1",
  form_id: "form-1",
  revision_id: "revision-1",
  created_at_micros: 1_772_960_000_000_000,
  updated_at_micros: 1_772_963_000_000_000,
  properties: { purpose: "Travel" },
  preview: "Travel entry",
});

const names = (formId: string, fieldId: number): string | undefined =>
  formId === "form-1" && fieldId === 7 ? "purpose" : undefined;

describe("entryQueryDisplayColumns", () => {
  it("renders preview projections as Preview, Created, Updated", () => {
    const columns = entryQueryDisplayColumns(
      source({ kind: "preview" }),
      [entryRow()],
      names,
    );
    expect(columns.map((column) => column.label)).toEqual([
      "Preview",
      "Created",
      "Updated",
    ]);
    expect(columns[0].text(entryRow())).toBe("Travel entry");
  });

  it("renders fields projections in projection order with timestamps last", () => {
    const fieldNames = vi.fn((
      formId: string,
      fieldId: number,
      sourceId?: string,
    ) => sourceId === "entries" ? names(formId, fieldId) : undefined);
    const columns = entryQueryDisplayColumns(
      source({
        kind: "fields",
        fields: [
          { kind: "property", field_id: 7 },
          { kind: "created_at" },
        ],
      }),
      [entryRow()],
      fieldNames,
      (formId, fieldId, sourceId) =>
        sourceId === "entries" ? names(formId, fieldId) : undefined,
    );
    expect(columns.map((column) => column.label)).toEqual([
      "purpose",
      "Created",
    ]);
    expect(fieldNames).toHaveBeenCalledWith("form-1", 7, "entries");
    expect(columns[0].text(entryRow())).toBe("Travel");
  });

  it("uses the Form key for values when its display label differs", () => {
    const columns = entryQueryDisplayColumns(
      source({
        kind: "fields",
        fields: [{ kind: "property", field_id: 7 }],
      }),
      [entryRow()],
      () => "Purpose of travel",
      () => "purpose",
    );
    expect(columns.map((column) => column.label)).toEqual([
      "Purpose of travel",
    ]);
    expect(columns[0].text(entryRow())).toBe("Travel");
  });

  it("falls back to the current row key order without field metadata", () => {
    const columns = entryQueryDisplayColumns(
      source({
        kind: "fields",
        fields: [{ kind: "property", field_id: 7 }],
      }),
      [entryRow()],
      undefined,
    );
    expect(columns.map((column) => column.label)).toEqual(["purpose"]);
    expect(columns[0].text(entryRow())).toBe("Travel");
  });
});
