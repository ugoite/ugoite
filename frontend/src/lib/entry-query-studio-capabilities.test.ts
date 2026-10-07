import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearStudioFormDefinitionCache,
  fetchStudioFormDefinition,
  studioBindingName,
  studioCapabilitiesFromForm,
  studioEntryFilterToComposition,
  studioEntrySortToComposition,
  studioFallbackCapabilities,
  studioFieldNames,
  studioFilterToEntryFilter,
  studioParseBindingText,
  studioSortToEntrySort,
} from "./entry-query-studio-capabilities";
import type { Form } from "./types";

const { formListMock } = vi.hoisted(() => ({ formListMock: vi.fn() }));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: {
    list: (...args: unknown[]) =>
      (formListMock as (...call: unknown[]) => unknown)(...args),
  },
}));

const capableForm = (): Form => ({
  id: "11111111-1111-4111-8111-111111111111",
  name: "Expenses",
  version: 1,
  template: "",
  fields: {
    title: {
      id: 101,
      type: "string",
      required: true,
      query_capability: {
        field: { kind: "property", field_id: 101 },
        name: "Title",
        field_type: "string",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals", "contains"],
      },
    },
    legacy: {
      id: 102,
      type: "string",
      required: false,
    },
  },
});

describe("entry-query-studio-capabilities", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    clearStudioFormDefinitionCache();
  });

  it("round-trips parameter bindings through display text", () => {
    expect(studioBindingName({ parameter: "month" })).toBe("month");
    expect(studioBindingName("plain")).toBeUndefined();
    expect(studioBindingName({ parameter: "", extra: 1 })).toBeUndefined();
    expect(studioParseBindingText("{{month}}")).toEqual({
      parameter: "month",
    });
    expect(studioParseBindingText("lunch")).toBe("lunch");
  });

  it("maps draft filters through the dialog shape without flattening bindings", () => {
    expect(
      studioFilterToEntryFilter({
        field_id: 101,
        operator: "equals",
        value: { parameter: "month" },
      }),
    ).toEqual({
      field: { kind: "property", field_id: 101 },
      operator: "equals",
      value: "{{month}}",
    });
    expect(
      studioEntryFilterToComposition({
        field: { kind: "property", field_id: 101 },
        operator: "contains",
        value: "{{month}}",
      }),
    ).toEqual({
      field_id: 101,
      operator: "contains",
      value: { parameter: "month" },
    });
    expect(
      studioEntryFilterToComposition({
        field: { kind: "property", field_id: 101 },
        operator: "equals",
        value: "lunch",
      }),
    ).toEqual({ field_id: 101, operator: "equals", value: "lunch" });
  });

  it("fails closed on non-property refs from the dialog", () => {
    expect(
      studioEntryFilterToComposition({
        field: { kind: "created_at" },
        operator: "equals",
        value: "x",
      }),
    ).toBeUndefined();
    expect(
      studioEntrySortToComposition({
        field: { kind: "updated_at" },
        direction: "asc",
      }),
    ).toBeUndefined();
    expect(
      studioSortToEntrySort({ field_id: 101, direction: "desc" }),
    ).toEqual({
      field: { kind: "property", field_id: 101 },
      direction: "desc",
    });
  });

  it("derives names and capabilities from the form definition", () => {
    const form = capableForm();
    expect(studioFieldNames(form).get(101)).toBe("Title");
    // Legacy fields without backend metadata keep their record-key name.
    expect(studioFieldNames(form).get(102)).toBe("legacy");
    const capabilities = studioCapabilitiesFromForm(form);
    expect(
      capabilities.find((field) => field.name === "Title"),
    ).toMatchObject({
      field_type: "string",
      filterable: true,
      sortable: true,
      supported_operators: ["equals", "contains"],
    });
    expect(
      capabilities.find((field) => field.name === "legacy"),
    ).toMatchObject({
      filterable: true,
      sortable: true,
      supported_operators: [
        "equals",
        "contains",
        "lt",
        "lte",
        "gt",
        "gte",
      ],
    });
  });

  it("falls back to schema field IDs without a definition", () => {
    expect(studioFieldNames(undefined).size).toBe(0);
    expect(
      studioFallbackCapabilities([{ field_id: 100, field_type: "date" }]),
    ).toMatchObject([{
      field: { kind: "property", field_id: 100 },
      name: "100",
      field_type: "date",
      filterable: true,
      sortable: true,
    }]);
  });

  it("caches the transient form definition per source", async () => {
    formListMock.mockResolvedValue([capableForm()]);
    const first = await fetchStudioFormDefinition(
      "space-1",
      "11111111-1111-4111-8111-111111111111",
    );
    const second = await fetchStudioFormDefinition(
      "space-1",
      "11111111-1111-4111-8111-111111111111",
    );
    expect(first?.name).toBe("Expenses");
    expect(second?.name).toBe("Expenses");
    expect(formListMock).toHaveBeenCalledTimes(1);
    await expect(
      fetchStudioFormDefinition("space-1", "missing-form"),
    ).resolves.toBeUndefined();
  });
});
