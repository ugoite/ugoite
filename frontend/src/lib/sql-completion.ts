import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionSource,
} from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import {
  keywordCompletionSource,
  schemaCompletionSource,
  StandardSQL,
} from "@codemirror/lang-sql";
import { type SqlSchema, sqlSchemaFormLabelsFor } from "./sql";

const NON_CODE_NODES = new Set([
  "String",
  "QuotedIdentifier",
  "LineComment",
  "BlockComment",
]);

const SQL_AGGREGATE_FUNCTIONS = ["AVG", "COUNT", "MAX", "MIN", "SUM"];

const aggregateCompletions: Completion[] = SQL_AGGREGATE_FUNCTIONS.map(
  (name) => ({
    label: name,
    type: "function",
    apply(view, _completion, from, to) {
      const hasOpeningParenthesis = view.state.doc.sliceString(to, to + 1) ===
        "(";
      const insert = hasOpeningParenthesis ? name : `${name}()`;
      view.dispatch({
        changes: { from, to, insert },
        selection: {
          anchor: from + name.length + (hasOpeningParenthesis ? 0 : 1),
        },
      });
    },
  }),
);

export function sqlCompletionSource(schema: SqlSchema): CompletionSource {
  const schemaSource = schemaCompletionSource({ schema });
  const keywords = keywordCompletionSource(StandardSQL);
  const names = Object.keys(schema.tables ?? {});
  const tableLabels = sqlSchemaFormLabelsFor(schema);
  const quoteIdentifier = (name: string) => `"${name.replaceAll('"', '""')}"`;
  const withTableLabels = (result: ReturnType<typeof schemaSource>) => {
    if (!result) return result;
    return {
      ...result,
      options: result.options.map((option) => {
        if (!Object.hasOwn(tableLabels, option.label)) return option;
        const relation = option.label;
        return {
          ...option,
          label: tableLabels[relation],
          apply: quoteIdentifier(relation),
        };
      }),
    };
  };

  return (context: CompletionContext) => {
    const node = syntaxTree(context.state).resolve(context.pos, -1);
    if (node.name === "QuotedIdentifier") {
      return withTableLabels(schemaSource(context));
    }
    if (NON_CODE_NODES.has(node.name)) return null;

    const beforeCursor = context.state.sliceDoc(0, context.pos);
    if (
      names.length > 0 &&
      /\b(?:FROM|JOIN)\s+[A-Za-z0-9_-]*$/i.test(beforeCursor)
    ) {
      const token = context.matchBefore(/[A-Za-z0-9_-]*$/);
      return {
        from: token?.from ?? context.pos,
        options: names.map((name) => ({
          label: tableLabels[name] ?? name,
          type: "type",
          apply: quoteIdentifier(name),
        })),
        validFor: /^[A-Za-z0-9_-]*$/,
      };
    }

    const existingCompletions = schemaSource(context) ?? keywords(context);
    const functionToken = context.matchBefore(/[A-Za-z_][A-Za-z0-9_]*$/);
    if (!functionToken) return existingCompletions;

    const matchingAggregates = aggregateCompletions.filter((completion) =>
      completion.label.toUpperCase().startsWith(
        functionToken.text.toUpperCase(),
      )
    );
    if (matchingAggregates.length === 0) return existingCompletions;

    const existingOptions = existingCompletions?.from === functionToken.from
      ? existingCompletions.options
      : [];
    const labels = new Set(
      matchingAggregates.map((completion) => completion.label),
    );
    return {
      from: functionToken.from,
      options: [
        ...matchingAggregates,
        ...existingOptions.filter((completion) =>
          !labels.has(completion.label)
        ),
      ],
      validFor: /^[A-Za-z_][A-Za-z0-9_]*$/,
    };
  };
}

export function sqlEditorAutocompletion(schema: SqlSchema) {
  return autocompletion({ override: [sqlCompletionSource(schema)] });
}
