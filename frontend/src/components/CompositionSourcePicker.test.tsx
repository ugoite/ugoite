import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
import type { DraftSource } from "~/lib/composition-draft";
import { CompositionSourcePicker } from "./CompositionSourcePicker";

const { formListMock, sqlListMock, sqlGetMock, sqlQueryMock } = vi.hoisted(
  () => ({
    formListMock: vi.fn(),
    sqlListMock: vi.fn(),
    sqlGetMock: vi.fn(),
    sqlQueryMock: vi.fn(),
  }),
);

vi.mock("~/lib/composition-api", () => ({
  canCreateSavedSqlComposition: (entry: { kind: string }) =>
    entry.kind === "user-query",
}));

vi.mock("~/lib/ugoite-client", () => ({
  formApi: {
    list: (...args: unknown[]) =>
      (formListMock as (...call: unknown[]) => unknown)(...args),
  },
  sqlApi: {
    list: (...args: unknown[]) =>
      (sqlListMock as (...call: unknown[]) => unknown)(...args),
    get: (...args: unknown[]) =>
      (sqlGetMock as (...call: unknown[]) => unknown)(...args),
    query: (...args: unknown[]) =>
      (sqlQueryMock as (...call: unknown[]) => unknown)(...args),
  },
}));

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const stylesheet = () =>
  readFileSync(path.join(__dirname, "..", "app.css"), "utf8");

const FORM_ID = "11111111-1111-4111-8111-111111111111";

const taskForm = {
  id: FORM_ID,
  name: "Tasks",
  version: 1,
  template: "task",
  fields: {},
};

const monthlyEntry = {
  id: "sql-1",
  name: "Monthly",
  kind: "user-query",
  sql: "SELECT total FROM monthly",
  variables: [],
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
  revision_id: "sql-rev-1",
};

