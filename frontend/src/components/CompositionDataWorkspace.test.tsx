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
  addSavedSqlSource,
  type CompositionDraft,
  createEmptyDraft,
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

const { sqlGetMock, sqlQueryMock } = vi.hoisted(() => ({
  sqlGetMock: vi.fn(),
  sqlQueryMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
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
  editor: () => HTMLElement;
  current: () => CompositionDraft;
  onSelect: ReturnType<typeof vi.fn>;
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
  } = {},
): Harness => {
  const [current, setCurrent] = createSignal(initial);
  const [selected, setSelected] = createSignal<string | null>(null);
  const harness: Harness = {
    current,
    onSelect: vi.fn((id: string) =>
      setSelected((previous) => previous === id ? null : id)
    ),
    onMove: vi.fn(),
    onRemove: vi.fn(),
    onNext: vi.fn(),
    onPrevious: vi.fn(),
    onRetry: vi.fn(),
    onRevision: vi.fn(() => true),
  };
  render(() => (
    <CompositionDataWorkspace
      spaceId="space-1"
      draft={current()}
      headingId="studio-data-heading"
      selectedSourceId={selected()}
      onSelectSource={harness.onSelect}
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
      previewing={false}
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
    editor: () => {
      const root = container.querySelector(".dataWorkspaceEditor");
      if (!root || !(root instanceof HTMLElement)) {
        throw new Error("expected data workspace editor");
      }
      return root;
    },
  };
};

describe("CompositionDataWorkspace", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    sqlGetMock.mockResolvedValue(savedSqlEntry);
    sqlQueryMock.mockResolvedValue({
      columns: ["total"],
      rows: [[128400]],
      has_more: false,
      result_schema: [{ name: "total", type: "float" }],
    });
  });

  afterEach(() => cleanup());

  it("selects navigator sources with full-row activation", async () => {
    renderWorkspace(twoSourceDraft());

    const monthly = await screen.findByRole("button", {
      name: "Monthly totals",
    });
    const expenses = await screen.findByRole("button", { name: "Expenses" });
    expect(monthly.textContent).not.toMatch(/›/);
    // No editor before selection: structure before explanation.
    expect(screen.queryByRole("heading", { name: "Expenses" })).toBeNull();

    fireEvent.click(expenses);
    await screen.findByRole("heading", { name: "Expenses" });
  });

  it("reorders sources with keyboard-operable move buttons", async () => {
    const harness = renderWorkspace(twoSourceDraft());

    const firstUp = await screen.findByRole("button", {
      name: "Move Monthly totals up",
    });
    const secondDown = await screen.findByRole("button", {
      name: "Move Expenses down",
    });
    // Boundary buttons stay disabled; native buttons keep keyboard focus.
    expect(firstUp).toBeDisabled();
    expect(secondDown).toBeDisabled();

    const secondUp = await screen.findByRole("button", {
      name: "Move Expenses up",
    });
    fireEvent.click(secondUp);
    expect(harness.onMove).toHaveBeenCalledWith("src-2", "up");

    const remove = await screen.findByRole("button", {
      name: "Remove Monthly totals",
    });
    fireEvent.click(remove);
    expect(harness.onRemove).toHaveBeenCalledWith("src-1");
  });

  it("renders an empty navigator without sources", async () => {
    renderWorkspace(createEmptyDraft("Tool"));
    expect(await screen.findByText("Add data to begin.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Expenses" })).toBeNull();
  });

  it("maps entry-query filter edits onto the draft query", async () => {
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });

    fireEvent.click(await screen.findByRole("button", { name: "Add filter" }));
    const editor = harness.editor();
    const operator = within(editor).getByLabelText("Operator");
    expect(
      within(operator).getAllByRole("option").map((option) =>
        (option as HTMLOptionElement).value
      ),
    ).toEqual(["equals", "contains", "lt", "lte", "gt", "gte"]);

    fireEvent.change(operator, { target: { value: "gte" } });
    const value = within(editor).getByLabelText("Value");
    fireEvent.change(value, { target: { value: "2026-10-01" } });
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: {
        filters: [{ field_id: 100, operator: "gte", value: "2026-10-01" }],
      },
    });

    fireEvent.click(within(editor).getByRole("button", { name: "Remove" }));
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: { filters: [] },
    });
  });

  it("maps entry-query sort and projection edits onto the draft query", async () => {
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });

    fireEvent.click(await screen.findByRole("button", { name: "Add sort" }));
    const editor = harness.editor();
    const direction = within(editor).getByLabelText("Sort direction");
    expect(
      within(direction).getAllByRole("option").map((option) =>
        (option as HTMLOptionElement).value
      ),
    ).toEqual(["asc", "desc"]);
    fireEvent.change(direction, { target: { value: "desc" } });
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: { sort: [{ field_id: 100, direction: "desc" }] },
    });

    fireEvent.click(
      within(editor).getByRole("radio", { name: "Selected fields" }),
    );
    expect(harness.current().sources[1]).toMatchObject({
      kind: "entry_query",
      query: { projection: { kind: "fields", fields: [100, 101] } },
    });
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
    expect(within(editor).getAllByText("month_start")).toHaveLength(2);
    expect(within(editor).getByText("total")).toBeInTheDocument();

    const edit = within(editor).getByRole("link", { name: "Edit Saved SQL" });
    expect(edit).toHaveAttribute("href", "/spaces/space-1/sql/sql-1");
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
      name: "Use the latest revision",
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
    const harness = renderWorkspace(twoSourceDraft());
    fireEvent.click(await screen.findByRole("button", { name: "Expenses" }));
    await screen.findByRole("heading", { name: "Expenses" });
    // No page yet: the existing preview path is still loading.
    expect(
      await within(harness.editor()).findByText("Loading results…"),
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

  it("stacks the navigator and editor at narrow widths", () => {
    const css = stylesheet();
    expect(css).toMatch(/\.dataWorkspace\s*\{[^}]*display:\s*grid/);
    // 390px sits inside the stacked rule: one column, no document scroll.
    expect(css).toMatch(
      /@media\s*\(max-width:\s*560px\)[\s\S]*?\.dataWorkspace\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0,\s*1fr\)/,
    );
  });
});
