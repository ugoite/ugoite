import { toTransportFields } from "~/lib/draft-values";
import type { Form, StructuredEntryFields } from "~/lib/types";

export type EntryInputMode = "webform" | "chat";

/**
 * Build a structured fields map for the `{ form, fields }`
 * payload. Typed values remain typed here; the shared Rust boundary owns
 * coercion and validation. `__*` control keys and blank values are dropped,
 * matching the structured draft boundary. The adapter attaches the browser
 * offset to `datetime-local` values for timezone-aware fields.
 */
export const buildStructuredEntryFields = (
  formDef: Form,
  values: StructuredEntryFields,
): Record<string, unknown> => toTransportFields(formDef, values);
