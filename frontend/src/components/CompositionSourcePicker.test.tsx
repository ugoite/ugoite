import "@testing-library/jest-dom/vitest";
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

vi.mock("~/lib/ugoite-client", () => ({
  formApi: {
    list: (...args: unknown[]) => formListMock(...args),
  },
  sqlApi: {
    list: (...args: unknown[]) => sqlListMock(...args),
    get: (...args: unknown[]) => sqlGetMock(...args),
    query: (...args: unknown[]) => sqlQueryMock(...args),
  },
}));

const FORM_ID = "11111111-1111-4111-8111-111111111111";
const taskForm = {
  id: FORM_ID,
  name: "Tasks",
  version: 1,
  template: "task",
  fields: {},
};
const monthlySql = {
  id: "sql-1",
  name: "Monthly",
  kind: "user-query",
  sql: "SELECT total FROM monthly WHERE month = $month",
  variables: [{ name: "month", type: "date", description: "", default: "2026-10-01" }],
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-02T00:00:00Z",
  revision_id: "sql-rev-7",
} as const;

describe("CompositionSourcePicker", () => {
  beforeEach(() => {
    setLocale("en");
    vi.clearAllMocks();
    formListMock.mockResolvedValue([taskForm]);
    sqlListMock.mockResolvedValue([monthlySql]);
    sqlGetMock.mockResolvedValue(monthlySql);
    sqlQueryMock.mockResolvedValue({
      columns: ["total"],
      rows: [[1]],
      has_more: false,
      result_schema: [{ name: "total", type: "decimal" }],
    });
  });

  afterEach(() => cleanup());

  it("creates editable inline SQL when a Form is selected", async () => {
    const onSelect = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));

    const dialog = await screen.findByRole("dialog", { name: "Choose a source" });
    expect(await within(dialog).findByRole("button", { name: "Tasks" }))
      .toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Tasks" }));

    expect(onSelect).toHaveBeenCalledWith({
      kind: "inline_sql",
      seed: {
        name: "Tasks",
        sql: 'SELECT * FROM "Tasks"',
      },
    });
    expect(JSON.stringify(onSelect.mock.calls[0])).not.toContain("entry_query");
    expect(formListMock).toHaveBeenCalledWith("space-1");
  });

  it("pins the selected Saved SQL revision and copies its server-owned schema and defaults", async () => {
    const onSelect = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));
    const dialog = await screen.findByRole("dialog", { name: "Choose a source" });
    fireEvent.click(await within(dialog).findByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(await within(dialog).findByRole("button", { name: "Monthly" }));

    await waitFor(() => expect(onSelect).toHaveBeenCalledTimes(1));
    expect(sqlGetMock).toHaveBeenCalledWith("space-1", "sql-1");
    expect(sqlQueryMock).toHaveBeenCalledWith("space-1", {
      sql: "SELECT total FROM monthly WHERE month = $month",
      parameters: { month: "2026-10-01" },
      parameter_types: { month: "date" },
      limit: 1,
      saved_sql: { id: "sql-1", revision_id: "sql-rev-7" },
    });
    expect(onSelect).toHaveBeenCalledWith({
      kind: "saved_sql",
      seed: expect.objectContaining({
        entryId: "sql-1",
        revisionId: "sql-rev-7",
        expectedResult: [{ name: "total", type: "decimal" }],
        variables: {},
        variableTypes: { month: "date" },
        variableDefaults: { month: "2026-10-01" },
      }),
    });
  });

  it("shows existing SQL sources first and reuses their draft identity", async () => {
    const existing: DraftSource = {
      kind: "inline_sql",
      draftId: "draft-source-1",
      name: "Expenses",
      sql: 'SELECT * FROM "Expenses"',
      formBindings: [],
      expectedResult: [],
      schemaFingerprint: "",
    };
    const onSelectExisting = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        existingSources={[existing]}
        onSelectExisting={onSelectExisting}
        onSelect={() => {}}
        onClose={() => {}}
      />
    ));

    const dialog = await screen.findByRole("dialog", { name: "Choose a source" });
    expect(await within(dialog).findByRole("heading", { name: "In this composition" }))
      .toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Expenses, SQL" }));
    expect(onSelectExisting).toHaveBeenCalledWith("draft-source-1");
    expect(dialog).not.toHaveTextContent("draft-source-1");
  });

  it("keeps Forms and Saved SQL in separate source tabs and permits blank SQL", async () => {
    const onSelect = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));
    const dialog = await screen.findByRole("dialog", { name: "Choose a source" });
    expect(await within(dialog).findByRole("tabpanel", { name: "Forms" }))
      .toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Monthly" }))
      .not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "SQL" }));
    expect(onSelect).toHaveBeenCalledWith({
      kind: "inline_sql",
      seed: { name: "SQL", sql: "SELECT 1" },
    });
  });

  it("rejects a Saved SQL source when the server cannot provide a result schema", async () => {
    sqlQueryMock.mockResolvedValueOnce({ columns: [], rows: [], has_more: false });
    const onSelect = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));
    const dialog = await screen.findByRole("dialog", { name: "Choose a source" });
    fireEvent.click(await within(dialog).findByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(await within(dialog).findByRole("button", { name: "Monthly" }));

    expect(await within(dialog).findByRole("alert"))
      .toHaveTextContent("This source cannot be added as tool data.");
    expect(onSelect).not.toHaveBeenCalled();
  });
});
