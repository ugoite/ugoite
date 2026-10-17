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
import { CompositionDataWorkspace } from "./CompositionDataWorkspace";
import {
  addInlineSqlSource,
  addSavedSqlSource,
  createEmptyDraft,
} from "~/lib/composition-draft";
import type { CompositionResolvedSource } from "~/lib/composition-api";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import { setLocale } from "~/lib/i18n";

vi.mock("~/components/SQLWorkspace", () => ({
  SQLWorkspace: (props: Record<string, unknown> & {
    sql: string;
    onSqlChange?: (sql: string) => void;
    onPreview: () => void;
    onNext?: () => void;
    preview: { page?: { rows: unknown[][] } };
  }) => (
    <div
      data-testid="sql-workspace"
      data-readonly={String(Boolean(props.readOnly))}
      data-variables={JSON.stringify(props.variables ?? [])}
    >
      <textarea
        aria-label="SQL"
        value={props.sql}
        readOnly={Boolean(props.readOnly)}
        onInput={(event) => props.onSqlChange?.(event.currentTarget.value)}
      />
      <button type="button" onClick={props.onPreview}>Run query</button>
      <button type="button" onClick={props.onNext}>Next</button>
      <output>{JSON.stringify(props.preview.page?.rows ?? [])}</output>
    </div>
  ),
}));

vi.mock("~/components/SqlStructureToolbar", () => ({
  SqlStructureToolbar: () => null,
}));

vi.mock("@solidjs/router", () => ({
  A: (props: { href: string; class?: string; children: unknown }) => (
    <a href={props.href} class={props.class}>{props.children as never}</a>
  ),
}));

const inlineDraft = () => {
  const added = addInlineSqlSource(createEmptyDraft("Dashboard"), {
    name: "Expenses",
    sql: 'SELECT "Amount" AS total FROM "Expenses"',
    formBindings: [{ relation: "Expenses", form_id: "form-1" }],
    expectedResult: [{ name: "total", type: "decimal" }],
    schemaFingerprint: "a".repeat(64),
  });
  return added.draft;
};

const savedSqlDraft = () => {
  const added = addSavedSqlSource(createEmptyDraft("Dashboard"), {
    entryId: "sql-1",
    revisionId: "rev-7",
    name: "Monthly totals",
    sql: "SELECT SUM(amount) AS total FROM expenses",
    expectedResult: [{ name: "total", type: "decimal" }],
    schemaFingerprint: "b".repeat(64),
    variables: { period: { variable: "month" } },
    variableTypes: { period: "date" },
  });
  return added.draft;
};

const readyPageState = (): CompositionSourcePageState => ({
  status: "ready",
  cursorStack: [undefined],
  page: {
    kind: "sql",
    page: {
      columns: ["total"],
      rows: [[42]],
      has_more: true,
      next: "cursor-2",
    },
  },
});

const propsFor = (
  draft: ReturnType<typeof inlineDraft>,
  overrides: Partial<Parameters<typeof CompositionDataWorkspace>[0]> = {},
): Parameters<typeof CompositionDataWorkspace>[0] => ({
  draft,
  headingId: "data-heading",
  selectedSourceId: "src-1",
  onSelectSource: vi.fn(),
  onAddSource: vi.fn(),
  onMoveSource: vi.fn(),
  onRemoveSource: vi.fn(),
  onInlineSql: vi.fn(() => true),
  onInlineSqlName: vi.fn(() => true),
  variables: [],
  variableValues: {},
  savedSqlEditHref: (entryId) => `/spaces/space-1/sql/${entryId}`,
  sqlSchema: { tables: {} },
  forms: [],
  planSources: [],
  sourceStates: { "src-1": readyPageState() },
  previewActive: false,
  onNext: vi.fn(),
  onPrevious: vi.fn(),
  onRetry: vi.fn(),
  registerSourceRow: vi.fn(),
  ...overrides,
});

