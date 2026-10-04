import { describe, expect, it } from "vitest";
import {
  buildEntryQueryComposition,
  buildEntryQueryCompositionDocument,
} from "./entry-query-composition";
import type { EntryProjection, EntryQuery } from "./entry-query";
import type { Form } from "./types";

const formId = "00000000-0000-7000-8000-000000000001";
const targetFormId = "00000000-0000-7000-8000-000000000002";

const noteForm: Form = {
  id: formId,
  name: "Notes",
  version: 1,
  template: "",
  fields: {
    title: {
      id: 101,
      type: "string",
      required: true,
      query_capability: {
        field: { kind: "property", field_id: 101 },
        name: "title",
        field_type: "string",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals", "contains"],
      },
    },
    pinned: {
      id: 102,
      type: "boolean",
      required: false,
    },
  },
};

const query: EntryQuery = {
  scope: { kind: "form", form_id: formId },
  text: "rent",
  filters: [
    {
      field: { kind: "property", field_id: 101 },
      operator: "contains",
      value: "rent",
    },
  ],
  sort: [{ field: { kind: "property", field_id: 102 }, direction: "desc" }],
};

const projection: EntryProjection = {
  kind: "fields",
  fields: [
    { kind: "property", field_id: 101 },
    { kind: "property", field_id: 102 },
  ],
};

