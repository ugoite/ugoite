// REQ-FE-004: canonical EntryBrowser display and query controls
// REQ-FE-008: EntryBrowser selection remains separate from mutation
import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EntryBrowser } from "./EntryBrowser";
import {
  createEntryQueryController,
  type EntryQueryCapabilities,
  systemEntryCapabilities,
} from "~/lib/entry-query";
const queryMock = vi.fn();

describe("EntryBrowser", () => {
  beforeEach(() => queryMock.mockReset());

  it("connects toolbar actions to canonical projection, text, and sort state", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-1",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: 1_772_960_000_000_000,
        updated_at_micros: 1_772_960_000_000_000,
        preview: "Readable entry",
      }],
      has_more: false,
    });
    const controller = createEntryQueryController(
      () => "space-1",
      undefined,
      undefined,
      50,
      queryMock,
    );
    await controller.load();
    render(() => (
      <EntryBrowser
        mode="select_one"
        controller={controller}
        capabilities={systemEntryCapabilities({
          kind: "form",
          form_id: "form-1",
        })}
      />
    ));
    expect(screen.getByText("Readable entry")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Filter"));
    fireEvent.input(screen.getByRole("searchbox"), {
      target: { value: "alice" },
    });
    expect(controller.query().text).toBe("alice");

    fireEvent.click(screen.getByText("Sort"));
    fireEvent.click(screen.getByRole("button", { name: "Add sort" }));
    expect(controller.query().sort).toHaveLength(1);
    expect(controller.query().sort[0].field).toEqual({ kind: "created_at" });

    fireEvent.click(screen.getAllByText("Columns")[0]);
    const updatedColumn = screen.getByRole("checkbox", { name: "Updated" });
    fireEvent.click(updatedColumn);
    expect(controller.projection()).toEqual({
      kind: "fields",
      fields: [{ kind: "updated_at" }],
    });
    expect(await screen.findByText("Use this entry")).toBeInTheDocument();
  });

  it("keeps selection separate from server mutation", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "stable-entry-id",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: 1_772_960_000_000_000,
        updated_at_micros: 1_772_960_000_000_000,
        preview: "Target row",
      }],
      has_more: false,
    });
    const onSelect = vi.fn();
    const controller = createEntryQueryController(
      () => "space-1",
      undefined,
      undefined,
      50,
      queryMock,
    );
    await controller.load();
    render(() => (
      <EntryBrowser
        mode="select_one"
        controller={controller}
        capabilities={systemEntryCapabilities({
          kind: "form",
          form_id: "form-1",
        })}
        onSelect={onSelect}
      />
    ));
    fireEvent.click(screen.getByRole("listitem"));

    expect(onSelect).toHaveBeenCalledWith(
      expect.objectContaining({ id: "stable-entry-id" }),
    );
    expect(queryMock).toHaveBeenCalledTimes(1);
  });

  it("maps the filter builder to EntryQuery filters", async () => {
    queryMock.mockResolvedValue({ rows: [], has_more: false });
    const capabilities: EntryQueryCapabilities = {
      ...systemEntryCapabilities({ kind: "form", form_id: "form-1" }),
      fields: [
        ...systemEntryCapabilities({ kind: "form", form_id: "form-1" })
          .fields,
        {
          field: { kind: "property", field_id: 7 },
          name: "status",
          field_type: "string",
          filterable: true,
          sortable: true,
          projectable: true,
          supported_operators: ["equals", "contains"],
        },
      ],
    };
    const controller = createEntryQueryController(
      () => "space-1",
      {
        scope: { kind: "form", form_id: "form-1" },
        filters: [{
          field: { kind: "property", field_id: 7 },
          operator: "equals",
          value: "",
        }],
        sort: [],
      },
      undefined,
      50,
      queryMock,
    );
    render(() => (
      <EntryBrowser controller={controller} capabilities={capabilities} />
    ));

    fireEvent.click(screen.getByText("Filter"));
    fireEvent.change(
      screen.getByRole("combobox", { name: "Filter field 1" }),
      { target: { value: JSON.stringify({ kind: "property", field_id: 7 }) } },
    );
    fireEvent.change(
      screen.getByRole("combobox", { name: "Operator 1" }),
      { target: { value: "equals" } },
    );
    const value = await screen.findByLabelText("Value");
    fireEvent.input(value, { target: { value: "active" } });
    expect(controller.query().filters).toEqual([{
      field: { kind: "property", field_id: 7 },
      operator: "equals",
      value: "active",
    }]);
  });

  it("keeps incomplete typed filters out of the canonical query until applied", async () => {
    queryMock.mockResolvedValue({ rows: [], has_more: false });
    const capabilities: EntryQueryCapabilities = {
      scope: { kind: "form", form_id: "form-1" },
      fields: [{
        field: { kind: "property", field_id: 8 },
        name: "priority",
        field_type: "integer",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals", "gte"],
      }],
    };
    const controller = createEntryQueryController(
      () => "space-1",
      { scope: capabilities.scope, filters: [], sort: [] },
      undefined,
      50,
      queryMock,
    );
    render(() => (
      <EntryBrowser controller={controller} capabilities={capabilities} />
    ));

    fireEvent.click(screen.getByText("Filter"));
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    const value = await screen.findByLabelText("Value");
    fireEvent.input(value, { target: { value: "12abc" } });

    expect(controller.query().filters).toEqual([]);
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();

    fireEvent.input(value, { target: { value: "12" } });
    expect(screen.getByRole("button", { name: "Apply" })).not.toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await vi.waitFor(() => expect(controller.query().filters).toEqual([{
      field: { kind: "property", field_id: 8 },
      operator: "equals",
      value: 12,
    }]));
  });
});
