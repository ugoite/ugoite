import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CompositionDataWorkspace } from "./CompositionDataWorkspace";
import {
  addEntryQuerySource,
  addMetricDisplay,
  addSavedSqlSource,
  addTableDisplay,
  type CompositionDraft,
  createEmptyDraft,
  setEntryQueryDisplaySystemFields,
  setEntryQueryFilters,
  setEntryQueryProjection,
  setEntryQuerySort,
  setSavedSqlRevision,
} from "~/lib/composition-draft";
import type {
  CompositionResolveDiagnostic,
  CompositionResolvedSource,
} from "~/lib/composition-api";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import { setLocale } from "~/lib/i18n";
import { clearStudioFormDefinitionCache } from "~/lib/entry-query-studio-capabilities";
import type { Form } from "~/lib/types";

const { sqlGetMock, sqlQueryMock, formApiListMock } = vi.hoisted(() => ({
  sqlGetMock: vi.fn(),
  sqlQueryMock: vi.fn(),
  formApiListMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: {
    list: (...args: unknown[]) =>
      (formApiListMock as (...call: unknown[]) => unknown)(...args),
  },
  sqlApi: {
    get: (...args: unknown[]) =>
      (sqlGetMock as (...call: unknown[]) => unknown)(...args),
    query: (...args: unknown[]) =>
      (sqlQueryMock as (...call: unknown[]) => unknown)(...args),
  },
}));

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>
      {props.children as never}
    </a>
  ),
}));

const stylesheet = () => readFileSync(join(__dirname, "..", "app.css"), "utf8");

const entrySeed = () => ({
  formId: "11111111-1111-4111-8111-111111111111",
  name: "Expenses",
  fieldSchema: [
    { field_id: 100, field_type: "date" },
    { field_id: 101, field_type: "string" },
  ],
  query: {
    filters: [],
    sort: [],
    projection: { kind: "preview" as const },
  },
});

/** Transient Form definition: two capable fields with human names plus one
 * genuinely incapable field the shared dialog must never offer. */
