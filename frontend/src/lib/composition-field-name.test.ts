import { describe, expect, it } from "vitest";
import { compositionFormFieldName } from "./composition-field-name";
import type { Form } from "./types";

const form = (field: Record<string, unknown>): Form => ({
  id: "form-1",
  name: "Expenses",
  version: 1,
  template: "entry",
  fields: { amount: field as never },
});

describe("compositionFormFieldName", () => {
  it("prefers the explicit Form label", () => {
    expect(
      compositionFormFieldName(
        [form({
          id: 1,
          type: "number",
          required: false,
          label: "Amount spent",
        })],
        "form-1",
        1,
      ),
    ).toBe("Amount spent");
  });

  it("uses the query capability name when the label is absent", () => {
    expect(
      compositionFormFieldName(
        [form({
          id: 1,
          type: "number",
          required: false,
          query_capability: {
            field: { kind: "property", field_id: 1 },
            name: "Amount",
            field_type: "number",
            filterable: true,
            sortable: true,
            projectable: true,
            supported_operators: [],
          },
        })],
        "form-1",
        1,
      ),
    ).toBe("Amount");
  });

  it("uses the Form field key when only the field identity is available", () => {
    expect(
      compositionFormFieldName(
        [form({ id: 1, type: "number", required: false })],
        "form-1",
        1,
      ),
    ).toBe("amount");
  });

  it("falls back from generated ordinals to human keys, not opaque IDs", () => {
    for (const name of ["項目 1", "フィールド 3", "Field 4"]) {
      const generated = form({
        id: 1,
        type: "number",
        required: false,
        query_capability: {
          field: { kind: "property", field_id: 1 },
          name,
          field_type: "number",
          filterable: true,
          sortable: true,
          projectable: true,
          supported_operators: [],
        },
      });
      expect(compositionFormFieldName([generated], "form-1", 1)).toBe(
        "amount",
      );
    }
    const ordinal = form({
      id: 1,
      type: "number",
      required: false,
      query_capability: {
        field: { kind: "property", field_id: 1 },
        name: "項目 1",
        field_type: "number",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: [],
      },
    });
    expect(compositionFormFieldName([ordinal], "form-1", 1)).toBe("amount");
    expect(
      compositionFormFieldName(
        [{
          ...ordinal,
          fields: {
            "101": { id: 1, type: "number", required: false } as never,
          },
        }],
        "form-1",
        1,
      ),
    ).toBeUndefined();
  });

  it("does not resolve a different Form by its display name", () => {
    expect(
      compositionFormFieldName(
        [form({ id: 1, type: "number", required: false })],
        "Expenses",
        1,
      ),
    ).toBeUndefined();
  });
});