describe("CompositionSourcePicker", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    formListMock.mockResolvedValue([taskForm]);
    sqlListMock.mockResolvedValue([monthlyEntry]);
    sqlGetMock.mockResolvedValue(monthlyEntry);
    sqlQueryMock.mockResolvedValue({
      columns: ["total"],
      rows: [[1]],
      has_more: false,
      result_schema: [{ name: "total", type: "float" }],
    });
  });

  afterEach(() => cleanup());

  it("lists forms and saved sql with full-row selection", async () => {
    const onSelect = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));

    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    // Forms tab first: full-row selection with a human name only.
    const tasksRow = await screen.findByRole("button", { name: "Tasks" });
    expect(tasksRow.textContent).not.toMatch(/›/);
    expect(screen.queryByRole("button", { name: "Monthly" })).toBeNull();

    // Saved SQL tab: the saved query row, same full-row selection.
    fireEvent.click(screen.getByRole("tab", { name: "Saved SQL" }));
    const monthlyRow = await screen.findByRole("button", { name: "Monthly" });
    expect(monthlyRow.textContent).not.toMatch(/›/);
    expect(screen.queryByRole("button", { name: "Tasks" })).toBeNull();

    // Back on the Forms tab, picking a form seeds the entry query.
    fireEvent.click(screen.getByRole("tab", { name: "Forms" }));
    fireEvent.click(await screen.findByRole("button", { name: "Tasks" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect).toHaveBeenCalledWith({
      kind: "entry_query",
      seed: {
        formId: FORM_ID,
        name: "Tasks",
        fieldSchema: [],
        query: { filters: [], sort: [], projection: { kind: "preview" } },
      },
    });
  });

  it("starts new Form sources with projectable fields selected", async () => {
    const onSelect = vi.fn();
    formListMock.mockResolvedValue([{
      ...taskForm,
      name: "Projectable tasks",
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
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));

    fireEvent.click(
      await screen.findByRole("button", { name: "Projectable tasks" }),
    );
    expect(onSelect).toHaveBeenCalledWith({
      kind: "entry_query",
      seed: {
        formId: FORM_ID,
        name: "Projectable tasks",
        fieldSchema: [
          { field_id: 101, field_type: "string" },
          { field_id: 102, field_type: "string" },
        ],
        query: {
          filters: [],
          sort: [],
          projection: { kind: "fields", fields: [101] },
        },
      },
    });
  });

  it("snapshots additional Form fields while limiting the initial projection", async () => {
    const form = {
      ...taskForm,
      name: "Many fields",
      fields: Object.fromEntries(
        Array.from({ length: 65 }, (_, index) => {
          const fieldId = index + 1;
          return [`field-${fieldId}`, {
            id: fieldId,
            type: "string",
            required: false,
            query_capability: {
              field: { kind: "property" as const, field_id: fieldId },
              name: `Field ${fieldId}`,
              field_type: "string",
              filterable: true,
              sortable: true,
              projectable: true,
              supported_operators: ["equals" as const],
            },
          }];
        }),
      ),
    };
    formListMock.mockResolvedValue([form]);
    const onSelect = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));

    fireEvent.click(await screen.findByRole("button", { name: "Many fields" }));

    const selected = onSelect.mock.calls[0]?.[0];
    expect(selected).toMatchObject({
      kind: "entry_query",
      seed: {
        query: {
          projection: {
            kind: "fields",
            fields: Array.from({ length: 64 }, (_, index) => index + 1),
          },
        },
      },
    });
    expect(selected.seed.fieldSchema).toHaveLength(65);
    expect(selected.seed.fieldSchema.at(-1)).toEqual({
      field_id: 65,
      field_type: "string",
    });
  });

  it("puts current Composition sources first and activates them by hidden draft identity", async () => {
    const onSelectExisting = vi.fn();
    const existingSources: DraftSource[] = [{
      kind: "entry_query",
      draftId: "source-draft-private",
      formId: FORM_ID,
      name: "Current tasks",
      fieldSchema: [],
      query: {
        filters: [],
        sort: [],
        projection: { kind: "preview" },
      },
    }];
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        existingSources={existingSources}
        onSelectExisting={onSelectExisting}
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    const currentSourcesHeading = await screen.findByRole("heading", {
      name: "In this composition",
    });
    const currentSourceGroup = currentSourcesHeading.parentElement;
    if (!currentSourceGroup) throw new Error("expected current source group");
    const currentSource = within(currentSourceGroup).getByRole("button", {
      name: "Current tasks, Forms",
    });
    const tabs = screen.getByRole("tablist");
    expect(
      currentSourcesHeading.compareDocumentPosition(tabs) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(currentSource).toHaveAttribute("title", "Current tasks");
    expect(currentSource).toHaveAccessibleName("Current tasks, Forms");
    expect(currentSource.outerHTML).not.toContain("source-draft-private");
    fireEvent.click(currentSource);
    expect(onSelectExisting).toHaveBeenCalledWith("source-draft-private");
  });

  it("keeps long source names available on picker rows without identifiers", async () => {
    const longFormName = "Project forms with a long name".repeat(4);
    const longSqlName = "Quarterly totals with a long name".repeat(4);
    formListMock.mockResolvedValue([{ ...taskForm, name: longFormName }]);
    sqlListMock.mockResolvedValue([{ ...monthlyEntry, name: longSqlName }]);

    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    const formRow = await screen.findByRole("button", {
      name: longFormName,
    });
    expect(formRow).toHaveAttribute("title", longFormName);
    expect(formRow.querySelector(".formRowName")).toHaveTextContent(
      longFormName,
    );
    expect(formRow.outerHTML).not.toContain(FORM_ID);

    fireEvent.click(screen.getByRole("tab", { name: "Saved SQL" }));
    const sqlRow = await screen.findByRole("button", { name: longSqlName });
    expect(sqlRow).toHaveAttribute("title", longSqlName);
    expect(sqlRow.querySelector(".rowListName")).toHaveTextContent(
      longSqlName,
    );
    expect(sqlRow.outerHTML).not.toContain("sql-1");
    expect(sqlRow.outerHTML).not.toContain("sql-rev-1");

    const css = stylesheet();
    expect(css).toMatch(
      /\.formRowName\s*\{[^}]*min-width:\s*0;[^}]*flex:\s*1 1 auto;[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/,
    );
    expect(css).toMatch(
      /\.rowListName\s*\{[^}]*flex:\s*1 1 auto;[^}]*min-width:\s*0;[^}]*overflow:\s*hidden;[^}]*text-overflow:\s*ellipsis;[^}]*white-space:\s*nowrap;/,
    );
  });

  it("pins the exact saved sql revision with server-owned column types", async () => {
    const onSelect = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));

    fireEvent.click(await screen.findByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(await screen.findByRole("button", { name: "Monthly" }));
    await waitFor(() => {
      expect(onSelect).toHaveBeenCalledTimes(1);
    });
    expect(sqlGetMock).toHaveBeenCalledWith("space-1", "sql-1");
    expect(sqlQueryMock).toHaveBeenCalledWith(
      "space-1",
      {
        sql: "SELECT total FROM monthly",
        parameters: {},
        parameter_types: {},
        limit: 1,
        saved_sql: { id: "sql-1", revision_id: "sql-rev-1" },
      },
    );
    expect(onSelect).toHaveBeenCalledWith({
      kind: "saved_sql",
      seed: {
        entryId: "sql-1",
        revisionId: "sql-rev-1",
        name: "Monthly",
        expectedResult: [{ name: "total", type: "float" }],
        variables: {},
        variableTypes: {},
      },
    });
  });

  it("switches the source lists behind Forms and Saved SQL tabs", async () => {
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    const formsTab = await screen.findByRole("tab", { name: "Forms" });
    const sqlTab = screen.getByRole("tab", { name: "Saved SQL" });
    expect(screen.getByRole("tablist")).toBeInTheDocument();
    expect(formsTab).toHaveAttribute("aria-selected", "true");
    expect(formsTab).toHaveAttribute(
      "aria-controls",
      "composition-source-panel-forms",
    );
    expect(sqlTab).toHaveAttribute("aria-selected", "false");
    expect(
      screen.getByRole("tabpanel", { name: "Forms" }),
    ).toHaveAttribute("id", "composition-source-panel-forms");
    expect(
      screen.queryByRole("tabpanel", { name: "Saved SQL" }),
    ).toBeNull();
    expect(
      await screen.findByRole("button", { name: "Tasks" }),
    ).toBeInTheDocument();

    fireEvent.click(sqlTab);
    expect(sqlTab).toHaveAttribute("aria-selected", "true");
    expect(formsTab).toHaveAttribute("aria-selected", "false");
    expect(
      screen.getByRole("tabpanel", { name: "Saved SQL" }),
    ).toHaveAttribute("id", "composition-source-panel-saved-sql");
    expect(screen.queryByRole("tabpanel", { name: "Forms" })).toBeNull();
    expect(
      await screen.findByRole("button", { name: "Monthly" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Tasks" })).toBeNull();
  });

  it("hides internal registry forms while keeping user forms", async () => {
    formListMock.mockResolvedValue([
      taskForm,
      {
        id: "22222222-2222-4222-8222-222222222222",
        name: "_ugoite_compositions",
        version: 1,
        template: "task",
        fields: {},
      },
      {
        id: "33333333-3333-4333-8333-333333333333",
        name: "SQL",
        version: 1,
        template: "task",
        fields: {},
      },
    ]);
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    expect(
      await screen.findByRole("button", { name: "Tasks" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "_ugoite_compositions" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "SQL" })).toBeNull();
  });

  it("renders the shared list-page row labels in the picker", async () => {
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    // Forms rows reuse the list-page label: glyph plus human name.
    const tasksRow = await screen.findByRole("button", { name: "Tasks" });
    expect(tasksRow.querySelector(".glyph")).toHaveTextContent("T");
    expect(tasksRow.querySelector(".formRowName")).toHaveTextContent("Tasks");

    // Saved SQL rows reuse the list-page label: the human query name.
    fireEvent.click(await screen.findByRole("tab", { name: "Saved SQL" }));
    expect(
      await screen.findByRole("button", { name: "Monthly" }),
    ).toBeInTheDocument();
  });

  it("keeps the source picker dialog at a fixed readable width", async () => {
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveClass("composition-source-picker");
    expect(stylesheet()).toMatch(
      /\.ui-dialog\.composition-source-picker[\s\S]*?width:\s*min\(560px,\s*calc\(100vw - 32px\)\)/,
    );
  });

  it("preserves loading, empty, and error states across tabs", async () => {
    let resolveForms!: (value: typeof taskForm[]) => void;
    let resolveSql!: (value: typeof monthlyEntry[]) => void;
    formListMock.mockReturnValueOnce(
      new Promise<typeof taskForm[]>((resolve) => {
        resolveForms = resolve;
      }),
    );
    sqlListMock.mockReturnValueOnce(
      new Promise<typeof monthlyEntry[]>((resolve) => {
        resolveSql = resolve;
      }),
    );
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    // Loading: the panel-local status announces while lists resolve.
    expect(await screen.findByRole("status")).toBeInTheDocument();

    // Empty per tab: each tab keeps its own empty state.
    resolveForms([]);
    resolveSql([]);
    expect(await screen.findByText("No forms")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Saved SQL" }));
    expect(await screen.findByText("No saved SQL")).toBeInTheDocument();
  });

  it("recovers from a list failure through retry", async () => {
    formListMock.mockRejectedValueOnce(new Error("offline"));
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    expect(
      await screen.findByText("Could not load data sources."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(
      await screen.findByRole("button", { name: "Tasks" }),
    ).toBeInTheDocument();
  });
});