describe("buildEntryQueryComposition", () => {
  it("round-trips a form-scope text+filter+sort+fields view to a source fragment", () => {
    const result = buildEntryQueryComposition({
      query,
      projection,
      form: noteForm,
    });

    expect(result).toEqual({
      status: "ok",
      source: {
        id: "entry_rows",
        kind: "entry_query",
        form_id: formId,
        query: {
          text: "rent",
          filters: [{ field_id: 101, operator: "contains", value: "rent" }],
          sort: [{ field_id: 102, direction: "desc" }],
          projection: { kind: "fields", fields: [101, 102] },
        },
      },
      fieldSchema: [
        { field_id: 101, field_type: "string" },
        { field_id: 102, field_type: "boolean" },
      ],
      warnings: [],
    });
  });

  it("snapshots every form field for a preview projection", () => {
    const result = buildEntryQueryComposition({
      query: {
        scope: { kind: "form", form_id: formId },
        filters: [],
        sort: [],
      },
      projection: { kind: "preview" },
      form: noteForm,
    });

    // Preview returns the whole row, so the resolver requires the snapshot
    // to cover every current field (else `source_schema_changed`).
    expect(result).toEqual({
      status: "ok",
      source: {
        id: "entry_rows",
        kind: "entry_query",
        form_id: formId,
        query: { filters: [], sort: [], projection: { kind: "preview" } },
      },
      fieldSchema: [
        { field_id: 101, field_type: "string" },
        { field_id: 102, field_type: "boolean" },
      ],
      warnings: [],
    });
  });

  it("snapshots text-searchable fields when text is set", () => {
    const result = buildEntryQueryComposition({
      query: {
        scope: { kind: "form", form_id: formId },
        text: "rent",
        filters: [],
        sort: [],
      },
      projection: {
        kind: "fields",
        fields: [{ kind: "property", field_id: 102 }],
      },
      form: noteForm,
    });

    // The resolver derives the text-search field set from the live Form, so
    // the snapshot must carry it even though only field 102 is projected.
    expect(result.status).toBe("ok");
    if (result.status !== "ok") return;
    expect(result.source.query.text).toBe("rent");
    expect(result.fieldSchema).toEqual([
      { field_id: 101, field_type: "string" },
      { field_id: 102, field_type: "boolean" },
    ]);
  });

  it("drops system refs from a fields projection instead of failing", () => {
    const result = buildEntryQueryComposition({
      query: {
        scope: { kind: "form", form_id: formId },
        filters: [],
        sort: [],
      },
      projection: {
        kind: "fields",
        fields: [
          { kind: "property", field_id: 101 },
          { kind: "created_at" },
          { kind: "updated_at" },
        ],
      },
      form: noteForm,
    });

    expect(result.status).toBe("ok");
    if (result.status !== "ok") return;
    expect(result.source.query.projection).toEqual({
      kind: "fields",
      fields: [101],
    });
    expect(result.fieldSchema).toEqual([
      { field_id: 101, field_type: "string" },
    ]);
  });

  it("snapshots row_reference targets and list item schemas", () => {
    const form: Form = {
      id: formId,
      name: "Tasks",
      version: 1,
      template: "",
      fields: {
        project: {
          id: 111,
          type: "row_reference",
          required: false,
          target_form: "Projects",
        },
        tags: {
          id: 112,
          type: "list",
          required: false,
          items: { type: "string" },
        },
      },
    };
    const projects: Form = {
      id: targetFormId,
      name: "Projects",
      version: 1,
      template: "",
      fields: {},
    };
    const result = buildEntryQueryComposition({
      query: {
        scope: { kind: "form", form_id: formId },
        filters: [],
        sort: [],
      },
      projection: {
        kind: "fields",
        fields: [
          { kind: "property", field_id: 111 },
          { kind: "property", field_id: 112 },
        ],
      },
      form,
      knownForms: [projects],
    });

    expect(result.status).toBe("ok");
    if (result.status !== "ok") return;
    expect(result.fieldSchema).toEqual([
      {
        field_id: 111,
        field_type: "row_reference",
        reference_form: targetFormId,
      },
      { field_id: 112, field_type: "list", items: { type: "string" } },
    ]);
  });

  it("reports All-scope as inexpressible without a form", () => {
    expect(
      buildEntryQueryComposition({
        query: { scope: { kind: "all" }, filters: [], sort: [] },
        projection: { kind: "preview" },
      }),
    ).toEqual({
      status: "inexpressible",
      reason: "entryQueryToolSave.needsForm",
    });
  });

  it("reports a missing form definition as inexpressible", () => {
    expect(
      buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: formId },
          filters: [],
          sort: [],
        },
        projection: { kind: "preview" },
      }),
    ).toEqual({
      status: "inexpressible",
      reason: "entryQueryToolSave.needsForm",
    });
  });

  it("reports a system-ref filter as inexpressible", () => {
    expect(
      buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: formId },
          filters: [
            { field: { kind: "created_at" }, operator: "gte", value: 100 },
          ],
          sort: [],
        },
        projection: { kind: "preview" },
        form: noteForm,
      }),
    ).toEqual({
      status: "inexpressible",
      reason: "entryQueryToolSave.unsupportedQuery",
    });
  });

  it("reports a system-ref sort as inexpressible", () => {
    expect(
      buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: formId },
          filters: [],
          sort: [{ field: { kind: "updated_at" }, direction: "desc" }],
        },
        projection: { kind: "preview" },
        form: noteForm,
      }),
    ).toEqual({
      status: "inexpressible",
      reason: "entryQueryToolSave.unsupportedQuery",
    });
  });

  it("reports an unknown field type as inexpressible", () => {
    const form: Form = {
      ...noteForm,
      fields: {
        odd: { id: 109, type: "future_type", required: false },
      },
    };
    expect(
      buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: formId },
          filters: [
            {
              field: { kind: "property", field_id: 109 },
              operator: "equals",
              value: "x",
            },
          ],
          sort: [],
        },
        projection: { kind: "preview" },
        form,
      }),
    ).toEqual({
      status: "inexpressible",
      reason: "entryQueryToolSave.unsupportedQuery",
    });
  });

  it("reports non-scalar filter values as inexpressible", () => {
    for (
      const value of [
        { nested: "object" },
        ["list"],
        Number.NaN,
        Number.POSITIVE_INFINITY,
      ]
    ) {
      expect(
        buildEntryQueryComposition({
          query: {
            scope: { kind: "form", form_id: formId },
            filters: [
              {
                field: { kind: "property", field_id: 101 },
                operator: "equals",
                value,
              },
            ],
            sort: [],
          },
          projection: { kind: "preview" },
          form: noteForm,
        }),
      ).toEqual({
        status: "inexpressible",
        reason: "entryQueryToolSave.unsupportedQuery",
      });
    }
  });

  it("reports a non-string text query as inexpressible", () => {
    expect(
      buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: formId },
          text: 42 as unknown as string,
          filters: [],
          sort: [],
        },
        projection: { kind: "preview" },
        form: noteForm,
      }),
    ).toEqual({
      status: "inexpressible",
      reason: "entryQueryToolSave.unsupportedQuery",
    });
  });

  it("reports a query field missing from the form as inexpressible", () => {
    expect(
      buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: formId },
          filters: [],
          sort: [{
            field: { kind: "property", field_id: 999 },
            direction: "asc",
          }],
        },
        projection: { kind: "preview" },
        form: noteForm,
      }),
    ).toEqual({
      status: "inexpressible",
      reason: "entryQueryToolSave.unsupportedQuery",
    });
  });

  it("reports an unresolvable reference target as inexpressible", () => {
    const form: Form = {
      id: formId,
      name: "Tasks",
      version: 1,
      template: "",
      fields: {
        project: {
          id: 111,
          type: "row_reference",
          required: false,
          target_form: "Missing",
        },
      },
    };
    expect(
      buildEntryQueryComposition({
        query: {
          scope: { kind: "form", form_id: formId },
          filters: [],
          sort: [],
        },
        projection: {
          kind: "fields",
          fields: [{ kind: "property", field_id: 111 }],
        },
        form,
        knownForms: [],
      }),
    ).toEqual({
      status: "inexpressible",
      reason: "entryQueryToolSave.unsupportedQuery",
    });
  });
});

describe("buildEntryQueryCompositionDocument", () => {
  it("binds one table component to the source with no parameters", () => {
    const built = buildEntryQueryComposition({
      query,
      projection,
      form: noteForm,
    });
    expect(built.status).toBe("ok");
    if (built.status !== "ok") return;

    const document = buildEntryQueryCompositionDocument(
      "  Notes view  ",
      built.source,
      built.fieldSchema,
    );

    expect(document).toEqual({
      format: "ugoite.composition",
      format_version: 1,
      kind: "dashboard",
      name: "Notes view",
      tags: [],
      spec: {
        parameters: [],
        sources: [
          { ...built.source, field_schema: built.fieldSchema },
        ],
        components: [{
          id: "results_table",
          kind: "table",
          source: "entry_rows",
        }],
        sections: [{ id: "main", components: ["results_table"] }],
      },
    });
    expect(JSON.stringify(document)).not.toContain("after");
    expect(JSON.stringify(document)).not.toContain("continuation");
  });
});