describe("CompositionDataWorkspace", () => {
  let originalMatchMedia: typeof window.matchMedia;

  beforeEach(() => {
    setLocale("en");
    originalMatchMedia = window.matchMedia;
    window.matchMedia = (() => ({
      matches: false,
      media: "(max-width: 560px)",
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    })) as typeof window.matchMedia;
  });

  afterEach(() => {
    cleanup();
    window.matchMedia = originalMatchMedia;
  });

  it("edits inline SQL directly and passes the current SQL page to the shared workspace", async () => {
    const props = propsFor(inlineDraft());
    render(() => (
      <>
        <h2 id="data-heading">Data</h2>
        <CompositionDataWorkspace {...props} />
      </>
    ));

    const editor = screen.getByRole("region", { name: "Expenses" });
    expect(editor).toBeInTheDocument();
    expect(editor).not.toHaveTextContent("form-1");
    expect(screen.queryByRole("textbox", { name: "Query name" })).toBeNull();
    const renameButton = screen.getByRole("button", { name: "Rename Expenses" });
    fireEvent.click(renameButton);
    const nameInput = screen.getByRole("textbox", { name: "Query name" });
    await waitFor(() => expect(nameInput).toHaveFocus());
    expect(nameInput).toHaveValue("Expenses");
    const sql = screen.getByRole("textbox", { name: "SQL" });
    expect(sql).toHaveValue('SELECT "Amount" AS total FROM "Expenses"');
    fireEvent.input(sql, { target: { value: "SELECT 1" } });
    expect(props.onInlineSql).toHaveBeenCalledWith("src-1", "SELECT 1");
    fireEvent.input(nameInput, {
      target: { value: "Costs" },
    });
    expect(props.onInlineSqlName).toHaveBeenCalledWith("src-1", "Costs");
    fireEvent.keyDown(nameInput, { key: "Enter" });
    await waitFor(() => {
      expect(screen.queryByRole("textbox", { name: "Query name" })).toBeNull();
      expect(screen.getByRole("button", { name: "Rename Expenses" })).toHaveFocus();
    });
    expect(screen.getByTestId("sql-workspace")).toHaveAttribute(
      "data-readonly",
      "false",
    );
    expect(screen.getByText("[[42]]")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    fireEvent.click(screen.getByRole("button", { name: "Run query" }));
    expect(props.onNext).toHaveBeenCalledWith("src-1");
    expect(props.onRetry).toHaveBeenCalledWith("src-1");
  });

  it("shows each SQL source name once and keeps rename behind an explicit action", () => {
    render(() => (
      <>
        <h2 id="data-heading">Data</h2>
        <CompositionDataWorkspace {...propsFor(inlineDraft())} />
      </>
    ));

    const row = screen.getByRole("button", { name: "Expenses" });
    expect(row).toHaveTextContent(/^Expenses$/);
    expect(row.querySelector(".rowListSecondary")).toBeNull();
    expect(row.querySelector("svg")).toBeNull();
    expect(screen.getAllByText("Expenses")).toHaveLength(1);
    expect(screen.queryByRole("textbox", { name: "Query name" })).toBeNull();
    expect(screen.getByRole("button", { name: "Rename Expenses" }))
      .toBeInTheDocument();
  });

  it("keeps Saved SQL read-only at its exact revision and links to explicit editing", () => {
    const draft = savedSqlDraft();
    const planSource: CompositionResolvedSource = {
      kind: "saved_sql",
      source_id: "src-1",
      request: {
        sql: "SELECT SUM(amount) AS total FROM expenses",
        limit: 50,
        parameter_types: { period: "date" },
        saved_sql: { id: "sql-1", revision_id: "rev-7" },
      },
      source_schema_fingerprint: "b".repeat(64),
      expected_result: [{ name: "total", type: "decimal" }],
    };
    const props = propsFor(draft, {
      variables: [{ id: "month", type: "date", required: true }],
      variableValues: { month: "2026-10-01" },
      planSources: [planSource],
    });
    render(() => (
      <>
        <h2 id="data-heading">Data</h2>
        <CompositionDataWorkspace {...props} />
      </>
    ));

    const workspace = screen.getByTestId("sql-workspace");
    expect(workspace).toHaveAttribute("data-readonly", "true");
    expect(workspace).toHaveAttribute(
      "data-variables",
      JSON.stringify([{ name: "period", type: "date", required: true }]),
    );
    expect(screen.getByRole("textbox", { name: "SQL" })).toHaveAttribute(
      "readonly",
    );
    expect(screen.getByRole("link", { name: "Edit Saved SQL" })).toHaveAttribute(
      "href",
      "/spaces/space-1/sql/sql-1",
    );
  });

  it("shows an accessible mobile source drawer and restores focus when it closes", async () => {
    window.matchMedia = ((query: string) => ({
      matches: query === "(max-width: 560px)",
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    })) as typeof window.matchMedia;
    const props = propsFor(inlineDraft(), { selectedSourceId: null });
    render(() => (
      <>
        <h2 id="data-heading">Data</h2>
        <CompositionDataWorkspace {...props} />
      </>
    ));

    const drawer = screen.getByRole("dialog", { name: "Data" });
    expect(within(drawer).getByRole("button", { name: "Expenses" }))
      .toBeInTheDocument();
    fireEvent.keyDown(drawer, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Data" })).toBeNull();
    });
    const reopen = screen.getByRole("button", { name: "Show data sources" });
    await waitFor(() => expect(reopen).toHaveFocus());
    fireEvent.click(reopen);
    expect(screen.getByRole("dialog", { name: "Data" })).toBeInTheDocument();
  });

  it("returns source picker focus to the visible source menu after drawer Add", async () => {
    window.matchMedia = ((query: string) => ({
      matches: query === "(max-width: 560px)",
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    })) as typeof window.matchMedia;
    const props = propsFor(inlineDraft(), { selectedSourceId: null });
    render(() => (
      <>
        <h2 id="data-heading">Data</h2>
        <CompositionDataWorkspace {...props} />
      </>
    ));

    const drawer = screen.getByRole("dialog", { name: "Data" });
    fireEvent.click(within(drawer).getByRole("button", { name: "Add source" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Data" })).toBeNull();
    });

    const trigger = screen.getByRole("button", { name: "Show data sources" });
    const returnFocus = vi.mocked(props.onAddSource).mock.calls[0]?.[0];
    expect(returnFocus?.()).toBe(trigger);
    returnFocus?.()?.focus();
    expect(trigger).toHaveFocus();
  });
});
