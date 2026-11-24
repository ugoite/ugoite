import { CompletionContext } from "@codemirror/autocomplete";
import { sql } from "@codemirror/lang-sql";
import { EditorState } from "@codemirror/state";
import { vi } from "vitest";
import { buildSqlSchema } from "./sql";
import { sqlCompletionSource } from "./sql-completion";

describe("SQL Form completion", () => {
  it("completes a Composition SQL variable marker to one explicit draft value", async () => {
    const schema = buildSqlSchema([]);
    const query = "SELECT {{mo";
    const state = EditorState.create({
      doc: query,
      extensions: [sql({ schema })],
    });
    const onComplete = vi.fn();
    const context = new CompletionContext(state, query.length, true);
    const result = await sqlCompletionSource(schema, {
      names: ["month", "money"],
      onComplete,
    })(context);
    const option = result?.options.find((item) => item.label === "month");
    let dispatched: { changes: { from: number; to: number; insert: string }; selection: { anchor: number } } | undefined;
    const view = {
      state: {
        doc: {
          sliceString: (from: number, to: number) => query.slice(from, to),
        },
      },
      dispatch: (transaction: typeof dispatched) => dispatched = transaction,
    };

    option?.apply?.(view as never, option, 7, query.length);

    expect(result?.options.map(({ label }) => label)).toEqual([
      "month",
      "money",
    ]);
    expect(onComplete).toHaveBeenCalledWith("month", "SELECT {{month}}");
    expect(dispatched?.changes).toEqual({
      from: 7,
      to: query.length,
      insert: "{{month}}",
    });
  });

  it.each([
    ["single-quoted string", "SELECT '{{mo"],
    ["line comment", "SELECT 1 -- {{mo"],
    ["block comment", "SELECT 1 /* {{mo"],
    ["quoted identifier", 'SELECT "{{mo'],
  ])("does not offer marker completion in a %s", async (_label, query) => {
    const schema = buildSqlSchema([]);
    const state = EditorState.create({
      doc: query,
      extensions: [sql({ schema })],
    });
    const context = new CompletionContext(state, query.length, true);
    const result = await sqlCompletionSource(schema, {
      names: ["month"],
    })(context);

    expect(result?.options.some(({ label }) => label === "month") ?? false)
      .toBe(false);
  });

  it("completes common aggregate functions while editing SQL", async () => {
    const schema = buildSqlSchema([]);
    const functions = [
      ["COUNT", "co"],
      ["SUM", "su"],
      ["AVG", "av"],
      ["MIN", "mi"],
      ["MAX", "ma"],
    ];

    for (const [name, prefix] of functions) {
      const query = "SELECT " + prefix;
      const state = EditorState.create({
        doc: query,
        extensions: [sql({ schema })],
      });
      const context = new CompletionContext(state, query.length, true);
      const result = await sqlCompletionSource(schema)(context);

      expect(result?.options.map(({ label }) => label), prefix).toContain(name);
      expect(result?.options.find(({ label }) => label === name)?.type)
        .toBe("function");
    }
  });

  it("inserts aggregate parentheses with the cursor inside", async () => {
    const schema = buildSqlSchema([]);
    const query = "SELECT CO";
    const state = EditorState.create({
      doc: query,
      extensions: [sql({ schema })],
    });
    const context = new CompletionContext(state, query.length, true);
    const result = await sqlCompletionSource(schema)(context);
    const option = result?.options.find((item) => item.label === "COUNT");
    let dispatched: unknown;
    const view = {
      state: {
        doc: {
          sliceString: (from: number, to: number) => query.slice(from, to),
        },
      },
      dispatch: (transaction: unknown) => dispatched = transaction,
    };

    expect(option).toBeDefined();
    option?.apply?.(view as never, option, 7, query.length);

    expect(dispatched).toEqual({
      changes: { from: 7, to: query.length, insert: "COUNT()" },
      selection: { anchor: 13 },
    });
  });

  it("shows Form names and inserts their server-owned SQL relations", async () => {
    const relation = "form_00000000000070008000000000000001";
    const schema = buildSqlSchema([
      {
        name: "1-Expense",
        sql_relation: relation,
        version: 1,
        template: "",
        fields: {},
        allow_extra_attributes: "deny" as const,
      },
    ]);
    const query = "SELECT * FROM 1";
    const state = EditorState.create({
      doc: query,
      extensions: [sql({ schema })],
    });
    const context = new CompletionContext(state, query.length, true);
    const result = await sqlCompletionSource(schema)(context);

    expect(result?.options).toContainEqual({
      label: "1-Expense",
      type: "type",
      apply: `"${relation}"`,
    });
  });

  it("completes __proto__ as an ordinary Form name", async () => {
    const relation = "form_00000000000070008000000000000002";
    const schema = buildSqlSchema([
      {
        name: "__proto__",
        sql_relation: relation,
        version: 1,
        template: "",
        fields: {},
        allow_extra_attributes: "deny" as const,
      },
    ]);
    const query = "SELECT * FROM __";
    const state = EditorState.create({
      doc: query,
      extensions: [sql({ schema })],
    });
    const context = new CompletionContext(state, query.length, true);
    const result = await sqlCompletionSource(schema)(context);

    expect(result?.options).toContainEqual({
      label: "__proto__",
      type: "type",
      apply: `"${relation}"`,
    });
  });
});
