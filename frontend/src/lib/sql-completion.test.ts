import { CompletionContext } from "@codemirror/autocomplete";
import { sql } from "@codemirror/lang-sql";
import { EditorState } from "@codemirror/state";
import { buildSqlSchema } from "./sql";
import { sqlCompletionSource } from "./sql-completion";

describe("SQL Form completion", () => {
  it("completes common aggregate functions while editing SQL", () => {
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
      const result = sqlCompletionSource(schema)(context);

      expect(result?.options.map(({ label }) => label), prefix).toContain(name);
      expect(result?.options.find(({ label }) => label === name)?.type)
        .toBe("function");
    }
  });

  it("inserts aggregate parentheses with the cursor inside", () => {
    const schema = buildSqlSchema([]);
    const query = "SELECT CO";
    const state = EditorState.create({
      doc: query,
      extensions: [sql({ schema })],
    });
    const context = new CompletionContext(state, query.length, true);
    const option = sqlCompletionSource(schema)(context)?.options.find((item) =>
      item.label === "COUNT"
    );
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

  it("shows Form names and inserts them as quoted identifiers", () => {
    const schema = buildSqlSchema([
      {
        name: "1-Expense",
        version: 1,
        template: "",
        fields: {},
      },
    ]);
    const query = "SELECT * FROM 1";
    const state = EditorState.create({
      doc: query,
      extensions: [sql({ schema })],
    });
    const context = new CompletionContext(state, query.length, true);
    const result = sqlCompletionSource(schema)(context);

    expect(result?.options).toContainEqual({
      label: "1-Expense",
      type: "type",
      apply: '"1-Expense"',
    });
  });

  it("completes __proto__ as an ordinary Form name", () => {
    const schema = buildSqlSchema([
      {
        name: "__proto__",
        version: 1,
        template: "",
        fields: {},
      },
    ]);
    const query = "SELECT * FROM __";
    const state = EditorState.create({
      doc: query,
      extensions: [sql({ schema })],
    });
    const context = new CompletionContext(state, query.length, true);
    const result = sqlCompletionSource(schema)(context);

    expect(result?.options).toContainEqual({
      label: "__proto__",
      type: "type",
      apply: '"__proto__"',
    });
  });
});
