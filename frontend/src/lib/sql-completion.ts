import {
  autocompletion,
  type CompletionContext,
  type CompletionSource,
} from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import {
  keywordCompletionSource,
  schemaCompletionSource,
  StandardSQL,
} from "@codemirror/lang-sql";
import type { SqlSchema } from "./sql";

const NON_CODE_NODES = new Set([
  "String",
  "QuotedIdentifier",
  "LineComment",
  "BlockComment",
]);

export function sqlCompletionSource(schema: SqlSchema): CompletionSource {
  const schemaSource = schemaCompletionSource({ schema });
  const keywords = keywordCompletionSource(StandardSQL);
  const names = Object.keys(schema.tables ?? {});

  return (context: CompletionContext) => {
    const node = syntaxTree(context.state).resolve(context.pos, -1);
    if (node.name === "QuotedIdentifier") return schemaSource(context);
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
          label: name,
          type: "type",
          apply: `"${name.replaceAll('"', '""')}"`,
        })),
        validFor: /^[A-Za-z0-9_-]*$/,
      };
    }

    return schemaSource(context) ?? keywords(context);
  };
}

export function sqlEditorAutocompletion(schema: SqlSchema) {
  return autocompletion({ override: [sqlCompletionSource(schema)] });
}
