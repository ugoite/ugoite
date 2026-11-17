import "@testing-library/jest-dom/vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

vi.mock("./SqlQueryEditor", () => ({
  SqlQueryEditor: (props: {
    ariaLabel?: string;
    value: string;
    onChange: (value: string) => void;
  }) => (
    <div
      class="cm-content"
      role="textbox"
      aria-label={props.ariaLabel}
      contentEditable={true}
      tabIndex={0}
      onInput={(event) => props.onChange(event.currentTarget.textContent ?? "")}
    >
      {props.value}
    </div>
  ),
}));

describe("Composition inline SQL argument bindings", () => {
  beforeEach(() => setLocale("en"));
  afterEach(cleanup);

  const makeDraft = (sql: string, bound = true) => {
    const draft = createCompositionGraphDraft("Report");
    draft.variables = [
      {
        id: "threshold",
        kind: "constant",
        value_type: { scalar_type: "integer", nullable: false },
        value: { kind: "integer", value: "3" },
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
          sql,
          arguments: bound ? [{ name: "limit", variable: "threshold" }] : [],
        },
      },
    ];
    return draft;
  };

  const renderStudio = (initial: CompositionGraphDraft) => {
    const [draft, setDraft] = createSignal(initial);
    render(() => (
      <CompositionGraphStudio
        spaceId="space"
        draft={draft()}
        getPrincipalGeneration={async () => "principal"}
        sqlSchema={buildSqlSchema([])}
        onDraftChange={setDraft}
        onQueryChange={(id, sql, formBindings) =>
          setDraft((current) => ({
            ...current,
            variables: current.variables.map((node) =>
              node.id === id && node.kind === "query" &&
                node.query.kind === "inline_sql"
                ? {
                  ...node,
                  query: {
                    ...node.query,
                    sql,
                    ...(formBindings
                      ? {
                        form_bindings: formBindings.map((binding) => ({
                          relation: binding.relation,
                          form_id: binding.form_id,
                        })),
                      }
                      : {}),
                  },
                }
                : node
            ),
          }))}
      />
    ));
    return draft;
  };

  const openQuery = async (id = "rows") => {
    fireEvent.click(
      screen.getByRole("tab", { name: "Variables" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: id }),
    );
    await screen.findByRole("textbox", { name: "SQL" });
  };

  it("binds each Rust-parsed marker beside the text editor without structure dialogs", async () => {
    const draft = renderStudio(
      makeDraft("SELECT {{limit}} AS value -- {{ignored}}", true),
    );
    await openQuery();

    expect(
      await screen.findByRole(
        "combobox",
        { name: "Choose a variable for limit" },
        { timeout: 10000 },
      ),
    ).toHaveValue("threshold");
    expect(screen.queryByRole("button", { name: "Structure" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Arguments" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Columns" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Filter" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Sort" })).toBeNull();
    expect(
      screen.queryByRole("combobox", { name: "Choose a variable for ignored" }),
    )
      .toBeNull();

    fireEvent.change(
      screen.getByRole("combobox", { name: "Choose a variable for limit" }),
      { target: { value: "caption" } },
    );
    await waitFor(() =>
      expect(draft().variables.find((node) => node.id === "rows"))
        .toMatchObject({
          query: {
            arguments: [{ name: "limit", variable: "caption" }],
          },
        })
    );
  });

  it("creates a unique suggested variable and binds it atomically from the SQL marker", async () => {
    const initial = makeDraft("SELECT {{limit}} AS value", false);
    initial.variables.push({
      id: "limit",
      kind: "constant",
      value_type: { scalar_type: "json", nullable: false },
      value: { kind: "json", value: { existing: true } },
    });
    const draft = renderStudio(initial);
    await openQuery();

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Create a variable for limit",
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Value" }));
    const name = screen.getByRole("textbox", { name: "Variable name" });
    await waitFor(() => expect(name).toHaveValue("limit_2"));
    fireEvent.submit(name.closest("form")!);

    await waitFor(() =>
      expect(draft().variables).toContainEqual(
        expect.objectContaining({ id: "limit_2", kind: "constant" }),
      )
    );
    expect(draft().variables.find((node) => node.id === "rows"))
      .toMatchObject({
        query: { arguments: [{ name: "limit", variable: "limit_2" }] },
      });
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "SQL" }))
        .toHaveFocus()
    );
  });

  it("suggests a unique name for variables created from Design", async () => {
    const initial = createCompositionGraphDraft("Report");
    initial.variables = [{
      id: "input",
      kind: "input",
      value_type: { scalar_type: "string", nullable: false },
      required: true,
    }];
    renderStudio(initial);

    fireEvent.click(
      screen.getByRole("button", { name: "Create Input" }),
    );
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Variable name" }))
        .toHaveValue("input_2")
    );
  });

  it("adds and cancels a Design input in place with matching graph references", async () => {
    const draft = renderStudio(createCompositionGraphDraft("Report"));

    fireEvent.click(
      screen.getByRole("button", { name: "Input" }),
    );
    const name = screen.getByRole("textbox", { name: "Variable name" });
    await waitFor(() => expect(name).toHaveValue("input"));
    fireEvent.click(
      screen.getByRole("button", { name: "Cancel" }),
    );
    expect(draft().variables).toHaveLength(0);
    expect(draft().components).toHaveLength(0);

    fireEvent.click(
      screen.getByRole("button", { name: "Input" }),
    );
    const nextName = screen.getByRole("textbox", { name: "Variable name" });
    fireEvent.input(nextName, { target: { value: "customer_id" } });
    fireEvent.submit(nextName.closest("form")!);

    await waitFor(() =>
      expect(draft().components).toContainEqual(
        expect.objectContaining({ kind: "input", variable: "customer_id" }),
      )
    );
    expect(draft().variables).toContainEqual({
      id: "customer_id",
      kind: "input",
      value_type: { scalar_type: "string", nullable: false },
      required: true,
    });
    expect(screen.getByRole("tab", { name: "Design" }))
      .toHaveAttribute("aria-selected", "true");
  });
});
