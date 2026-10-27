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
  CompositionBindingDiagnosticError,
  type CompositionBindingProvider,
  type CompositionGraphDraft,
  type CompositionVariableNode,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { setLocale } from "~/lib/i18n";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";

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

  it("keeps a failed Form selection retryable without committing partial state", async () => {
    const onCommit = vi.fn();
    let attempts = 0;
    const provider: CompositionBindingProvider = {
      resolveForm: vi.fn(async () => {
        attempts += 1;
        if (attempts === 1) {
          throw new CompositionBindingDiagnosticError("query_schema_changed");
        }
        return { variable: formQuery };
      }),
      prepareSavedSql: vi.fn(),
    };
    const draft = createCompositionGraphDraft("Report");
    renderPicker({ kind: "table", provider, onCommit, draft });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Forms" }));
    const form = within(dialog).getByRole("button", { name: "Customers" });
    fireEvent.click(form);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "This Form changed while you were binding it.",
    );
    expect(form).toHaveAttribute("aria-pressed", "true");
    expect(onCommit).not.toHaveBeenCalled();
    expect(draft.components).toEqual([]);
    expect(screen.getByRole("dialog", { name: "Table" })).toBeInTheDocument();

    fireEvent.click(form);
    await waitFor(() => expect(onCommit).toHaveBeenCalledOnce());
    expect(provider.resolveForm).toHaveBeenCalledTimes(2);
    expect(onCommit.mock.calls[0][0].components).toEqual([{
      id: "component_1",
      kind: "table",
      variable: "customer_rows",
    }]);
  });

  it("shows a safe actionable message for a typed missing Form error", async () => {
    const provider: CompositionBindingProvider = {
      resolveForm: vi.fn(async () => {
        throw new UgoiteApiError({
          kind: "not_found",
          code: "FORM_NOT_FOUND",
          operation: "composition.preview",
          status: 404,
          message: 'Could not resolve SELECT * FROM "form_sensitive-id"',
          detail: { message: 'SELECT * FROM "form_sensitive-id"' },
        });
      }),
      prepareSavedSql: vi.fn(),
    };
    renderPicker({ kind: "table", provider, onCommit: vi.fn() });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Forms" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Customers" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("This Form is unavailable");
    expect(alert.textContent).not.toContain("form_sensitive-id");
    expect(alert.textContent).not.toContain("SELECT");
    expect(dialog).toBeInTheDocument();
  });

  it("prepares only the selected exact Saved SQL revision without inspecting earlier rows", async () => {
    const onCommit = vi.fn();
    const prepareSavedSql = vi.fn(async (
      _input: Parameters<CompositionBindingProvider["prepareSavedSql"]>[0],
    ) => ({
      table: {
        variable: {
          ...formQuery,
          query: {
            kind: "saved_sql" as const,
            arguments: [],
            form_bindings: [],
            schema_fingerprint: "fingerprint",
            entry_id: "entry-39",
            revision_id: "revision-39",
          },
        },
      },
      scalars: [],
    }));
    const provider: CompositionBindingProvider = {
      resolveForm: vi.fn(),
      prepareSavedSql,
    };
    const savedSql = Array.from({ length: 40 }, (_, index) => ({
      entry_id: `entry-${index}`,
      revision_id: `revision-${index}`,
      name: `Saved table ${index}`,
      arguments: [],
    }));
    renderPicker({ kind: "table", provider, savedSql, onCommit });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Saved SQL" }));
    expect(prepareSavedSql).not.toHaveBeenCalled();
    const selected = within(dialog).getByRole("button", {
      name: "Saved table 39",
      exact: true,
    });
    expect(selected).toBeEnabled();
    fireEvent.click(selected);
    await waitFor(() => expect(onCommit).toHaveBeenCalledOnce());
    expect(onCommit.mock.calls[0][0].variables[0].query).toMatchObject({
      kind: "saved_sql",
      entry_id: "entry-39",
      revision_id: "revision-39",
    });
    expect(onCommit.mock.calls[0][0].components[0]).toEqual({
      id: "component_1",
      kind: "table",
      variable: "customer_rows",
    });
    expect(prepareSavedSql).toHaveBeenCalledOnce();
    expect(prepareSavedSql.mock.calls[0][0]).toMatchObject({
      entry_id: "entry-39",
      revision_id: "revision-39",
      arguments: [],
    });
  });

  it("waits for explicit argument binding without creating sample variables", async () => {
    const period: CompositionVariableNode = {
      id: "period",
      kind: "constant",
      value_type: { scalar_type: "date", nullable: false },
      value: { kind: "date", value: "2026-10-01" },
    };
    const draft = {
      ...createCompositionGraphDraft("Report"),
      variables: [period],
    };
    const prepareSavedSql = vi.fn(async (
      _input: Parameters<CompositionBindingProvider["prepareSavedSql"]>[0],
    ) => ({ table: { variable: formQuery }, scalars: [] }));
    renderPicker({
      kind: "table",
      draft,
      provider: { resolveForm: vi.fn(), prepareSavedSql },
      savedSql: [{
        entry_id: "dated",
        revision_id: "exact",
        name: "By date",
        arguments: [{ name: "from", type: "date" }],
      }],
      onCommit: vi.fn(),
    });
    const dialog = screen.getByRole("dialog", { name: "Table" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "By date" }));
    expect(prepareSavedSql).not.toHaveBeenCalled();
    const add = within(dialog).getByRole("button", {
      name: "Add",
      exact: true,
    });
    expect(add).toBeDisabled();
    fireEvent.change(
      within(dialog).getByRole("combobox", {
        name: "Variable for SQL argument from",
      }),
      { target: { value: "period" } },
    );
    fireEvent.click(add);
    await waitFor(() => expect(prepareSavedSql).toHaveBeenCalledOnce());
    expect(prepareSavedSql.mock.calls[0][0]).toMatchObject({
      arguments: [{ name: "from", variable: "period" }],
      draft,
    });
    expect(prepareSavedSql.mock.calls[0][0].draft.variables).toEqual([period]);
  });

  it("reports an incompatible selected Saved SQL locally without committing a component", async () => {
    const onCommit = vi.fn();
    const prepareSavedSql = vi.fn(async () => ({ scalars: [] }));
    renderPicker({
      kind: "metric",
      provider: { resolveForm: vi.fn(), prepareSavedSql },
      savedSql: [{
        entry_id: "no-metric",
        revision_id: "exact",
        name: "Text only",
        arguments: [],
      }],
      onCommit,
    });
    const dialog = screen.getByRole("dialog", { name: "Metric" });
    fireEvent.click(within(dialog).getByRole("tab", { name: "Saved SQL" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Text only" }));
    await waitFor(() =>
      expect(within(dialog).getByRole("alert")).toHaveTextContent(
        "No supported single-value result is available.",
      )
    );
    expect(onCommit).not.toHaveBeenCalled();
    expect(prepareSavedSql).toHaveBeenCalledOnce();
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
