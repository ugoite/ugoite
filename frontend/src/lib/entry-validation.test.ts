import { describe, expect, it } from "vitest";
import {
  invalidFieldsFromError,
  toRustFormDefinition,
  validateEntryDraftViaWasm,
} from "~/lib/entry-validation";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import type { Form } from "~/lib/types";

const testForm = (): Form => ({
  name: "Note",
  version: 1,
  template: "# Title",
  fields: {
    Body: { type: "string", required: true },
    Done: { type: "boolean", required: false },
    Count: { type: "integer", required: false },
    Due: { type: "date", required: false },
    At: { type: "timestamp", required: false },
    Tags: { type: "list", required: false },
  },
});

describe("entry-validation", () => {
  it("converts frontend forms to the Rust shape without coercion", () => {
    const rust = toRustFormDefinition(testForm()) as {
      name: string;
      fields: Array<{ name: string; field_type: string }>;
    };
    expect(rust.name).toBe("Note");
    expect(rust.fields.find((field) => field.name === "Done")?.field_type)
      .toBe("boolean");
    expect(rust.fields.find((field) => field.name === "Count")?.field_type)
      .toBe("integer");
  });

  it("extracts invalid fields from Rust diagnostics", () => {
    const error = new UgoiteApiError({
      kind: "entry_validation",
      message: "Entry form validation failed",
      code: "FORM_VALIDATION_FAILED",
      detail: {
        warnings: [
          {
            code: "invalid_type",
            field: "Done",
            expected_type: "boolean",
            expected_format: "true, false, yes, no, on, off, 1, or 0",
            reason: "value does not match",
            message: "Field 'Done' has invalid type",
          },
        ],
      },
    });
    expect(invalidFieldsFromError(error)).toEqual(["Done"]);
  });

  it("uses one Rust classification for boolean/number/date/timestamp/list", async () => {
    const cases: Array<[string, unknown]> = [
      ["Done", "maybe"],
      ["Count", "not-an-int"],
      ["Due", "2026-13-40"],
      ["At", "not-a-timestamp"],
      ["Tags", 42],
    ];
    for (const [field, value] of cases) {
      const result = await validateEntryDraftViaWasm(testForm(), {
        title: "T",
        tags: [],
        fields: { Body: "hello", [field]: value },
      });
      expect(result.ok).toBe(false);
      if (!result.ok) {
        // Same code the server mutation surfaces for the same draft.
        expect(result.code).toBe("FORM_VALIDATION_FAILED");
        expect(result.invalidFields).toEqual([field]);
      }
    }
  });

  it("accepts a valid draft through the shared boundary", async () => {
    const result = await validateEntryDraftViaWasm(testForm(), {
      title: "T",
      tags: [],
      fields: { Body: "hello", Done: "yes", Count: 3 },
    });
    expect(result.ok).toBe(true);
  });
});

describe("lane1 parity fixture", () => {
  // Mirrors fixtures/entry/structured-compat/10-structured-authoring-parity.json
  // invalid_cases: the browser surface must converge on the same codes and
  // fields as core preview, the WASM bridge, and both CLI modes.
  const parityForm = (): Form => ({
    name: "Parity",
    version: 1,
    template: "# Parity",
    fields: {
      Title: { type: "string", required: true },
      Notes: { type: "markdown", required: false },
      Done: { type: "boolean", required: false },
      Count: { type: "integer", required: false },
      Score: { type: "double", required: false },
      Due: { type: "date", required: false },
      At: { type: "timestamp", required: false },
      Tags: { type: "list", required: false },
      Rows: { type: "object_list", required: false },
      Ref: { type: "row_reference", required: false, target_form: "Task" },
      File: { type: "asset_reference", required: false },
      Files: {
        type: "list",
        required: false,
        items: { type: "asset_reference" },
      },
    },
  });

  it("converges on the same invalid codes and fields as the fixture", async () => {
    const cases: Array<{
      field: string;
      fields: Record<string, unknown>;
      code: string;
    }> = [
      {
        field: "Done",
        fields: { Title: "hello", Done: "maybe" },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "Count",
        fields: { Title: "hello", Count: "not-an-int" },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "Score",
        fields: { Title: "hello", Score: "not-a-number" },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "Due",
        fields: { Title: "hello", Due: "tomorrow" },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "At",
        fields: { Title: "hello", At: "not-a-timestamp" },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "Tags",
        fields: { Title: "hello", Tags: 42 },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "Rows",
        fields: { Title: "hello", Rows: "not-an-array" },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "Ref",
        fields: { Title: "hello", Ref: 7 },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "File",
        fields: { Title: "hello", File: { asset_id: "not-a-uuid" } },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "Title",
        fields: { Done: true },
        code: "FORM_VALIDATION_FAILED",
      },
      {
        field: "Nope",
        fields: { Title: "hello", Nope: "x" },
        code: "UNKNOWN_FORM_FIELDS",
      },
    ];
    for (const { field, fields, code } of cases) {
      const result = await validateEntryDraftViaWasm(parityForm(), {
        title: "T",
        tags: [],
        fields,
      });
      expect(result.ok).toBe(false);
      if (!result.ok) {
        expect(result.code).toBe(code);
        expect(result.invalidFields).toEqual([field]);
      }
    }
  });

  it("accepts the fixture valid draft and reopens it unchanged", async () => {
    const fields = {
      Title: "hello",
      Notes: "Some *markdown* body.",
      Done: true,
      Count: 42,
      Score: 3.14,
      Due: "2026-09-11",
      At: "2026-09-11T10:00:00",
      Tags: ["alpha", "beta"],
      Rows: [{ step: "one" }],
      Ref: "task-01",
    };
    const first = await validateEntryDraftViaWasm(parityForm(), {
      title: "Website",
      tags: ["inbox"],
      fields,
    });
    expect(first.ok).toBe(true);
    // Reopen resolves through the same boundary to the same outcome.
    const second = await validateEntryDraftViaWasm(parityForm(), {
      title: "Website",
      tags: ["inbox"],
      fields,
    });
    expect(second).toEqual(first);
  });
});
