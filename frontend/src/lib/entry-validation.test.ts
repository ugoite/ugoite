import { describe, expect, it } from "vitest";
import {
  invalidFieldsFromError,
  toRustFormDefinition,
  validateEntryDraftViaWasm,
} from "~/lib/entry-validation";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import type { Form } from "~/lib/types";

const testForm = (): Form => ({
  id: "00000000-0000-0000-0000-000000000001",
  name: "Note",
  version: 1,
  template: "# Title",
  fields: {
    Body: { id: 100, type: "string", required: true },
    Done: { id: 101, type: "boolean", required: false },
    Count: { id: 102, type: "integer", required: false },
    Due: { id: 103, type: "date", required: false },
    At: { id: 104, type: "timestamp", required: false },
    Tags: { id: 105, type: "list", required: false },
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

  it.each(
    [
      ["deny", false],
      ["allow_json", true],
      ["allow_columns", true],
    ] as const,
  )("preserves the durable Form policy: %s", (policy, allowed) => {
    const rust = toRustFormDefinition({
      ...testForm(),
      allow_extra_attributes: policy,
    });
    expect(rust.allow_extra_attributes).toBe(allowed);
    expect(rust.extension_metadata).toEqual({
      "ugoite.extra_attributes_policy": policy,
    });
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

  it("rejects missing stable Form and Field identities without synthesizing them", () => {
    expect(() =>
      toRustFormDefinition({
        ...testForm(),
        id: undefined,
      })
    ).toThrowError(/missing its stable FormId/);
    expect(() =>
      toRustFormDefinition({
        ...testForm(),
        fields: { Body: { type: "string", required: true } },
      })
    ).toThrowError(/missing its stable FieldId/);
  });

  it("resolves human-readable reference Form names through the loaded catalog", () => {
    const target: Form = {
      id: "00000000-0000-0000-0000-000000000002",
      name: "Project",
      version: 1,
      template: "# Project",
      fields: {},
    };
    const source: Form = {
      id: "00000000-0000-0000-0000-000000000003",
      name: "Task",
      version: 1,
      template: "# Task",
      fields: {
        Project: {
          id: 100,
          type: "row_reference",
          required: false,
          target_form: "Project",
        },
      },
    };
    const rust = toRustFormDefinition(source, [target]) as {
      id: string;
      fields: Array<{ id: number; reference_form?: string }>;
    };
    expect(rust.id).toBe(source.id);
    expect(rust.fields[0]).toMatchObject({
      id: 100,
      reference_form: target.id,
    });
  });

  it("rejects an unresolved reference as a typed admission error", () => {
    const form: Form = {
      ...testForm(),
      fields: {
        Link: {
          id: 100,
          type: "row_reference",
          required: false,
          target_form: "MissingForm",
        },
      },
    };
    expect(() => toRustFormDefinition(form)).toThrow(UgoiteApiError);
    try {
      toRustFormDefinition(form);
    } catch (error) {
      expect(error).toBeInstanceOf(UgoiteApiError);
      expect((error as UgoiteApiError).code).toBe("INVALID_INPUT");
      expect((error as UgoiteApiError).detail).toMatchObject({
        kind: "form_identity",
        target_form: "MissingForm",
      });
    }
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
