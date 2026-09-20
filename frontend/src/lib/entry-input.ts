import { type DraftFields, toTransportFields } from "~/lib/draft-values";
import type { Form } from "~/lib/types";
export { normalizeEntryFieldValue } from "~/lib/draft-values";

export type EntryInputMode = "webform" | "chat";

/**
 * Build a structured fields map for the `{ form, fields }`
 * payload. Typed values remain typed here; the shared Rust boundary owns
 * coercion and validation. `__*` control keys and blank values are dropped,
 * matching the structured draft boundary. Zoned timestamps get the same
 * browser-offset normalization as the shared Rust path so `datetime-local`
 * controls round-trip.
 */
export const buildStructuredEntryFields = (
  formDef: Form,
  values: DraftFields,
): Record<string, unknown> => toTransportFields(formDef, values);