const expenseForm = (): Form => ({
  id: "11111111-1111-4111-8111-111111111111",
  name: "Expenses",
  version: 1,
  template: "",
  fields: {
    occurred: {
      id: 100,
      type: "date",
      required: false,
      query_capability: {
        field: { kind: "property", field_id: 100 },
        name: "Occurred",
        field_type: "date",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals", "lt", "lte", "gt", "gte"],
      },
    },
    title: {
      id: 101,
      type: "string",
      required: false,
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
    attachment: {
      id: 102,
      type: "binary",
      required: false,
      query_capability: {
        field: { kind: "property", field_id: 102 },
        name: "Attachment",
        field_type: "binary",
        filterable: false,
        sortable: false,
        projectable: true,
        supported_operators: [],
      },
    },
  },
});

/** Form whose fields carry no filter or sort capability at all. */
const incapableForm = (): Form => ({
  id: "11111111-1111-4111-8111-111111111111",
  name: "Expenses",
  version: 1,
  template: "",
  fields: {
    attachment: {
      id: 100,
      type: "binary",
      required: false,
      query_capability: {
        field: { kind: "property", field_id: 100 },
        name: "Attachment",
        field_type: "binary",
        filterable: false,
        sortable: false,
        projectable: true,
        supported_operators: [],
      },
    },
  },
});

const sqlSeed = () => ({
  entryId: "sql-1",
  revisionId: "sql-rev-1",
  name: "Monthly totals",
  expectedResult: [{ name: "total", type: "float" as const }],
  variables: { month_start: { parameter: "month_start" } },
});

const twoSourceDraft = (): CompositionDraft => {
  let draft = createEmptyDraft("Tool");
  draft = addSavedSqlSource(draft, sqlSeed()).draft;
  draft = addEntryQuerySource(draft, entrySeed()).draft;
  return draft;
};

const savedSqlEntry = {
  id: "sql-1",
  name: "Monthly totals",
  kind: "user-query",
  sql: "SELECT SUM(amount) AS total FROM expenses",
  variables: [{ type: "date", name: "month_start", description: "" }],
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
  revision_id: "sql-rev-1",
};

interface Harness {
  container: HTMLElement;
  unmount: () => void;
  editor: () => HTMLElement;
  current: () => CompositionDraft;
  onSelect: ReturnType<typeof vi.fn>;
  onAddSource: ReturnType<typeof vi.fn>;
  onMove: ReturnType<typeof vi.fn>;
  onRemove: ReturnType<typeof vi.fn>;
  onNext: ReturnType<typeof vi.fn>;
  onPrevious: ReturnType<typeof vi.fn>;
  onRetry: ReturnType<typeof vi.fn>;
  onRevision: ReturnType<typeof vi.fn>;
}

const renderWorkspace = (
  initial: CompositionDraft,
  overrides: {
    planSources?: readonly CompositionResolvedSource[];
    sourceStates?: Record<string, CompositionSourcePageState>;
    diagnostics?: readonly CompositionResolveDiagnostic[];
    previewActive?: boolean;
    collapsible?: boolean;
  } = {},
): Harness => {
  const [current, setCurrent] = createSignal(initial);
  const [selected, setSelected] = createSignal<string | null>(null);
  const harness: Harness = {
    current,
    onSelect: vi.fn((id: string) =>
      setSelected((previous) => previous === id ? null : id)
    ),
    onAddSource: vi.fn(),
    onMove: vi.fn(),
    onRemove: vi.fn(),
    onNext: vi.fn(),
    onPrevious: vi.fn(),
    onRetry: vi.fn(),
    onRevision: vi.fn(() => true),
  };
  const rendered = render(() => (
    <CompositionDataWorkspace
      spaceId="space-1"
      draft={current()}
      headingId="studio-data-heading"
      selectedSourceId={selected()}
      collapsible={overrides.collapsible}
      onSelectSource={harness.onSelect}
      onAddSource={harness.onAddSource}
      onMoveSource={harness.onMove}
      onRemoveSource={harness.onRemove}
      onEntryQueryFilters={(id, filters) => {
        const result = setEntryQueryFilters(current(), id, filters);
        if (result.ok) setCurrent(result.draft);
        return result.ok;
      }}
      onEntryQuerySort={(id, sort) => {
        const result = setEntryQuerySort(current(), id, sort);
        if (result.ok) setCurrent(result.draft);
        return result.ok;
      }}
      onEntryQueryProjection={(id, projection) => {
        const result = setEntryQueryProjection(current(), id, projection);
        if (result.ok) setCurrent(result.draft);
        return result.ok;
      }}
      onEntryQueryDisplaySystemFields={(id, fields) => {
        const result = setEntryQueryDisplaySystemFields(current(), id, fields);
        if (result.ok) setCurrent(result.draft);
        return result.ok;
      }}
      onSavedSqlRevision={(id, revision, variableTypes) => {
        const result = setSavedSqlRevision(current(), id, revision);
        if (result.ok) setCurrent(result.draft);
        harness.onRevision(id, revision, variableTypes);
        return result.ok;
      }}
      savedSqlEditHref={(entryId) =>
        `/spaces/space-1/sql/${encodeURIComponent(entryId)}`}
      planSources={overrides.planSources ?? []}
      sourceStates={overrides.sourceStates ?? {}}
      diagnostics={overrides.diagnostics ?? []}
      previewActive={overrides.previewActive ?? false}
      onNext={harness.onNext}
      onPrevious={harness.onPrevious}
      onRetry={harness.onRetry}
      registerSourceRow={() => {}}
    />
  ));
  const container = document.body;
  return {
    ...harness,
    container,
    unmount: rendered.unmount,
    editor: () => {
      const root = container.querySelector(".dataWorkspaceEditor");
      if (!root || !(root instanceof HTMLElement)) {
        throw new Error("expected data workspace editor");
      }
      return root;
    },
  };
};

const openColumnDialog = async (editor: HTMLElement) => {
  fireEvent.click(within(editor).getByRole("button", { name: "Columns" }));
  return await within(editor).findByRole("dialog");
};

describe("CompositionDataWorkspace", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    clearStudioFormDefinitionCache();
    // No definition by default: the editor degrades to the schema snapshot.
    formApiListMock.mockResolvedValue([]);
    sqlGetMock.mockResolvedValue(savedSqlEntry);
    sqlQueryMock.mockResolvedValue({
      columns: ["total"],
      rows: [[128400]],
      has_more: false,
      result_schema: [{ name: "total", type: "float" }],
    });
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("selects navigator sources with full-row activation", async () => {
    renderWorkspace(twoSourceDraft());

    const monthly = await screen.findByRole("button", {
      name: "Monthly totals",
    });
    const expenses = await screen.findByRole("button", { name: "Expenses" });
    expect(monthly.textContent).not.toMatch(/›/);
    expect(expenses).toHaveAttribute("aria-pressed", "false");
    // No editor before selection: structure before explanation.
    expect(screen.queryByRole("heading", { name: "Expenses" })).toBeNull();

    fireEvent.click(expenses);
    await screen.findByRole("heading", { name: "Expenses" });
    expect(expenses).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps long navigator names available without exposing source identifiers", async () => {
    const draft = twoSourceDraft();
    const source = draft.sources[0];
    if (!source) throw new Error("expected a source");
    const fullName =
      "Monthly totals for the previous fiscal quarter, including adjustments";
    const namedDraft: CompositionDraft = {
      ...draft,
      sources: draft.sources.map((item) =>
        item.draftId === source.draftId ? { ...item, name: fullName } : item
      ),
    };
    const { container } = renderWorkspace(namedDraft);

    const row = await screen.findByRole("button", { name: fullName });
    expect(row).toHaveAttribute("title", fullName);
    expect(row).toHaveAccessibleName(fullName);
    const buttonLabels = Array.from(container.querySelectorAll("button"))
      .map((button) =>
        [
          button.getAttribute("aria-label"),
          button.getAttribute("title"),
          button.textContent,
        ].join(" ")
      )
      .join(" ");
    for (const identifier of [source.draftId, "sql-1", "sql-rev-1"]) {
      expect(document.body.textContent).not.toContain(identifier);
      expect(buttonLabels).not.toContain(identifier);
    }
  });

  it("preserves selected detail when closing and restoring the mobile navigator", async () => {
    const media = {
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal("matchMedia", vi.fn(() => media));
    const { container } = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });

    const navigator = container.querySelector(".dataWorkspaceNavigator");
    expect(navigator).toBeInTheDocument();
    const show = screen.getByRole("button", { name: "Show data sources" });
    expect(show).toHaveAttribute("aria-expanded", "false");
    expect(show).toHaveAttribute("aria-controls", navigator?.id);
    fireEvent.click(show);
    await Promise.resolve();

    expect(navigator).not.toHaveAttribute("hidden");
    const hide = screen.getByRole("button", { name: "Hide data sources" });
    expect(hide).toHaveAttribute("aria-expanded", "true");
    expect(hide).toHaveAttribute("aria-controls", navigator?.id);
    expect(hide).toHaveFocus();

    fireEvent.click(hide);
    await Promise.resolve();
    expect(navigator).toHaveAttribute("hidden");
    const restoredShow = screen.getByRole("button", {
      name: "Show data sources",
    });
    expect(restoredShow).toHaveFocus();
    expect(screen.getByRole("heading", { name: "Expenses" })).toBeVisible();

    fireEvent.click(restoredShow);
    await Promise.resolve();

    expect(navigator).not.toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "Hide data sources" }))
      .toHaveFocus();
    expect(screen.getByRole("button", { name: "Expenses" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Expenses" })).toBeVisible();
  });

  it("reorders sources with keyboard-operable move buttons", async () => {
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));

    const selectedUp = await screen.findByRole("button", {
      name: "Move Expenses up",
    });
    const selectedDown = await screen.findByRole("button", {
      name: "Move Expenses down",
    });
    // Boundary buttons stay disabled; native buttons keep keyboard focus.
    expect(selectedUp).toBeEnabled();
    expect(selectedDown).toBeDisabled();

    fireEvent.click(selectedUp);
    expect(harness.onMove).toHaveBeenCalledWith("src-2", "up");

    fireEvent.click(await screen.findByRole("button", { name: "Monthly totals" }));
    const remove = await screen.findByRole("button", {
      name: "Remove Monthly totals",
    });
    fireEvent.click(remove);
    expect(harness.onRemove).toHaveBeenCalledWith("src-1");
  });

  it("renders an empty navigator without add-data prose", () => {
    renderWorkspace(createEmptyDraft("Tool"));
    expect(screen.queryByText("Add data to begin.")).toBeNull();
    expect(screen.queryByRole("button", { name: "Add data" })).toBeNull();
    expect(screen.getByRole("button", { name: "Add source" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Expenses" })).toBeNull();
  });

  it("keeps source actions in the navigator footer", async () => {
    const harness = renderWorkspace(twoSourceDraft());
    const navigator = harness.container.querySelector(".dataWorkspaceNavigator");
    if (!navigator) throw new Error("expected source navigator");
    const footer = navigator.querySelector(".dataWorkspaceNavigatorFooter");
    expect(footer).toBeInTheDocument();
    expect(footer?.firstElementChild).toHaveAccessibleName("Add source");
    expect(screen.getByRole("button", { name: "Move Source up" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove selected source" }))
      .toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Add source" }));
    expect(harness.onAddSource).toHaveBeenCalledOnce();
  });

  it("disables source removal while a design block references it", async () => {
    const draft = twoSourceDraft();
    const withTable = addTableDisplay(draft, "src-2");
    if (!withTable.ok) throw new Error("expected a table display");
    const harness = renderWorkspace(withTable.draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));

    const remove = screen.getByRole("button", { name: "Remove Expenses" });
    expect(remove).toBeDisabled();
    expect(remove).toHaveAttribute("title", "Remove its design blocks first.");
    fireEvent.click(remove);
    expect(harness.onRemove).not.toHaveBeenCalled();
  });

  it("uses a closable mobile navigator and keeps its detail selection", async () => {
    const media = {
      matches: true,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal("matchMedia", vi.fn(() => media));
    const { container } = renderWorkspace(twoSourceDraft());
    const navigator = container.querySelector(".dataWorkspaceNavigator");
    expect(navigator).not.toHaveAttribute("hidden");

    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    await waitFor(() => expect(navigator).toHaveAttribute("hidden"));
    const show = screen.getByRole("button", { name: "Show data sources" });
    expect(show).toHaveFocus();
    expect(screen.getByRole("heading", { name: "Expenses" })).toBeVisible();

    fireEvent.click(show);
    await Promise.resolve();
    expect(navigator).not.toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "Expenses" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("heading", { name: "Expenses" })).toBeVisible();
  });

  it("supports older matchMedia change listeners", () => {
    const media = {
      matches: false,
      addListener: vi.fn(),
      removeListener: vi.fn(),
    };
    vi.stubGlobal("matchMedia", vi.fn(() => media));
    const harness = renderWorkspace(twoSourceDraft());

    expect(media.addListener).toHaveBeenCalledOnce();
    harness.unmount();
    expect(media.removeListener).toHaveBeenCalledOnce();
  });

  it("keeps source detail controls available in the Split pane", async () => {
    const harness = renderWorkspace(twoSourceDraft(), { collapsible: true });
    const workspace = harness.container.querySelector(".dataWorkspace");
    const navigator = harness.container.querySelector(
      ".dataWorkspaceNavigator",
    );
    expect(workspace).toHaveClass("dataWorkspace--collapsible");

    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    fireEvent.click(screen.getByRole("button", { name: "Hide data sources" }));

    expect(navigator).toHaveAttribute("hidden");
    expect(screen.getByRole("heading", { name: "Expenses" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Show data sources" }));
    expect(navigator).not.toHaveAttribute("hidden");
    expect(screen.getByRole("button", { name: "Expenses" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("maps entry-query filter edits onto the draft query", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    // Human field names from the transient definition, not raw IDs.
    expect(await within(editor).findByText("Occurred")).toBeInTheDocument();
    expect(within(editor).getByText("Title")).toBeInTheDocument();
    expect(within(editor).queryByText("100", { exact: true })).toBeNull();
    expect(within(editor).queryByText("101", { exact: true })).toBeNull();

    // Add routes through the shared dialog: rows are added, operator and
    // value edit inside, Apply writes back through the narrow updater.
    fireEvent.click(within(editor).getByRole("button", { name: "Add filter" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Add filter" }));
    const field = within(dialog).getByLabelText("Filter field 1");
    fireEvent.change(field, {
      target: { value: JSON.stringify({ kind: "property", field_id: 101 }) },
    });
    const operator = within(dialog).getByLabelText("Operator 1");
    expect(
      within(operator).getAllByRole("option").map((option) =>
        (option as HTMLOptionElement).value
      ),
    ).toEqual(["equals", "contains"]);
    fireEvent.change(operator, { target: { value: "contains" } });
    fireEvent.input(within(dialog).getByLabelText("Value"), {
      target: { value: "lunch" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [{ field_id: 101, operator: "contains", value: "lunch" }],
      },
    });

    // Cancel preserves the draft: the dialog edit is discarded on close.
    fireEvent.click(
      within(editor).getByRole("button", { name: "Edit" }),
    );
    const reopened = await screen.findByRole("dialog");
    fireEvent.input(within(reopened).getByLabelText("Value"), {
      target: { value: "dinner" },
    });
    fireEvent.click(
      within(reopened).getByRole("button", { name: "Cancel" }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [{ field_id: 101, operator: "contains", value: "lunch" }],
      },
    });

    fireEvent.click(within(editor).getByRole("button", { name: "Remove" }));
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: { filters: [] },
    });
  });

  it("keeps parameter bindings through the shared dialog round trip", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    let draft = createEmptyDraft("Tool");
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      query: {
        filters: [{
          field_id: 101,
          operator: "equals",
          value: { parameter: "month" },
        }],
        sort: [],
        projection: { kind: "preview" as const },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    // The binding renders as display text, never flattened in the row.
    expect(await within(editor).findByText("Title Equals {{month}}"))
      .toBeInTheDocument();

    // Applying untouched through the dialog preserves the binding shape.
    fireEvent.click(
      within(editor).getByRole("button", { name: "Edit" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [{
          field_id: 101,
          operator: "equals",
          value: { parameter: "month" },
        }],
      },
    });
  });

  it("maps entry-query sort and projection edits onto the draft query", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();
    await within(editor).findAllByText("Occurred");

    // Sort routes through the shared dialog; Apply writes the draft.
    fireEvent.click(within(editor).getByRole("button", { name: "Add sort" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Add sort" }));
    const direction = within(dialog).getByLabelText("Sort direction 1");
    expect(
      within(direction).getAllByRole("option").map((option) =>
        (option as HTMLOptionElement).value
      ),
    ).toEqual(["asc", "desc"]);
    fireEvent.change(direction, { target: { value: "desc" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: { sort: [{ field_id: 100, direction: "desc" }] },
    });

    // Projection stays inline: the dialog's column contract is not the
    // Composition numeric-IDs-only projection.
    fireEvent.click(
      within(editor).getByRole("radio", { name: "Selected fields" }),
    );
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: { projection: { kind: "fields", fields: [100, 101] } },
    });
  });

  it("keeps the preview projection unavailable while an EntryQuery metric uses a field", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const base = twoSourceDraft();
    const metric = addMetricDisplay(base, "src-2", { fieldId: 100 });
    if (!metric.ok) throw new Error("expected metric");
    const harness = renderWorkspace(metric.draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();
    await within(editor).findByRole("radio", { name: "Selected fields" });

    expect(
      within(editor).getByRole("radio", { name: "Preview" }),
    ).toBeDisabled();
    expect(
      within(editor).getByRole("radio", { name: "Selected fields" }),
    ).toBeChecked();
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: { projection: { kind: "fields", fields: [100] } },
    });
  });

  it("keeps Preview unavailable when the source snapshot omits current Form fields", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const draft = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: [{ field_id: 100, field_type: "date" }],
      query: {
        filters: [],
        sort: [],
        projection: { kind: "fields", fields: [100] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();

    const preview = await within(editor).findByRole("radio", {
      name: "Preview",
    });
    expect(preview).toBeDisabled();
    expect(preview).toHaveAttribute(
      "title",
      "Preview requires a complete Form field snapshot.",
    );
  });

  it("limits filter and sort options to fields in the source snapshot", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const draft = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: [{ field_id: 100, field_type: "date" }],
      query: {
        filters: [],
        sort: [],
        projection: { kind: "fields", fields: [100] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();
    await within(editor).findAllByText("Occurred");

    fireEvent.click(within(editor).getByRole("button", { name: "Add filter" }));
    const filterDialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(filterDialog).getByRole("button", { name: "Add filter" }),
    );
    const filterField = within(filterDialog).getByLabelText("Filter field 1");
    expect(
      within(filterField).getAllByRole("option").map((option) =>
        option.textContent
      ),
    ).toEqual(["Occurred"]);
    fireEvent.keyDown(filterDialog, { key: "Escape" });

    fireEvent.click(within(editor).getByRole("button", { name: "Add sort" }));
    const sortDialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(sortDialog).getByRole("button", { name: "Add sort" }),
    );
    const sortField = within(sortDialog).getByLabelText("Sort field 1");
    expect(
      within(sortField).getAllByRole("option").map((option) =>
        option.textContent
      ),
    ).toEqual(["Occurred"]);
  });

  it("omits nonprojectable Form fields from the projection choices", async () => {
    formApiListMock.mockResolvedValue([{
      ...expenseForm(),
      fields: {
        title: {
          id: 101,
          type: "string",
          required: false,
          query_capability: {
            field: { kind: "property", field_id: 101 },
            name: "Title",
            field_type: "string",
            filterable: true,
            sortable: true,
            projectable: true,
            supported_operators: ["equals"],
          },
        },
        internalNote: {
          id: 102,
          type: "string",
          required: false,
          query_capability: {
            field: { kind: "property", field_id: 102 },
            name: "Internal note",
            field_type: "string",
            filterable: true,
            sortable: true,
            projectable: false,
            supported_operators: ["equals"],
          },
        },
      },
    }]);
    const draft = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: [
        { field_id: 101, field_type: "string" },
        { field_id: 102, field_type: "string" },
      ],
      query: {
        filters: [],
        sort: [],
        projection: { kind: "fields", fields: [101] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();

    const columns = await openColumnDialog(editor);
    expect(
      await within(columns).findByRole("checkbox", { name: "Title" }),
    ).toBeInTheDocument();
    expect(
      within(columns).queryByRole("checkbox", { name: "Internal note" }),
    ).toBeNull();
  });

  it("caps the initial fields projection to the supported limit", async () => {
    const fields = Array.from({ length: 65 }, (_, index) => ({
      field_id: index + 1,
      field_type: "string",
    }));
    const draft = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: fields,
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();

    fireEvent.click(
      await within(editor).findByRole("radio", { name: "Selected fields" }),
    );
    const projection = harness.current().sources[0];
    expect(projection).toMatchObject({
      kind: "entry_query",
      query: {
        projection: {
          kind: "fields",
          fields: Array.from({ length: 64 }, (_, index) => index + 1),
        },
      },
    });
    const columns = await openColumnDialog(editor);
    expect(
      within(columns).getByRole("checkbox", { name: "Field 65" }),
    ).toBeDisabled();
  });

  it("reserves metric fields when initializing a capped projection", async () => {
    const fields = Array.from({ length: 65 }, (_, index) => ({
      field_id: index + 1,
      field_type: "string",
    }));
    const added = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: fields,
    });
    const metric = addMetricDisplay(added.draft, added.draftId, {
      fieldId: 65,
    });
    if (!metric.ok) throw new Error("expected metric");
    const previewDraft = {
      ...metric.draft,
      sources: metric.draft.sources.map((source) =>
        source.draftId === added.draftId && source.kind === "entry_query"
          ? {
            ...source,
            query: {
              ...source.query,
              projection: { kind: "preview" as const },
            },
          }
          : source
      ),
    };
    const harness = renderWorkspace(previewDraft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();

    fireEvent.click(
      await within(editor).findByRole("radio", { name: "Selected fields" }),
    );
    const columns = await openColumnDialog(editor);
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        projection: {
          kind: "fields",
          fields: [65, ...Array.from({ length: 63 }, (_, index) => index + 1)],
        },
      },
    });
    expect(
      within(columns).getByRole("checkbox", { name: "Field 65" }),
    ).toBeChecked();
    expect(
      within(columns).getByRole("checkbox", { name: "Field 65" }),
    ).toBeDisabled();
    expect(
      within(columns).getByRole("checkbox", { name: "Field 64" }),
    ).toBeDisabled();
  });

  it("requires a free slot before adding an unprojected metric field at capacity", async () => {
    const fields = Array.from({ length: 65 }, (_, index) => ({
      field_id: index + 1,
      field_type: "string",
    }));
    const added = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: fields,
    });
    const metric = addMetricDisplay(added.draft, added.draftId, {
      fieldId: 65,
    });
    if (!metric.ok) throw new Error("expected metric");
    const fullProjectionDraft = {
      ...metric.draft,
      sources: metric.draft.sources.map((source) =>
        source.draftId === added.draftId && source.kind === "entry_query"
          ? {
            ...source,
            query: {
              ...source.query,
              projection: {
                kind: "fields" as const,
                fields: fields.slice(0, 64).map((field) => field.field_id),
              },
            },
          }
          : source
      ),
    };
    const harness = renderWorkspace(fullProjectionDraft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();
    const columns = await openColumnDialog(editor);
    const requiredField = await within(columns).findByRole("checkbox", {
      name: "Field 65",
    });

    expect(requiredField).toBeDisabled();
    expect(requiredField).toHaveAttribute(
      "title",
      "The maximum of 64 fields is selected.",
    );
    fireEvent.click(
      within(columns).getByRole("checkbox", { name: "Field 64" }),
    );
    fireEvent.click(within(columns).getByRole("button", { name: "Apply" }));
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        projection: {
          kind: "fields",
          fields: [...Array.from({ length: 63 }, (_, index) => index + 1), 65],
        },
      },
    });
    const updatedColumns = await openColumnDialog(editor);
    const updatedRequiredField = within(updatedColumns).getByRole("checkbox", {
      name: "Field 65",
    });
    expect(updatedRequiredField).toBeChecked();
    expect(updatedRequiredField).toBeDisabled();
  });

  it("disables projection edits when several missing metric fields exceed capacity", async () => {
    const fields = Array.from({ length: 66 }, (_, index) => ({
      field_id: index + 1,
      field_type: "string",
    }));
    const added = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: fields,
    });
    const firstMetric = addMetricDisplay(added.draft, added.draftId, {
      fieldId: 65,
    });
    if (!firstMetric.ok) throw new Error("expected first metric");
    const secondMetric = addMetricDisplay(
      firstMetric.draft,
      added.draftId,
      { fieldId: 66 },
    );
    if (!secondMetric.ok) throw new Error("expected second metric");
    const overbookedDraft = {
      ...secondMetric.draft,
      sources: secondMetric.draft.sources.map((source) =>
        source.draftId === added.draftId && source.kind === "entry_query"
          ? {
            ...source,
            query: {
              ...source.query,
              projection: {
                kind: "fields" as const,
                fields: fields.slice(0, 64).map((field) => field.field_id),
              },
            },
          }
          : source
      ),
    };
    const harness = renderWorkspace(overbookedDraft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();
    const columns = await openColumnDialog(editor);

    for (const field of ["Field 1", "Field 64", "Field 65", "Field 66"]) {
      const checkbox = await within(columns).findByRole("checkbox", {
        name: field,
      });
      expect(checkbox).toBeDisabled();
      expect(checkbox).toHaveAttribute(
        "title",
        "Edit metric bindings to fit within the 64-field limit.",
      );
    }
  });

  it("disables projection controls when metric bindings require more than 64 fields", async () => {
    const fields = Array.from({ length: 65 }, (_, index) => ({
      field_id: index + 1,
      field_type: "string",
    }));
    const added = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: fields,
      query: {
        filters: [],
        sort: [],
        projection: {
          kind: "fields",
          fields: fields.slice(0, 64).map((field) => field.field_id),
        },
      },
    });
    const draft = {
      ...added.draft,
      displays: fields.map((field, index) => ({
        kind: "metric" as const,
        draftId: `disp-${index + 1}`,
        sourceDraftId: added.draftId,
        valueField: { fieldId: field.field_id },
      })),
    };
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();
    const selectedFields = await within(editor).findByRole("radio", {
      name: "Selected fields",
    });
    expect(selectedFields).toBeDisabled();
    expect(selectedFields).toHaveAttribute(
      "title",
      "Edit metric bindings to fit within the 64-field limit.",
    );
    const columns = await openColumnDialog(editor);
    const field64 = within(columns).getByRole("checkbox", {
      name: "Field 64",
    });
    expect(field64).toBeDisabled();
    expect(field64).toHaveAttribute(
      "title",
      "Edit metric bindings to fit within the 64-field limit.",
    );
  });

  it("disables unselected projection fields at the limit and re-enables them after freeing a slot", async () => {
    const fields = Array.from({ length: 65 }, (_, index) => ({
      field_id: index + 1,
      field_type: "string",
    }));
    const added = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: fields,
      query: {
        filters: [],
        sort: [],
        projection: {
          kind: "fields",
          fields: fields.slice(0, 64).map((field) => field.field_id),
        },
      },
    });
    const metric = addMetricDisplay(added.draft, added.draftId, {
      fieldId: 1,
    });
    if (!metric.ok) throw new Error("expected metric");
    const harness = renderWorkspace(metric.draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();
    const columns = await openColumnDialog(editor);

    const checkbox65 = await within(columns).findByRole("checkbox", {
      name: "Field 65",
    });
    expect(checkbox65).toBeDisabled();
    expect(checkbox65).toHaveAttribute(
      "title",
      "The maximum of 64 fields is selected.",
    );
    expect(
      within(columns).getByRole("checkbox", { name: "Field 1" }),
    ).toBeDisabled();
    const checkbox64 = within(columns).getByRole("checkbox", {
      name: "Field 64",
    });
    expect(checkbox64).toBeEnabled();

    fireEvent.click(checkbox64);
    fireEvent.click(within(columns).getByRole("button", { name: "Apply" }));
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        projection: {
          kind: "fields",
          fields: expect.not.arrayContaining([64]),
        },
      },
    });
    const updatedColumns = await openColumnDialog(editor);
    expect(
      within(updatedColumns).getByRole("checkbox", { name: "Field 65" }),
    ).toBeEnabled();
  });

  it("hides incapable fields behind the shared dialog gating", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();
    await within(editor).findByText("Occurred");

    // Filter dialog offers only capable fields by human name.
    fireEvent.click(within(editor).getByRole("button", { name: "Add filter" }));
    const filterDialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(filterDialog).getByRole("button", { name: "Add filter" }),
    );
    expect(
      within(within(filterDialog).getByLabelText("Filter field 1"))
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Occurred", "Title"]);
    fireEvent.click(
      within(filterDialog).getByRole("button", { name: "Cancel" }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();

    // Sort dialog gates the same way.
    fireEvent.click(within(editor).getByRole("button", { name: "Add sort" }));
    const sortDialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(sortDialog).getByRole("button", { name: "Add sort" }),
    );
    expect(
      within(within(sortDialog).getByLabelText("Sort field 1"))
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Occurred", "Title"]);
    fireEvent.click(within(sortDialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: { filters: [], sort: [] },
    });
  });

  it("REQ-FE-070: keeps EntryQuery field IDs in query data and out of editor labels", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    let draft = createEmptyDraft("Tool");
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      query: {
        filters: [{ field_id: 100, operator: "equals", value: "lunch" }],
        sort: [{ field_id: 101, direction: "desc" }],
        projection: { kind: "fields", fields: [100, 101] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    await within(editor).findAllByText("Occurred");
    expect(editor.textContent).toContain("Occurred Equals lunch");
    expect(editor.textContent).toContain("Title · Descending");
    expect(editor.textContent).toContain("Title");
    expect(editor.textContent).not.toMatch(/\b100\b|\b101\b/);

    fireEvent.click(within(editor).getByRole("button", { name: "Add filter" }));
    const filterDialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(filterDialog).getByRole("button", { name: "Add filter" }),
    );
    const filterFieldLabels = within(
      within(filterDialog).getByLabelText("Filter field 1"),
    )
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(filterFieldLabels).toContain("Title");
    expect(filterFieldLabels.join(" ")).not.toMatch(/\b100\b|\b101\b/);
    fireEvent.click(
      within(filterDialog).getByRole("button", { name: "Cancel" }),
    );

    fireEvent.click(within(editor).getByRole("button", { name: "Add sort" }));
    const sortDialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(sortDialog).getByRole("button", { name: "Add sort" }),
    );
    const sortFieldLabels = within(
      within(sortDialog).getByLabelText("Sort field 1"),
    )
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(sortFieldLabels).toContain("Title");
    expect(sortFieldLabels.join(" ")).not.toMatch(/\b100\b|\b101\b/);
    fireEvent.click(within(sortDialog).getByRole("button", { name: "Cancel" }));

    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [{ field_id: 100, operator: "equals", value: "lunch" }],
        sort: [{ field_id: 101, direction: "desc" }],
        projection: { kind: "fields", fields: [100, 101] },
      },
    });
  });

  it("REQ-FE-076: keeps timestamps optional and Form IDs out of column labels", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const draft = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      query: {
        filters: [],
        sort: [],
        projection: { kind: "fields", fields: [100, 101] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();
    await within(editor).findByText("Occurred");
    const columns = await openColumnDialog(editor);

    expect(within(columns).getByRole("checkbox", { name: "Occurred" }))
      .toBeChecked();
    expect(within(columns).getByRole("checkbox", { name: "Title" }))
      .toBeChecked();
    const created = within(columns).getByRole("checkbox", { name: "Created" });
    const updated = within(columns).getByRole("checkbox", { name: "Updated" });
    expect(created).not.toBeChecked();
    expect(updated).not.toBeChecked();
    expect(columns.textContent).not.toMatch(/\b100\b|\b101\b/);

    fireEvent.click(created);
    fireEvent.click(within(columns).getByRole("button", { name: "Apply" }));
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        projection: { kind: "fields", fields: [100, 101] },
        display_system_fields: ["created_at"],
      },
    });
  });

  it("keeps timestamps selectable at the 64-property projection limit", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const fields = Array.from({ length: 64 }, (_, index) => ({
      field_id: index + 1,
      field_type: "string",
    }));
    const draft = addEntryQuerySource(createEmptyDraft("Tool"), {
      ...entrySeed(),
      fieldSchema: fields,
      query: {
        filters: [],
        sort: [],
        projection: {
          kind: "fields",
          fields: fields.map((field) => field.field_id),
        },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    const editor = harness.editor();
    const columns = await openColumnDialog(editor);
    const created = within(columns).getByRole("checkbox", { name: "Created" });
    const updated = within(columns).getByRole("checkbox", { name: "Updated" });

    expect(created).toBeEnabled();
    expect(updated).toBeEnabled();
    fireEvent.click(created);
    expect(updated).toBeEnabled();
    fireEvent.click(within(columns).getByRole("button", { name: "Apply" }));

    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        projection: {
          kind: "fields",
          fields: fields.map((field) => field.field_id),
        },
        display_system_fields: ["created_at"],
      },
    });
  });

  it("lets Preview select timestamps without turning Preview into a Form field", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const draft = addEntryQuerySource(createEmptyDraft("Tool"), entrySeed())
      .draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();
    await within(editor).findByText("Occurred");
    const columns = await openColumnDialog(editor);

    expect(within(columns).queryByRole("checkbox", { name: "Occurred" }))
      .toBeNull();
    const updated = within(columns).getByRole("checkbox", { name: "Updated" });
    expect(updated).not.toBeChecked();
    fireEvent.click(updated);
    fireEvent.click(within(columns).getByRole("button", { name: "Apply" }));

    expect(within(editor).getByRole("radio", { name: "Preview" }))
      .toBeChecked();
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        projection: { kind: "preview" },
        display_system_fields: ["updated_at"],
      },
    });
  });

  it("uses localized field ordinals while form metadata is loading", async () => {
    setLocale("ja");
    formApiListMock.mockReturnValue(new Promise(() => {}));
    let draft = createEmptyDraft("Tool");
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      query: {
        filters: [{ field_id: 100, operator: "equals", value: "lunch" }],
        sort: [{ field_id: 101, direction: "desc" }],
        projection: { kind: "fields", fields: [100, 101] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    expect((await within(editor).findAllByText("項目 1")).length)
      .toBeGreaterThan(0);
    expect(within(editor).getAllByText("項目 2").length).toBeGreaterThan(0);
    expect(editor.textContent).toContain("項目 1 一致する lunch");
    expect(editor.textContent).toContain("項目 2 · 降順");
    expect(within(editor).queryByText("100", { exact: true })).toBeNull();
    expect(within(editor).queryByText("101", { exact: true })).toBeNull();
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [{ field_id: 100, operator: "equals", value: "lunch" }],
        sort: [{ field_id: 101, direction: "desc" }],
        projection: { kind: "fields", fields: [100, 101] },
      },
    });
  });

  it("uses field ordinals when the form is absent", async () => {
    formApiListMock.mockResolvedValue([]);
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    expect(await within(editor).findByText("Field 1")).toBeInTheDocument();
    expect(within(editor).getByText("Field 2")).toBeInTheDocument();
    expect(within(editor).queryByText("100", { exact: true })).toBeNull();
    expect(within(editor).queryByText("101", { exact: true })).toBeNull();
  });

  it("REQ-FE-070: uses distinct localized labels for fields missing from the schema snapshot", async () => {
    setLocale("ja");
    formApiListMock.mockResolvedValue([]);
    let draft = createEmptyDraft("Tool");
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      query: {
        filters: [
          { field_id: 108, operator: "equals", value: "first" },
          { field_id: 109, operator: "equals", value: "second" },
        ],
        sort: [{ field_id: 109, direction: "desc" }],
        projection: { kind: "fields", fields: [100, 108, 109] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    expect(editor.textContent).toContain("項目 3 一致する first");
    expect(editor.textContent).toContain("項目 4 一致する second");
    expect(editor.textContent).toContain("項目 4 · 降順");
    expect(within(editor).queryByText("108", { exact: true })).toBeNull();
    expect(within(editor).queryByText("109", { exact: true })).toBeNull();
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [
          { field_id: 108, operator: "equals", value: "first" },
          { field_id: 109, operator: "equals", value: "second" },
        ],
        sort: [{ field_id: 109, direction: "desc" }],
        projection: { kind: "fields", fields: [100, 108, 109] },
      },
    });
  });

  it("REQ-FE-070: skips localized fallback labels used by live Form fields", async () => {
    setLocale("ja");
    const form = expenseForm();
    form.fields.ordinal_label = {
      id: 108,
      type: "string",
      required: false,
      query_capability: {
        field: { kind: "property", field_id: 108 },
        name: "項目 3",
        field_type: "string",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals"],
      },
    };
    formApiListMock.mockResolvedValue([form]);
    let draft = createEmptyDraft("Tool");
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      fieldSchema: [
        ...entrySeed().fieldSchema,
        { field_id: 103, field_type: "string" },
      ],
      query: {
        filters: [
          { field_id: 103, operator: "equals", value: "schema-missing" },
          { field_id: 108, operator: "equals", value: "known" },
          { field_id: 109, operator: "equals", value: "missing" },
        ],
        sort: [],
        projection: { kind: "fields", fields: [103, 108, 109] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    await waitFor(() => {
      expect(editor.textContent).toContain("項目 3 一致する known");
      expect(editor.textContent).toContain("項目 4 一致する schema-missing");
      expect(editor.textContent).toContain("項目 5 一致する missing");
    });
    expect(within(editor).queryByText("103", { exact: true })).toBeNull();
    expect(within(editor).queryByText("108", { exact: true })).toBeNull();
    expect(within(editor).queryByText("109", { exact: true })).toBeNull();
    expect(harness.current().sources[0]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [
          { field_id: 103, operator: "equals", value: "schema-missing" },
          { field_id: 108, operator: "equals", value: "known" },
          { field_id: 109, operator: "equals", value: "missing" },
        ],
        projection: { kind: "fields", fields: [103, 108, 109] },
      },
    });
  });

  it("uses transient Form names for referenced fields missing from the schema snapshot", async () => {
    const form = expenseForm();
    form.fields.tax_code = {
      id: 108,
      type: "string",
      required: false,
      query_capability: {
        field: { kind: "property", field_id: 108 },
        name: "Tax code",
        field_type: "string",
        filterable: true,
        sortable: true,
        projectable: true,
        supported_operators: ["equals"],
      },
    };
    formApiListMock.mockResolvedValue([form]);
    let draft = createEmptyDraft("Tool");
    draft = addEntryQuerySource(draft, {
      ...entrySeed(),
      query: {
        filters: [
          { field_id: 108, operator: "equals", value: "first" },
          { field_id: 109, operator: "equals", value: "second" },
        ],
        sort: [],
        projection: { kind: "fields", fields: [108, 109] },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    await waitFor(() => expect(editor.textContent).toContain("Tax code"));
    expect(editor.textContent).toContain("Field 3");
    expect(within(editor).queryByText("108", { exact: true })).toBeNull();
    expect(within(editor).queryByText("109", { exact: true })).toBeNull();
  });

  it("falls back to schema fields without a form definition", async () => {
    formApiListMock.mockRejectedValue(new Error("denied"));
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();

    // Explicit error with a retry, while editing never blocks.
    expect(
      await within(editor).findByText("Could not load data sources."),
    ).toBeInTheDocument();
    expect(
      within(editor).getByRole("button", { name: "Add filter" }),
    ).toBeEnabled();
    // Fallback labels stay human-readable while IDs remain in query data.
    expect(within(editor).getByText("Field 1")).toBeInTheDocument();
    expect(within(editor).getByText("Field 2")).toBeInTheDocument();
    expect(within(editor).queryByText("100", { exact: true })).toBeNull();
    expect(within(editor).queryByText("101", { exact: true })).toBeNull();

    // The dialog still edits through the schema snapshot.
    fireEvent.click(within(editor).getByRole("button", { name: "Add filter" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Add filter" }));
    expect(
      within(within(dialog).getByLabelText("Filter field 1"))
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Field 1", "Field 2"]);
    fireEvent.input(within(dialog).getByLabelText("Value"), {
      target: { value: "2026-10-01" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [{ field_id: 100, operator: "equals", value: "2026-10-01" }],
      },
    });

    // Retry recovers names once the definition loads.
    formApiListMock.mockResolvedValue([expenseForm()]);
    fireEvent.click(within(editor).getByRole("button", { name: "Retry" }));
    expect(await within(editor).findByText("Occurred")).toBeInTheDocument();
  });

  it("disables studio add controls with a reason at zero capable fields", async () => {
    formApiListMock.mockResolvedValue([incapableForm()]);
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();
    await within(editor).findByText("Attachment");

    // Genuinely incapable: disabled Adds carry the reason in name and title.
    const addFilter = within(editor).getByRole("button", {
      name: "Add filter: No filterable fields are available for this scope.",
    });
    expect(addFilter).toBeDisabled();
    expect(addFilter).toHaveAttribute(
      "title",
      "Add filter: No filterable fields are available for this scope.",
    );
    const addSort = within(editor).getByRole("button", {
      name: "Add sort: No sortable fields are available for this scope.",
    });
    expect(addSort).toBeDisabled();
    expect(addSort).toHaveAttribute(
      "title",
      "Add sort: No sortable fields are available for this scope.",
    );
    // Zero-clause sections stay prose-free: the reason never renders as text.
    expect(
      within(editor).queryByText("No filterable fields are available", {
        exact: false,
      }),
    ).toBeNull();
    expect(
      within(editor).queryByText("No sortable fields are available", {
        exact: false,
      }),
    ).toBeNull();
  });

  it("renders saved sql read-only with variables and result schema", async () => {
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );
    await screen.findByRole("heading", { name: "Monthly totals" });

    const editor = harness.editor();
    // Read-only SQL: preformatted text, never an editable field.
    const sql = within(editor).getByText("SELECT SUM(amount) AS total", {
      exact: false,
    });
    expect(sql.tagName).toBe("PRE");
    expect(within(editor).queryAllByRole("textbox")).toHaveLength(0);
    // The variable pill shows the bound parameter only when it differs
    // from the variable name; here both read month_start once.
    expect(within(editor).getAllByText("month_start")).toHaveLength(1);
    expect(within(editor).getByText("total")).toBeInTheDocument();

    const edit = within(editor).getByRole("link", { name: "Edit Saved SQL" });
    expect(edit).toHaveAttribute("href", "/spaces/space-1/sql/sql-1");
  });

  it("keeps the saved sql revision in its closed disclosure", async () => {
    const draft = twoSourceDraft();
    const source = draft.sources[0];
    if (!source) throw new Error("expected a source");
    const harness = renderWorkspace(draft);
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );
    await screen.findByRole("heading", { name: "Monthly totals" });

    const editor = harness.editor();
    const disclosure = editor.querySelector("details");
    expect(disclosure).toBeInTheDocument();
    expect(disclosure).not.toHaveAttribute("open");
    const summary = disclosure?.querySelector("summary");
    expect(summary?.textContent).toBe("Monthly totals");

    const primaryContent = editor.cloneNode(true) as HTMLElement;
    primaryContent.querySelectorAll("details").forEach((details) =>
      details.remove()
    );
    const primaryLabels = [
      primaryContent.textContent ?? "",
      ...Array.from(primaryContent.querySelectorAll("button, a, summary"))
        .map((element) =>
          [
            element.textContent,
            element.getAttribute("aria-label"),
            element.getAttribute("title"),
          ].join(" ")
        ),
    ].join(" ");
    for (const identifier of [source.draftId, "sql-1", "sql-rev-1"]) {
      expect(primaryLabels).not.toContain(identifier);
    }

    const revision = within(disclosure as HTMLDetailsElement).getByText(
      "Revision sql-rev-1",
      { exact: true },
    );
    fireEvent.click(summary as HTMLElement);
    expect(disclosure).toHaveAttribute("open");
    expect(revision).toBeInTheDocument();
  });

  it("updates the composition source to the exact new revision", async () => {
    const harness = renderWorkspace(twoSourceDraft());
    sqlGetMock.mockResolvedValue({
      ...savedSqlEntry,
      revision_id: "sql-rev-2",
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );
    await screen.findByRole("heading", { name: "Monthly totals" });

    const editor = harness.editor();
    const update = await within(editor).findByRole("button", {
      name: "Use the edited revision",
    });
    fireEvent.click(update);

    await waitFor(() => {
      expect(sqlQueryMock).toHaveBeenCalledWith(
        "space-1",
        expect.objectContaining({
          limit: 1,
          saved_sql: { id: "sql-1", revision_id: "sql-rev-2" },
        }),
      );
    });
    await waitFor(() => {
      expect(harness.onRevision).toHaveBeenCalledWith(
        "src-1",
        expect.objectContaining({ revisionId: "sql-rev-2" }),
        { month_start: "date" },
      );
    });
    expect(harness.current().sources[0]).toMatchObject({
      kind: "saved_sql",
      revisionId: "sql-rev-2",
    });
  });

  it("conceals denied saved sql sources without identifiers", async () => {
    sqlGetMock.mockRejectedValue({ status: 403 });
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );
    await screen.findByRole("heading", { name: "Monthly totals" });

    const editor = harness.editor();
    expect(
      await within(editor).findByText("A data source is unavailable."),
    ).toBeInTheDocument();
    // No existence oracle: the entry identity never reaches the UI.
    expect(within(editor).queryByText("sql-1")).toBeNull();
    expect(within(editor).queryByText("sql-rev-1")).toBeNull();
  });

  it("renders explicit loading, empty, and error result states", async () => {
    const loading = renderWorkspace(twoSourceDraft(), { previewActive: true });
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    // No page yet with the shared preview in flight: the spinner shows.
    expect(
      await within(loading.editor()).findByText("Loading results…"),
    ).toBeInTheDocument();

    cleanup();
    const failed = renderWorkspace(twoSourceDraft(), {
      planSources: [{
        kind: "entry_query",
        source_id: "src-2",
        request: {
          query: {
            scope: { kind: "form", form_id: "form-1" },
            filters: [],
            sort: [],
          },
          projection: { kind: "preview" },
          limit: 100,
        },
        source_schema_fingerprint: "fp",
      }],
      sourceStates: {
        "src-2": {
          status: "error",
          cursorStack: [undefined],
          error: new Error("denied"),
        },
      },
      diagnostics: [],
    });
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    expect(
      await within(failed.editor()).findByText("Could not load results."),
    ).toBeInTheDocument();
    const retry = within(failed.editor()).getByRole("button", {
      name: "Retry",
    });
    fireEvent.click(retry);
    expect(failed.onRetry).toHaveBeenCalledWith("src-2");

    cleanup();
    const empty = renderWorkspace(twoSourceDraft(), {
      planSources: [{
        kind: "entry_query",
        source_id: "src-2",
        request: {
          query: {
            scope: { kind: "form", form_id: "form-1" },
            filters: [],
            sort: [],
          },
          projection: { kind: "preview" },
          limit: 100,
        },
        source_schema_fingerprint: "fp",
      }],
      sourceStates: {
        "src-2": {
          status: "ready",
          cursorStack: [undefined],
          page: { kind: "entry_query", page: { rows: [], has_more: false } },
        },
      },
      diagnostics: [],
    });
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    expect(
      await within(empty.editor()).findByText("No results"),
    ).toBeInTheDocument();
  });

  it("renders nothing settled without a source page and no filter or sort prose", async () => {
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();
    // Settled preview without a page for this source: no spinner, no error,
    // no empty text. The diagnostics strip already covers failures.
    expect(within(editor).queryByText("Loading results…")).toBeNull();
    expect(within(editor).queryByText("Could not load results.")).toBeNull();
    expect(within(editor).queryByText("No results")).toBeNull();
    // Zero clauses render nothing: the Add buttons stay the guidance.
    expect(
      within(editor).queryByText("No filterable fields are available", {
        exact: false,
      }),
    ).toBeNull();
    expect(
      within(editor).queryByText("No sortable fields are available", {
        exact: false,
      }),
    ).toBeNull();
    expect(
      within(editor).getByRole("button", { name: "Add filter" }),
    ).toBeEnabled();
    expect(
      within(editor).getByRole("button", { name: "Add sort" }),
    ).toBeEnabled();
  });

  it("keeps the add controls disabled without schema fields", async () => {
    let draft = createEmptyDraft("Tool");
    draft = addEntryQuerySource(draft, {
      formId: "11111111-1111-4111-8111-111111111111",
      name: "Fieldless",
      fieldSchema: [],
      query: {
        filters: [],
        sort: [],
        projection: { kind: "preview" as const },
      },
    }).draft;
    const harness = renderWorkspace(draft);
    fireEvent.click(await screen.findByRole("button", { name: "Fieldless" }));
    await screen.findByRole("heading", { name: "Fieldless" });
    const editor = harness.editor();
    // Genuinely empty fields: the Add buttons disable with the reason in
    // name and title, still with no prose.
    expect(
      within(editor).getByRole("button", {
        name: "Add filter: No filterable fields are available for this scope.",
      }),
    ).toBeDisabled();
    expect(
      within(editor).getByRole("button", {
        name: "Add sort: No sortable fields are available for this scope.",
      }),
    ).toBeDisabled();
    expect(
      within(editor).queryByText("No filterable fields are available", {
        exact: false,
      }),
    ).toBeNull();
    expect(
      within(editor).queryByText("No sortable fields are available", {
        exact: false,
      }),
    ).toBeNull();
  });

  it("gates saved sql result spinners on the active preview", async () => {
    const idle = renderWorkspace(twoSourceDraft());
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );
    await screen.findByRole("heading", { name: "Monthly totals" });
    // The Saved SQL entry loads, but the settled preview carries no page
    // for this source: no spinner, no error.
    const idleEditor = idle.editor();
    await within(idleEditor).findByText("SELECT SUM(amount) AS total", {
      exact: false,
    });
    expect(within(idleEditor).queryByText("Loading results…")).toBeNull();
    expect(within(idleEditor).queryByText("Could not load results."))
      .toBeNull();

    cleanup();
    const active = renderWorkspace(twoSourceDraft(), { previewActive: true });
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );
    await screen.findByRole("heading", { name: "Monthly totals" });
    expect(
      await within(active.editor()).findByText("Loading results…"),
    ).toBeInTheDocument();

    cleanup();
    const ready = renderWorkspace(twoSourceDraft(), {
      planSources: [{
        kind: "saved_sql",
        source_id: "src-1",
        request: {
          sql: "SELECT SUM(amount) AS total FROM expenses",
          limit: 100,
          saved_sql: { id: "sql-1", revision_id: "sql-rev-1" },
        },
        source_schema_fingerprint: "fp",
      }],
      sourceStates: {
        "src-1": {
          status: "ready",
          cursorStack: [undefined],
          page: {
            kind: "saved_sql",
            page: { columns: ["total"], rows: [[128400]], has_more: false },
          },
        },
      },
      diagnostics: [],
    });
    fireEvent.click(
      await screen.findByRole("button", { name: "Monthly totals" }),
    );
    await screen.findByRole("heading", { name: "Monthly totals" });
    // Ready rows render settled exactly as before, spinner-free.
    const readyEditor = ready.editor();
    await within(readyEditor).findByText("128400");
    expect(within(readyEditor).queryByText("Loading results…")).toBeNull();
  });

  it("uses a compact overlay navigator at narrow widths", () => {
    const css = stylesheet();
    expect(css).toMatch(/\.dataWorkspace\s*\{[^}]*display:\s*grid/);
    expect(css).toMatch(
      /\.dataWorkspace--collapsible\s+\.dataWorkspaceNavigatorActions[\s\S]*?\.dataWorkspace--collapsible\s+\.dataWorkspaceMainActions\s*\{[^}]*display:\s*flex/,
    );
    expect(css).toMatch(
      /\.dataWorkspace--navigator-collapsed\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)/,
    );
    // 390px sits inside the stacked rule: one column, no document scroll.
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.dataWorkspace--mobile-navigator-open\s+\.dataWorkspaceNavigator[^}]*position:\s*absolute/,
    );
  });

  it("REQ-FE-070: keeps internal Form IDs out of Composition source details", async () => {
    formApiListMock.mockResolvedValue([expenseForm()]);
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    const editor = harness.editor();
    await within(editor).findByText("Occurred");

    expect(editor.querySelector("details")).toBeNull();
    expect(editor.textContent).not.toContain(entrySeed().formId);
    expect(editor.innerHTML).not.toContain(entrySeed().formId);
  });
});
