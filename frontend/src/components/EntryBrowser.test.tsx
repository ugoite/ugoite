// REQ-FE-004: canonical EntryBrowser display and query controls
// REQ-FE-008: EntryBrowser selection remains separate from mutation
import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EntryBrowser } from "./EntryBrowser";
import {
  createEntryQueryController,
  type EntryQueryCapabilities,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import { formatDateLabel } from "~/lib/date-format";
const queryMock = vi.fn();

const CREATED_MICROS = 1_772_960_000_000_000;
const UPDATED_MICROS = 1_772_963_000_000_000;

const expectedDateLabel = (micros: number): string =>
  formatDateLabel(new Date(micros / 1_000).toISOString());

const headerLabels = (): string[] =>
  screen.getAllByRole("columnheader").map((header) => header.textContent ?? "");

describe("EntryBrowser", () => {
  beforeEach(() => queryMock.mockReset());

  it("connects toolbar actions to canonical projection, text, and sort state", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-1",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
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
    // Preview projection renders the table with preview + system columns.
    expect(screen.getByRole("table")).toBeInTheDocument();
    expect(headerLabels()).toEqual([
      "Preview",
      "Created",
      "Updated",
      "Use this entry",
    ]);
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
    // Columns selection and visible table columns stay consistent: the
    // table renders exactly the projected fields in projection order,
    // plus the select_one confirm column.
    await screen.findByText(expectedDateLabel(UPDATED_MICROS));
    expect(headerLabels()).toEqual(["Updated", "Use this entry"]);
    expect(
      await screen.findByRole("button", { name: "Use this entry" }),
    ).toBeInTheDocument();
  });

  it("keeps selection separate from server mutation", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "stable-entry-id",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
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
    fireEvent.click(screen.getByRole("button", { name: "Target row" }));

    expect(onSelect).toHaveBeenCalledWith(
      expect.objectContaining({ id: "stable-entry-id" }),
    );
    expect(queryMock).toHaveBeenCalledTimes(1);
  });

  it("renders form-scoped projected fields as explicit columns in projection order", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-9",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        properties: { status: "active", owner: "ada" },
      }],
      has_more: false,
    });
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
        {
          field: { kind: "property", field_id: 8 },
          name: "owner",
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
        filters: [],
        sort: [],
      },
      {
        kind: "fields",
        fields: [
          { kind: "property", field_id: 7 },
          { kind: "property", field_id: 8 },
          { kind: "updated_at" },
        ],
      },
      50,
      queryMock,
    );
    await controller.load();
    render(() => (
      <EntryBrowser controller={controller} capabilities={capabilities} />
    ));

    expect(headerLabels()).toEqual(["status", "owner", "Updated"]);
    const row = screen.getByRole("row", { name: /active/ });
    expect(within(row).getByText("active")).toBeInTheDocument();
    expect(within(row).getByText("ada")).toBeInTheDocument();
    expect(within(row).getByText(expectedDateLabel(UPDATED_MICROS)))
      .toBeInTheDocument();
    // The stable storage id stays out of the visible columns.
    expect(screen.queryByText("entry-9")).not.toBeInTheDocument();
    expect(
      document.querySelector('[data-entry-id="entry-9"]'),
    ).not.toBeNull();
  });

  it("shows system-level columns only for All Forms and never unions properties", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-2",
        form_id: "form-9",
        revision_id: "revision-2",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        preview: "Cross-form preview",
      }],
      has_more: false,
    });
    const capabilities: EntryQueryCapabilities = {
      ...systemEntryCapabilities({ kind: "all" }),
      fields: [
        ...systemEntryCapabilities({ kind: "all" }).fields,
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
      { scope: { kind: "all" }, filters: [], sort: [] },
      { kind: "preview" },
      50,
      queryMock,
    );
    await controller.load();
    render(() => (
      <EntryBrowser
        controller={controller}
        capabilities={capabilities}
        formLabels={{ "form-9": "Tasks" }}
      />
    ));

    expect(headerLabels()).toEqual(["Form", "Preview", "Created", "Updated"]);
    expect(screen.getByText("Tasks")).toBeInTheDocument();
    expect(screen.queryByText("form-9")).not.toBeInTheDocument();

    fireEvent.click(screen.getAllByText("Columns")[0]);
    expect(screen.getByRole("checkbox", { name: "Form" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Created" }))
      .toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Updated" }))
      .toBeInTheDocument();
    // A smuggled heterogeneous property is never offered as a column.
    expect(screen.queryByRole("checkbox", { name: "status" }))
      .not.toBeInTheDocument();
  });

  it("keeps the table structure inside a horizontal scroll container", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-3",
        form_id: "form-1",
        revision_id: "revision-3",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        preview: "Scrollable row",
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
    const { container } = render(() => (
      <EntryBrowser
        controller={controller}
        capabilities={systemEntryCapabilities({
          kind: "form",
          form_id: "form-1",
        })}
      />
    ));

    const scroll = container.querySelector(".entry-browser-table-scroll");
    expect(scroll).not.toBeNull();
    expect(scroll?.querySelector("table.entry-browser-table")).not.toBeNull();
    // No list/card presentation remains: rows are table rows only.
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
    expect(screen.queryByRole("listitem")).not.toBeInTheDocument();
    expect(screen.getAllByRole("row")).toHaveLength(2);
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
});
