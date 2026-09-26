// REQ-FE-004: canonical EntryBrowser display and query controls
// REQ-FE-008: EntryBrowser selection remains separate from mutation
import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
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

    fireEvent.input(screen.getByRole("searchbox"), {
      target: { value: "alice" },
    });
    expect(controller.query().text).toBe("alice");

    fireEvent.click(screen.getByRole("button", { name: "Sort" }));
    fireEvent.click(screen.getByRole("button", { name: "Add sort" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(controller.query().sort).toHaveLength(1);
    expect(controller.query().sort[0].field).toEqual({ kind: "created_at" });

    fireEvent.click(screen.getByRole("button", { name: "Columns" }));
    fireEvent.click(screen.getByRole("radio", { name: "Selected fields" }));
    const createdColumn = screen.getByRole("checkbox", { name: "Created" });
    fireEvent.click(createdColumn);
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
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
    const targetRow = screen.getByText("Target row").closest("tr");
    expect(targetRow).not.toBeNull();
    fireEvent.click(targetRow!);
    expect(targetRow).toHaveAttribute("aria-selected", "true");
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Use this entry" }));
    expect(onSelect).toHaveBeenCalledWith(
      expect.objectContaining({ id: "stable-entry-id" }),
    );
    expect(queryMock).toHaveBeenCalledTimes(1);
  });

  it("navigates only from the shared trailing chevron action", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-to-open",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        preview: "Entry name",
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
    const { container } = render(() => (
      <EntryBrowser
        controller={controller}
        capabilities={systemEntryCapabilities({
          kind: "form",
          form_id: "form-1",
        })}
        onSelect={onSelect}
      />
    ));

    const row = screen.getByRole("row", { name: /Entry name/ });
    const primaryCell = within(row).getByText("Entry name");
    expect(primaryCell.closest("button, a")).toBeNull();
    expect(primaryCell.closest("strong, b")).toBeNull();
    fireEvent.click(row);
    expect(row).toHaveAttribute("aria-selected", "true");
    expect(onSelect).not.toHaveBeenCalled();

    const open = screen.getByRole("button", { name: "Open entry" });
    expect(open.querySelector("span.rowListChevron")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    expect(container.querySelectorAll(".rowListChevron")).toHaveLength(1);
    fireEvent.click(open);
    expect(onSelect).toHaveBeenCalledWith(
      expect.objectContaining({ id: "entry-to-open" }),
    );
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it("updates Form labels when metadata arrives without reloading the page", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-1",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        preview: "Readable entry",
      }],
      has_more: true,
    });
    const controller = createEntryQueryController(
      () => "space-1",
      undefined,
      undefined,
      50,
      queryMock,
    );
    await controller.load();
    const [labels, setLabels] = createSignal<Record<string, string>>({});
    const { container } = render(() => (
      <EntryBrowser
        controller={controller}
        capabilities={systemEntryCapabilities({ kind: "all" })}
        formLabels={labels()}
      />
    ));

    expect(screen.getByText("Unknown form")).toBeInTheDocument();
    const scroll = container.querySelector(".entry-browser-table-scroll");
    if (!scroll) throw new Error("Expected the result scroll container");
    scroll.scrollTop = 80;
    setLabels({ "form-1": "Tasks" });

    expect(await screen.findByText("Tasks")).toBeInTheDocument();
    expect(scroll.scrollTop).toBe(80);
    expect(controller.rows()).toHaveLength(1);
    expect(queryMock).toHaveBeenCalledTimes(1);
  });

  it("shows a neutral placeholder while Form labels are loading or unavailable", async () => {
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
    const [state, setState] = createSignal<"loading" | "error">("loading");
    render(() => (
      <EntryBrowser
        controller={controller}
        capabilities={systemEntryCapabilities({ kind: "all" })}
        formLabels={{ "form-1": "Stale label" }}
        formLabelsState={state()}
      />
    ));

    expect(screen.getByText("—")).toBeInTheDocument();
    setState("error");
    expect(screen.getByText("—")).toBeInTheDocument();
    expect(screen.queryByText("Unknown form")).not.toBeInTheDocument();
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

    expect(headerLabels()).toEqual([
      "status",
      "owner",
      "Updated",
      "Open entry",
    ]);
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

  it("keeps timestamp columns after Form fields in stable Created then Updated order", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-1",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        preview: "Record",
      }],
      has_more: false,
    });
    const capabilities = systemEntryCapabilities({
      kind: "form",
      form_id: "form-1",
    });
    const controller = createEntryQueryController(
      () => "space-1",
      { scope: capabilities.scope, filters: [], sort: [] },
      {
        kind: "fields",
        fields: [
          { kind: "updated_at" },
          { kind: "created_at" },
        ],
      },
      50,
      queryMock,
    );
    await controller.load();
    render(() => (
      <EntryBrowser controller={controller} capabilities={capabilities} />
    ));
    expect(headerLabels()).toEqual([
      "Created",
      "Updated",
      "Open entry",
    ]);
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

    expect(headerLabels()).toEqual([
      "Form",
      "Preview",
      "Created",
      "Updated",
      "Open entry",
    ]);
    expect(screen.getByText("Tasks")).toBeInTheDocument();
    expect(screen.queryByText("form-9")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Columns" }));
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

    fireEvent.click(screen.getByRole("button", { name: /^Filter/ }));
    expect(screen.getByRole("button", { name: "Filter, 1 applied" }))
      .toBeInTheDocument();
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
      value: "",
    }]);
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(controller.query().filters).toEqual([{
      field: { kind: "property", field_id: 7 },
      operator: "equals",
      value: "active",
    }]);
    expect(screen.getByRole("button", { name: "Filter, 1 applied" }))
      .toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: /Remove status Equals active/ }),
    );
    expect(controller.query().filters).toEqual([]);
    expect(screen.getByRole("button", { name: "Filter" }))
      .toBeInTheDocument();
  });

  it("preserves untouched timezone-aware filter precision on Apply", async () => {
    queryMock.mockResolvedValue({ rows: [], has_more: false });
    const timestamp = "2026-09-25T01:02:03.123456789+09:00";
    const capabilities: EntryQueryCapabilities = {
      ...systemEntryCapabilities({ kind: "form", form_id: "form-1" }),
      fields: [
        ...systemEntryCapabilities({ kind: "form", form_id: "form-1" })
          .fields,
        {
          field: { kind: "property", field_id: 9 },
          name: "Occurred at",
          field_type: "timestamp_tz_ns",
          filterable: true,
          sortable: true,
          projectable: true,
          supported_operators: ["equals"],
        },
      ],
    };
    const initialFilter = {
      field: { kind: "property" as const, field_id: 9 },
      operator: "equals" as const,
      value: timestamp,
    };
    const controller = createEntryQueryController(
      () => "space-1",
      {
        scope: { kind: "form", form_id: "form-1" },
        filters: [initialFilter],
        sort: [],
      },
      undefined,
      50,
      queryMock,
    );
    render(() => (
      <EntryBrowser controller={controller} capabilities={capabilities} />
    ));

    fireEvent.click(screen.getByRole("button", { name: /^Filter/ }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    expect(controller.query().filters).toEqual([initialFilter]);
  });

  it("keeps column reordering in a draft until Apply and fixes timestamps at the end", async () => {
    queryMock.mockResolvedValue({ rows: [], has_more: false });
    const capabilities: EntryQueryCapabilities = {
      ...systemEntryCapabilities({ kind: "form", form_id: "form-1" }),
      fields: [
        ...systemEntryCapabilities({ kind: "form", form_id: "form-1" }).fields,
        ...[
          { id: 7, name: "Status" },
          { id: 8, name: "Owner" },
        ].map(({ id, name }) => ({
          field: { kind: "property" as const, field_id: id },
          name,
          field_type: "string",
          filterable: true,
          sortable: true,
          projectable: true,
          supported_operators: ["equals" as const],
        })),
      ],
    };
    const controller = createEntryQueryController(
      () => "space-1",
      { scope: capabilities.scope, filters: [], sort: [] },
      {
        kind: "fields",
        fields: [
          { kind: "property", field_id: 7 },
          { kind: "property", field_id: 8 },
          { kind: "updated_at" },
          { kind: "created_at" },
        ],
      },
      50,
      queryMock,
    );
    render(() => (
      <EntryBrowser controller={controller} capabilities={capabilities} />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Columns" }));
    fireEvent.click(screen.getByRole("button", { name: "Move down Status" }));
    const orderedOptions = Array.from(
      document.querySelectorAll(".entry-browser-column-row label"),
      (label) => label.textContent?.trim(),
    );
    expect(orderedOptions.slice(0, 2)).toEqual(["Owner", "Status"]);
    expect(controller.projection().kind).toBe("fields");
    if (controller.projection().kind === "fields") {
      expect(controller.projection().fields[0]).toEqual({
        kind: "property",
        field_id: 7,
      });
    }
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    expect(controller.projection()).toEqual({
      kind: "fields",
      fields: [
        { kind: "property", field_id: 8 },
        { kind: "property", field_id: 7 },
        { kind: "created_at" },
        { kind: "updated_at" },
      ],
    });
  });

  it("can hide preview timestamp columns without changing the server projection", async () => {
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-preview",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: CREATED_MICROS,
        updated_at_micros: UPDATED_MICROS,
        preview: "Preview row",
      }],
      has_more: false,
    });
    const capabilities = systemEntryCapabilities({
      kind: "form",
      form_id: "form-1",
    });
    const controller = createEntryQueryController(
      () => "space-1",
      { scope: capabilities.scope, filters: [], sort: [] },
      { kind: "preview" },
      50,
      queryMock,
    );
    await controller.load();
    render(() => (
      <EntryBrowser controller={controller} capabilities={capabilities} />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Columns" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Created" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Updated" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    expect(controller.projection()).toEqual({ kind: "preview" });
    expect(headerLabels()).toEqual(["Preview", "Open entry"]);
  });

  it("keeps the remaining timestamp column after ordinary columns", async () => {
    const hiddenCreatedMicros = CREATED_MICROS - 86_400_000_000;
    queryMock.mockResolvedValue({
      rows: [{
        id: "entry-preview-one-time",
        form_id: "form-1",
        revision_id: "revision-1",
        created_at_micros: hiddenCreatedMicros,
        updated_at_micros: UPDATED_MICROS,
        preview: "Preview row",
      }],
      has_more: false,
    });
    const capabilities = systemEntryCapabilities({
      kind: "form",
      form_id: "form-1",
    });
    const controller = createEntryQueryController(
      () => "space-1",
      { scope: capabilities.scope, filters: [], sort: [] },
      { kind: "preview" },
      50,
      queryMock,
    );
    await controller.load();
    render(() => (
      <EntryBrowser controller={controller} capabilities={capabilities} />
    ));

    fireEvent.click(screen.getByRole("button", { name: "Columns" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Created" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    expect(controller.projection()).toEqual({ kind: "preview" });
    expect(headerLabels()).toEqual(["Preview", "Updated", "Open entry"]);
    await screen.findByText(expectedDateLabel(UPDATED_MICROS));
    expect(screen.queryByText(expectedDateLabel(hiddenCreatedMicros)))
      .not.toBeInTheDocument();
  });

  it("validates numeric filter input before applying its typed value", () => {
    queryMock.mockResolvedValue({ rows: [], has_more: false });
    const base = systemEntryCapabilities({ kind: "form", form_id: "form-1" });
    const capabilities: EntryQueryCapabilities = {
      ...base,
      fields: [
        ...base.fields,
        {
          field: { kind: "property", field_id: 4 },
          name: "Amount",
          field_type: "double",
          filterable: true,
          sortable: true,
          projectable: true,
          supported_operators: ["equals", "gt"],
        },
      ],
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

    fireEvent.click(screen.getByRole("button", { name: "Filter" }));
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    fireEvent.change(screen.getByRole("combobox", { name: "Filter field 1" }), {
      target: { value: JSON.stringify({ kind: "property", field_id: 4 }) },
    });
    const value = screen.getByRole("spinbutton");
    fireEvent.input(value, { target: { value: "1e309" } });

    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
    expect(controller.query().filters).toEqual([]);
    fireEvent.input(value, { target: { value: "3.5" } });
    expect(value).toHaveValue(3.5);
    expect(screen.getByRole("button", { name: "Apply" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(controller.query().filters).toEqual([{
      field: { kind: "property", field_id: 4 },
      operator: "equals",
      value: 3.5,
    }]);
  });

  it("discards a filter draft on Escape and returns focus to its toolbar button", () => {
    queryMock.mockResolvedValue({ rows: [], has_more: false });
    const capabilities = systemEntryCapabilities({
      kind: "form",
      form_id: "form-1",
    });
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

    const filterButton = screen.getByRole("button", { name: "Filter" });
    fireEvent.click(filterButton);
    fireEvent.click(screen.getByRole("button", { name: "Add filter" }));
    expect(controller.query().filters).toEqual([]);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(controller.query().filters).toEqual([]);
    expect(document.activeElement).toBe(filterButton);
    expect(filterButton).toHaveAttribute("aria-expanded", "false");
  });
});
