import {
  parseEntryMarkdownCompat,
  renderEntryMarkdownCompat,
} from "~/lib/ugoite-client/protocol";
import { type DraftValue, draftValueToDisplayString } from "~/lib/draft-values";
import { toRustFormDefinition } from "~/lib/entry-validation";
import type { Form } from "~/lib/types";

export type CompatDraft = {
  title: string;
  tags: string[];
  fields: Record<string, string>;
};

const toStringFields = (
  fields: Record<string, unknown>,
): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const [name, value] of Object.entries(fields)) {
    out[name] = draftValueToDisplayString(value as DraftValue);
  }
  return out;
};

/**
 * Parse source-view Markdown into the structured draft via the Rust bridge.
 * The Rust parser is the semantic authority; callers keep the textarea
 * responsive by setting it synchronously and applying this result async.
 */
export const parseSourceToDraftViaWasm = async (
  markdown: string,
  fallbackTitle: string,
): Promise<CompatDraft> => {
  const parsed = await parseEntryMarkdownCompat(markdown, fallbackTitle);
  return {
    title: parsed.title,
    tags: Array.isArray(parsed.tags) ? parsed.tags : [],
    fields: toStringFields(
      (parsed.fields ?? {}) as Record<string, unknown>,
    ),
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
  fields: Record<string, unknown>,
): Promise<string> => {
  const rendered = await renderEntryMarkdownCompat(
    toRustFormDefinition(form),
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
