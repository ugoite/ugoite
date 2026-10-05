import { CompletionContext } from "@codemirror/autocomplete";
import { sql } from "@codemirror/lang-sql";
import { EditorState } from "@codemirror/state";
import { buildSqlSchema } from "./sql";
import { sqlCompletionSource } from "./sql-completion";

describe("SQL Form completion", () => {
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
