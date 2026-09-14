import {
  parseEntryMarkdownCompat,
  renderEntryMarkdownCompat,
} from "~/lib/ugoite-client/protocol";
import type { DraftFields } from "~/lib/draft-values";
import { toRustFormDefinition } from "~/lib/entry-validation";
import type { Form } from "~/lib/types";
import type { MarkdownConversionDiagnostic } from "~/lib/ugoite-client/protocol";

export type CompatDraft = {
  title: string;
  tags: string[];
  fields: DraftFields;
  diagnostics: MarkdownConversionDiagnostic[];
};

/**
 * Parse source-view Markdown into the structured draft via the Rust bridge.
 * The Rust parser is the semantic authority; callers keep the textarea
 * responsive by setting it synchronously and applying this result async.
 */
export const parseSourceToDraftViaWasm = async (
  markdown: string,
  fallbackTitle: string,
  options: { strict?: boolean } = {},
): Promise<CompatDraft> => {
  const parsed = await parseEntryMarkdownCompat(
    markdown,
    fallbackTitle,
    options,
  );
  return {
    title: parsed.title,
    tags: Array.isArray(parsed.tags) ? parsed.tags : [],
    fields: (parsed.fields ?? {}) as DraftFields,
    diagnostics: Array.isArray(parsed.diagnostics) ? parsed.diagnostics : [],
  };
};

/**
 * Render a structured draft to source-view Markdown via the Rust bridge.
 * Field order and frontmatter encoding follow the canonical 0.1
 * representation. Formless notes have no Rust form scope and keep the
 * TypeScript compatibility builder (handled by callers).
 */
export const renderDraftToSourceViaWasm = async (
  form: Form,
  title: string,
  tags: string[],
  fields: DraftFields,
  knownForms: readonly Form[] = [],
): Promise<string> => {
  const rendered = await renderEntryMarkdownCompat(
    toRustFormDefinition(form, knownForms),
    {
      title,
      form_name: form.name,
      tags,
      fields,
      extra_attributes: {},
    },
  );
  return rendered.markdown;
};
