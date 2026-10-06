import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale } from "~/lib/i18n";
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
    const tasksRow = await screen.findByRole("button", { name: "Tasks" });
    const monthlyRow = await screen.findByRole("button", { name: "Monthly" });
    // Full-row selection: no boxed chevrons, human names only.
    expect(tasksRow.textContent).not.toMatch(/›/);
    expect(monthlyRow.textContent).not.toMatch(/›/);

    fireEvent.click(tasksRow);
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

  it("pins the exact saved sql revision with server-owned column types", async () => {
    const onSelect = vi.fn();
    render(() => (
      <CompositionSourcePicker
        spaceId="space-1"
        onSelect={onSelect}
        onClose={() => {}}
      />
    ));

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
});
