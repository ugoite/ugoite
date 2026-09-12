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
