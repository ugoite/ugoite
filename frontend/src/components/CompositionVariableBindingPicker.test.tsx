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
import { CompositionVariableBindingPicker } from "./CompositionVariableBindingPicker";
import {
  type CompositionBindingProvider,
  type CompositionGraphDraft,
  type CompositionVariableNode,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";

const session = {
  session_id: "picker-session",
  space_id: "space-1",
  document_revision: "revision-1",
  principal_generation: "principal-generation-1",
};

const formQuery: CompositionVariableNode & { kind: "query" } = {
  id: "customer_rows",
  kind: "query",
  returns: {
    kind: "table",
    columns: [{
      name: "name",
      value_type: { scalar_type: "string", nullable: false },
    }],
  },
  query: {
    kind: "inline_sql",
    sql: 'SELECT * FROM "Customers"',
    arguments: [],
    form_bindings: [{ relation: "Customers", form_id: "form-id" }],
    schema_fingerprint: "fingerprint",
  },
};
const metricQuery: CompositionVariableNode & { kind: "query" } = {
  id: "customer_count",
  kind: "query",
  returns: {
    kind: "scalar",
    value_type: { scalar_type: "integer", nullable: false },
  },
  query: {
    kind: "inline_sql",
    sql: 'SELECT COUNT(*) AS count FROM "Customers"',
    arguments: [],
    form_bindings: [{ relation: "Customers", form_id: "form-id" }],
    schema_fingerprint: "fingerprint",
  },
};

describe("CompositionVariableBindingPicker", () => {
  beforeEach(() => setLocale("en"));
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  const forms = [{
    id: "form-id",
    name: "Customers",
    relation: "Customers",
    fields: [{
      name: "Total",
      column: "field_total",
      type: "decimal" as const,
    }],
  }];
  const renderPicker = (
    props: Partial<Parameters<typeof CompositionVariableBindingPicker>[0]> & {
      provider: CompositionBindingProvider;
      kind: "table" | "metric";
      onCommit: (draft: ReturnType<typeof createCompositionGraphDraft>) => void;
    },
  ) =>
    render(() => (
      <CompositionVariableBindingPicker
        kind={props.kind}
        draft={props.draft ?? createCompositionGraphDraft("Report")}
        forms={props.forms ?? forms}
        savedSql={props.savedSql ?? []}
        provider={props.provider}
        session={props.session ?? session}
        getPrincipalGeneration={props.getPrincipalGeneration ??
          (async () => session.principal_generation)}
        isSessionCurrent={props.isSessionCurrent ?? (() => true)}
        componentId={props.componentId}
        onCommit={props.onCommit}
        onClose={props.onClose ?? (() => {})}
      />
    ));

  it("inserts a Form table with only a variable reference", async () => {
    const onCommit = vi.fn();
    const provider: CompositionBindingProvider = {
      resolveForm: vi.fn(async () => ({ variable: formQuery })),
      prepareSavedSql: vi.fn(),
    };
    renderPicker({ kind: "table", provider, onCommit });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Forms" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Customers" }));
    await waitFor(() => expect(onCommit).toHaveBeenCalledOnce());
    const saved = onCommit.mock.calls[0][0];
    expect(saved.variables).toEqual([formQuery]);
    expect(saved.components).toEqual([{
      id: "component_1",
      kind: "table",
      variable: "customer_rows",
    }]);
    expect(JSON.stringify(saved.components[0])).not.toContain("form-id");
    expect(provider.resolveForm).toHaveBeenCalledWith(
      expect.objectContaining({ form_id: "form-id", component: "table" }),
    );
  });

  it("keeps the picker open when Saved SQL inspection finishes after a tab switch", async () => {
    const onClose = vi.fn();
    let finish!: (value: {
      table: { variable: typeof formQuery };
      scalars: [];
    }) => void;
    const pending = new Promise<{
      table: { variable: typeof formQuery };
      scalars: [];
    }>((resolve) => finish = resolve);
    const inspectSavedSql = vi.fn(() => pending);
    const provider: CompositionBindingProvider = {
      resolveForm: vi.fn(),
      prepareSavedSql: vi.fn(),
      inspectSavedSql,
    };
    renderPicker({
      kind: "table",
      provider,
      savedSql: [{
        entry_id: "saved-entry",
        revision_id: "saved-revision",
        name: "Saved table",
        arguments: [],
      }],
      onCommit: vi.fn(),
      onClose,
    });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Saved SQL" }));
    await waitFor(() => expect(inspectSavedSql).toHaveBeenCalledOnce());
    fireEvent.click(within(dialog).getByRole("tab", { name: "Variables" }));

    finish({ table: { variable: formQuery }, scalars: [] });
    await pending;
    await Promise.resolve();

    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Table" })).toBeVisible();
    expect(within(dialog).getByRole("tab", { name: "Variables" }))
      .toHaveAttribute(
        "aria-selected",
        "true",
      );
  });

  it("requires explicit aggregate selection for a Form metric", async () => {
    const onCommit = vi.fn();
    const resolveForm = vi.fn(async () => ({ variable: metricQuery }));
    renderPicker({
      kind: "metric",
      provider: { resolveForm, prepareSavedSql: vi.fn() },
      onCommit,
    });
    const dialog = screen.getByRole("dialog", { name: "Metric" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Forms" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Customers" }));
    expect(onCommit).not.toHaveBeenCalled();
    fireEvent.change(
      within(dialog).getByRole("combobox", { name: "Aggregation" }),
      { target: { value: "count" } },
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Add" }));
    await waitFor(() => expect(onCommit).toHaveBeenCalledOnce());
    expect(resolveForm).toHaveBeenCalledWith(
      expect.objectContaining({ aggregate: { kind: "count" } }),
    );
    expect(onCommit.mock.calls[0][0].components).toEqual([{
      id: "component_1",
      kind: "metric",
      variable: "customer_count",
    }]);
  });

  it("shows Form field names while binding the SQL column", async () => {
    const onCommit = vi.fn();
    const resolveForm = vi.fn(async () => ({ variable: metricQuery }));
    renderPicker({
      kind: "metric",
      provider: { resolveForm, prepareSavedSql: vi.fn() },
      onCommit,
    });
    const dialog = screen.getByRole("dialog", { name: "Metric" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Forms" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Customers" }));
    fireEvent.change(
      within(dialog).getByRole("combobox", { name: "Aggregation" }),
      { target: { value: "sum" } },
    );

    const column = within(dialog).getByRole("combobox", { name: "Column" });
    expect(within(column).getByRole("option", { name: "Total" }))
      .toHaveValue("field_total");
    fireEvent.change(column, { target: { value: "field_total" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Add" }));

    await waitFor(() => expect(resolveForm).toHaveBeenCalledOnce());
    expect(resolveForm).toHaveBeenCalledWith(
      expect.objectContaining({
        aggregate: { kind: "sum", column: "field_total" },
      }),
    );
  });

  it("pins exact Saved SQL revision and chosen scalar column", async () => {
    const onCommit = vi.fn();
    const entry = {
      entry_id: "entry-id",
      revision_id: "revision-7",
      name: "Monthly total",
      arguments: [],
    };
    const prepareSavedSql = vi.fn(async () => ({
      scalars: [{
        column: "total",
        binding: {
          variable: {
            ...metricQuery,
            query: {
              ...metricQuery.query,
              kind: "saved_sql" as const,
              entry_id: entry.entry_id,
              revision_id: entry.revision_id,
              column: "total",
            },
          },
        },
      }],
    }));
    renderPicker({
      kind: "metric",
      provider: { resolveForm: vi.fn(), prepareSavedSql },
      savedSql: [entry],
      onCommit,
    });
    const dialog = screen.getByRole("dialog", { name: "Metric" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Monthly total" }),
    );
    await waitFor(() => expect(onCommit).toHaveBeenCalledOnce());
    expect(prepareSavedSql).toHaveBeenCalledWith(
      expect.objectContaining({
        entry_id: "entry-id",
        revision_id: "revision-7",
      }),
    );
  });

  it("rejects a prepared Saved SQL scalar after the principal changes", async () => {
    const onCommit = vi.fn();
    const onClose = vi.fn();
    let principalGeneration = "account:editor";
    const entry = {
      entry_id: "saved-entry",
      revision_id: "saved-revision",
      name: "Monthly totals",
      arguments: [],
    };
    const savedScalar = (column: string) => ({
      column,
      binding: {
        variable: {
          ...metricQuery,
          id: `saved_${column}`,
          query: {
            kind: "saved_sql" as const,
            entry_id: entry.entry_id,
            revision_id: entry.revision_id,
            column,
            arguments: [],
            form_bindings: [],
            schema_fingerprint: "fingerprint",
          },
        },
      },
    });
    const prepareSavedSql = vi.fn(async () => ({
      scalars: [savedScalar("total"), savedScalar("count")],
    }));
    renderPicker({
      kind: "metric",
      provider: { resolveForm: vi.fn(), prepareSavedSql },
      savedSql: [entry],
      onCommit,
      onClose,
      getPrincipalGeneration: async () => principalGeneration,
      isSessionCurrent: async (candidate) =>
        candidate.principal_generation === principalGeneration,
    });
    const dialog = screen.getByRole("dialog", { name: "Metric" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Monthly totals" }),
    );
    const totalChoice = await within(dialog).findByRole("button", {
      name: "total",
    });
    expect(within(dialog).getByRole("button", { name: "count" }))
      .toBeVisible();

    principalGeneration = "account:viewer";
    fireEvent.click(totalChoice);

    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("discards a resolved query after the authoring session becomes stale", async () => {
    const onCommit = vi.fn();
    const onClose = vi.fn();
    let principalGeneration = session.principal_generation;
    let finish!: (value: { variable: typeof formQuery }) => void;
    const pending = new Promise<{ variable: typeof formQuery }>((resolve) => {
      finish = resolve;
    });
    const provider: CompositionBindingProvider = {
      resolveForm: vi.fn(() => pending),
      prepareSavedSql: vi.fn(),
    };
    renderPicker({
      kind: "table",
      provider,
      onCommit,
      onClose,
      getPrincipalGeneration: async () => principalGeneration,
      isSessionCurrent: (candidate) =>
        candidate.principal_generation === principalGeneration,
    });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Forms" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Customers" }));
    await waitFor(() => expect(provider.resolveForm).toHaveBeenCalledOnce());
    principalGeneration = "different-principal";
    finish({ variable: formQuery });
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("discards a deferred Saved SQL result after the principal changes", async () => {
    const onCommit = vi.fn();
    const onClose = vi.fn();
    let principalGeneration = "account:editor";
    let finish!: (value: {
      table: { variable: typeof formQuery };
      scalars: [];
    }) => void;
    const pending = new Promise<{
      table: { variable: typeof formQuery };
      scalars: [];
    }>((resolve) => finish = resolve);
    const provider: CompositionBindingProvider = {
      resolveForm: vi.fn(),
      prepareSavedSql: vi.fn(() => pending),
    };
    renderPicker({
      kind: "table",
      provider,
      savedSql: [{
        entry_id: "saved-entry",
        revision_id: "saved-revision",
        name: "Saved table",
        arguments: [],
      }],
      onCommit,
      onClose,
      getPrincipalGeneration: async () => principalGeneration,
      isSessionCurrent: (candidate) =>
        candidate.principal_generation === principalGeneration,
    });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Saved table" }),
    );
    await waitFor(() =>
      expect(provider.prepareSavedSql).toHaveBeenCalledOnce()
    );

    principalGeneration = "account:viewer";
    finish({ table: { variable: formQuery }, scalars: [] });

    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
    expect(onCommit).not.toHaveBeenCalled();
  });

  it.each([
    { label: "Variables", panel: "variables" },
    { label: "Forms", panel: "forms" },
    { label: "Saved SQL", panel: "saved_sql" },
  ])("cycles through visible controls on the $label tab", async ({
    label,
    panel,
  }) => {
    const input: CompositionVariableNode = {
      id: "search_term",
      kind: "input",
      value_type: { scalar_type: "string", nullable: false },
      required: false,
    };
    const draft: CompositionGraphDraft = {
      ...createCompositionGraphDraft("Report"),
      variables: [formQuery, input],
    };
    const provider: CompositionBindingProvider = {
      resolveForm: vi.fn(async () => ({ variable: formQuery })),
      prepareSavedSql: vi.fn(async () => ({ scalars: [] })),
    };
    renderPicker({
      kind: "table",
      provider,
      draft,
      savedSql: [{
        entry_id: "saved-entry",
        revision_id: "saved-revision",
        name: "Saved table",
        arguments: [{ name: "search", type: "string" }],
      }],
      onCommit: vi.fn(),
    });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: label }));
    if (panel === "saved_sql") {
      fireEvent.click(
        within(dialog).getByRole("button", { name: "Saved table" }),
      );
    }
    const lastActiveControl = panel === "variables"
      ? within(dialog).getByRole("button", { name: "customer_rows" })
      : panel === "forms"
      ? within(dialog).getByRole("button", { name: "Customers" })
      : within(dialog).getByRole("combobox", {
        name: "Variable for SQL argument search",
      });
    lastActiveControl.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    const closeButton = within(dialog).getByRole("button", { name: "Close" });
    expect(document.activeElement).toBe(closeButton);
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(lastActiveControl);
  });

  it("uses the same picker to change only an existing component reference", () => {
    const existing: CompositionVariableNode = {
      ...formQuery,
      id: "existing_rows",
    };
    const draft: CompositionGraphDraft = {
      ...createCompositionGraphDraft(),
      variables: [formQuery, existing],
      components: [{
        id: "table-1",
        kind: "table" as const,
        variable: "customer_rows",
      }],
      layout: {
        kind: "flow" as const,
        rows: [{
          id: "main",
          items: [{ kind: "component", component: "table-1" }],
        }],
      },
    };
    const onCommit = vi.fn();
    renderPicker({
      kind: "table",
      draft,
      componentId: "table-1",
      provider: { resolveForm: vi.fn(), prepareSavedSql: vi.fn() },
      onCommit,
    });
    fireEvent.click(screen.getByRole("button", { name: "existing_rows" }));
    expect(onCommit.mock.calls[0][0].components).toEqual([{
      id: "table-1",
      kind: "table",
      variable: "existing_rows",
    }]);
    expect(onCommit.mock.calls[0][0].variables).toEqual(draft.variables);
  });
});
