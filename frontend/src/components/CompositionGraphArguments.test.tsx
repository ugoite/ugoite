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
import { createSignal } from "solid-js";
import { CompositionGraphStudio } from "./CompositionGraphStudio";
import {
  type CompositionGraphDraft,
  createCompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { buildSqlSchema } from "~/lib/sql";
import { setLocale } from "~/lib/i18n";

vi.mock("~/lib/composition-api", () => ({
  compositionApi: {
    canonicalizeDocument: async (document: unknown) => ({
      canonical_yaml: JSON.stringify(document),
    }),
    evaluate: async (_space: string, request: { evaluation_id: string }) => ({
      evaluation_id: request.evaluation_id,
      ok: true,
      query_results: {},
      computed_query_ids: [],
      queries: [],
      variable_values: {},
      component_bindings: [],
      dependencies: {},
      authorization_fingerprint: "auth",
      publication_fingerprint: "pub",
    }),
  },
}));

describe("Composition shared SQL argument editing", () => {
  beforeEach(() => setLocale("en"));
  afterEach(cleanup);
  const setup = () => {
    const initial = createCompositionGraphDraft("Report");
    initial.variables = [
      {
        id: "threshold",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: 3 },
      },
      {
        id: "caption",
        kind: "constant",
        value_type: { scalar_type: "string", nullable: false },
        value: { kind: "string", value: "hello" },
      },
      {
        id: "rows",
        kind: "query",
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: {
          kind: "inline_sql",
          sql: "SELECT $limit AS value -- $limit remains a comment",
          arguments: [{ name: "limit", variable: "threshold" }],
        },
      },
    ];
    const [draft, setDraft] = createSignal(initial);
    const validity = vi.fn();
    render(() => (
      <CompositionGraphStudio
        draft={draft()}
        forms={[]}
        savedSql={[]}
        provider={{ resolveForm: vi.fn(), prepareSavedSql: vi.fn() }}
        session={{
          session_id: "editor",
          space_id: "space",
          document_revision: null,
          principal_generation: "principal",
        }}
        getPrincipalGeneration={async () => "principal"}
        isSessionCurrent={() => true}
        sqlSchema={buildSqlSchema([])}
        onDraftChange={setDraft}
        onEditorValidityChange={validity}
        onQueryChange={(id, sql) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.map((node) =>
              node.id === id && node.kind === "query"
                ? { ...node, query: { ...node.query, kind: "inline_sql", sql } }
                : node
            ),
          }))}
      />
    ));
    const query = () =>
      draft().variables.find((node) => node.id === "rows") as Extract<
        CompositionGraphDraft["variables"][number],
        { kind: "query" }
      >;
    const open = async () => {
      fireEvent.click(
        screen.getByRole("tab", { name: "Variables", exact: true }),
      );
      fireEvent.click(
        screen.getByRole("button", { name: "rows", exact: true }),
      );
      const action = await screen.findByRole("button", {
        name: "Arguments",
        exact: true,
      });
      await waitFor(() => expect(action).toBeEnabled());
      fireEvent.click(action);
      return screen.getByRole("dialog", { name: "Arguments", exact: true });
    };
    return { draft, setDraft, query, open, validity };
  };

  it("renames through Rust while preserving bindings and comment lookalikes", async () => {
    const { query, open } = setup();
    const dialog = await open();
    expect(within(dialog).queryByRole("checkbox")).toBeNull();
    fireEvent.input(within(dialog).getByLabelText("Argument name for limit"), {
      target: { value: "minimum" },
    });
    const rename = within(dialog).getByRole("button", {
      name: "Rename argument",
    });
    rename.focus();
    fireEvent.click(rename);
    await waitFor(() =>
      expect(query().query.arguments).toEqual([{
        name: "minimum",
        variable: "threshold",
      }])
    );
    expect(query().query).toMatchObject({
      sql: "SELECT $minimum AS value -- $limit remains a comment",
    });
    await waitFor(() =>
      expect(dialog).toContainElement(document.activeElement as HTMLElement)
    );
  });

  it("disconnects an incompatible type and requires an explicit matching binding", async () => {
    const { draft, query, open, validity } = setup();
    const dialog = await open();
    within(dialog).getByLabelText("Type for limit").focus();
    fireEvent.change(within(dialog).getByLabelText("Type for limit"), {
      target: { value: "string" },
    });
    await waitFor(() => expect(query().query.arguments[0].variable).toBe(""));
    await waitFor(() =>
      expect(dialog).toContainElement(document.activeElement as HTMLElement)
    );
    expect(draft().variables[0]).toMatchObject({
      value_type: { scalar_type: "integer" },
    });
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(dialog).not.toBeInTheDocument();
    const binding = screen.getByRole("combobox", {
      name: "limit",
      exact: true,
    });
    expect(within(binding).queryByRole("option", { name: "threshold" }))
      .toBeNull();
    expect(within(binding).getByRole("option", { name: "caption" }))
      .toBeInTheDocument();
    await waitFor(() =>
      expect(validity).toHaveBeenLastCalledWith("rows", false)
    );
    fireEvent.change(binding, { target: { value: "caption" } });
    await waitFor(() =>
      expect(validity).toHaveBeenLastCalledWith("rows", true)
    );
    expect(query().query.arguments[0].variable).toBe("caption");
  });

  it("removes a declaration only after its SQL use is removed and does not restore old Work", async () => {
    const { query, setDraft, open } = setup();
    const dialog = await open();
    expect(within(dialog).getByRole("button", { name: "Remove limit" }))
      .toBeDisabled();
    setDraft((current) => ({
      ...current,
      variables: current.variables.map((node) =>
        node.id === "rows" && node.kind === "query"
          ? {
            ...node,
            query: {
              ...node.query,
              kind: "inline_sql",
              sql: "SELECT 1 -- $limit is only a comment",
            },
          }
          : node
      ),
    }));
    await waitFor(() =>
      expect(within(dialog).getByRole("button", { name: "Remove limit" }))
        .toBeEnabled()
    );
    const remove = within(dialog).getByRole("button", { name: "Remove limit" });
    remove.focus();
    fireEvent.click(remove);
    await waitFor(() => expect(query().query.arguments).toEqual([]));
    await waitFor(() =>
      expect(within(dialog).getByLabelText("Argument name")).toBeEnabled()
    );
    await waitFor(() =>
      expect(dialog).toContainElement(document.activeElement as HTMLElement)
    );
    fireEvent.input(within(dialog).getByLabelText("Argument name"), {
      target: { value: "limit" },
    });
    fireEvent.change(within(dialog).getByLabelText("Type", { exact: true }), {
      target: { value: "date" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Add argument" }),
    );
    await waitFor(() =>
      expect(query().query.arguments).toEqual([{ name: "limit", variable: "" }])
    );
    expect(within(dialog).getByLabelText("Type for limit")).toHaveValue("date");
  });
  it("discards a rename if the SQL changes before Rust completes", async () => {
    const { query, setDraft, open } = setup();
    const dialog = await open();
    fireEvent.input(within(dialog).getByLabelText("Argument name for limit"), {
      target: { value: "obsolete" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Rename argument" }),
    );
    setDraft((current) => ({
      ...current,
      variables: current.variables.map((node) =>
        node.id === "rows" && node.kind === "query"
          ? {
            ...node,
            query: {
              ...node.query,
              kind: "inline_sql",
              sql: "SELECT $limit + 2 AS value",
            },
          }
          : node
      ),
    }));
    await waitFor(() =>
      expect(within(dialog).getByRole("alert")).toBeVisible()
    );
    expect(query().query).toMatchObject({
      sql: "SELECT $limit + 2 AS value",
      arguments: [{ name: "limit", variable: "threshold" }],
    });
  });
  it("declares a directly typed parameter without deleting it from SQL first", async () => {
    const { query, setDraft, open, validity } = setup();
    const dialog = await open();
    setDraft((current) => ({
      ...current,
      variables: current.variables.map((node) =>
        node.id === "rows" && node.kind === "query"
          ? {
            ...node,
            query: {
              ...node.query,
              kind: "inline_sql",
              sql: "SELECT $fresh AS value",
            },
          }
          : node
      ),
    }));
    await waitFor(() =>
      expect(within(dialog).getByLabelText("Argument name")).toBeEnabled()
    );
    fireEvent.input(within(dialog).getByLabelText("Argument name"), {
      target: { value: "fresh" },
    });
    fireEvent.change(within(dialog).getByLabelText("Type", { exact: true }), {
      target: { value: "integer" },
    });
    const add = within(dialog).getByRole("button", { name: "Add argument" });
    expect(add).toBeEnabled();
    fireEvent.click(add);
    await waitFor(() =>
      expect(query().query.arguments.some((entry) => entry.name === "fresh"))
        .toBe(true)
    );
    await waitFor(() =>
      expect(within(dialog).getByRole("button", { name: "Remove limit" }))
        .toBeEnabled()
    );
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Remove limit" }),
    );
    await waitFor(() =>
      expect(query().query.arguments).toEqual([{ name: "fresh", variable: "" }])
    );
    fireEvent.click(
      within(dialog).getAllByRole("button", { name: "Close" })[0],
    );
    fireEvent.change(
      screen.getByRole("combobox", { name: "fresh", exact: true }),
      { target: { value: "threshold" } },
    );
    await waitFor(() =>
      expect(validity).toHaveBeenLastCalledWith("rows", true)
    );
    expect(query().query).toMatchObject({
      sql: "SELECT $fresh AS value",
      arguments: [{ name: "fresh", variable: "threshold" }],
    });
  });
  it("returns focus after Composition Form selection while SQL validity reparses", async () => {
    const initial = createCompositionGraphDraft("Report");
    initial.variables = [{
      id: "rows",
      kind: "query",
      returns: {
        kind: "scalar",
        value_type: { scalar_type: "integer", nullable: false },
      },
      query: { kind: "inline_sql", sql: "SELECT 1", arguments: [] },
    }];
    const [draft, setDraft] = createSignal(initial);
    const form = {
      id: "f",
      name: "Expenses",
      version: 1,
      template: "# Expenses",
      sql_relation: "form_123",
      fields: { Amount: { type: "float", sql_column: "amount" } },
    };
    render(() => (
      <CompositionGraphStudio
        draft={draft()}
        forms={[]}
        savedSql={[]}
        provider={{ resolveForm: vi.fn(), prepareSavedSql: vi.fn() }}
        session={{
          session_id: "s",
          space_id: "space",
          document_revision: null,
          principal_generation: "p",
        }}
        getPrincipalGeneration={async () => "p"}
        isSessionCurrent={() => true}
        sqlSchema={buildSqlSchema([form])}
        sqlForms={[form]}
        onDraftChange={setDraft}
        onQueryChange={(id, sql) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.map((node) =>
              node.id === id && node.kind === "query"
                ? { ...node, query: { ...node.query, kind: "inline_sql", sql } }
                : node
            ),
          }))}
      />
    ));
    fireEvent.click(
      screen.getByRole("tab", { name: "Variables", exact: true }),
    );
    fireEvent.click(screen.getByRole("button", { name: "rows", exact: true }));
    const opener = await screen.findByRole("button", {
      name: "Form",
      exact: true,
    });
    await waitFor(() => expect(opener).toBeEnabled());
    opener.focus();
    fireEvent.click(opener);
    const choice = await screen.findByRole("button", {
      name: "Expenses",
      exact: true,
    });
    await waitFor(() => expect(choice).toBeEnabled());
    choice.focus();
    fireEvent.click(choice);
    await waitFor(() =>
      expect(draft().variables[0]).toMatchObject({
        query: { sql: 'SELECT 1 FROM "form_123"' },
      })
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    await waitFor(() => expect(opener).toHaveFocus());
  });
});
