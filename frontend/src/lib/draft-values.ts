import {
  isAssetReference,
  isAssetReferenceListField,
  parseAssetReference,
  parseAssetReferenceList,
} from "~/lib/asset-reference";
import { normalizeEntryFieldValue } from "~/lib/entry-input";
import type { AssetReference, Form } from "~/lib/types";

/**
 * Typed structured-draft value (Lane 1 PR6).
 *
 * Text controls keep editing in strings, but the draft map distinguishes
 * reference/collection kinds so object/list/reference values are never
 * degraded to JSON strings or Markdown fragments inside the draft:
 * - scalar string/boolean/numeric stay scalar
 * - list stays a string array (Markdown-list text is only the textarea
 *   presentation; transport keeps the typed form when already parsed)
 * - object_list stays an object array
 * - row_reference stays the stable Entry ID string (display title lives in
 *   the picker, never in the saved value)
 * - asset_reference stays the canonical AssetReference object
 * - asset_reference_list stays an AssetReference array
 */
export type DraftValue =
  | string
  | boolean
  | number
  | string[]
  | Record<string, unknown>[]
  | AssetReference
  | AssetReference[]
  | null
  | undefined;

export type DraftFields = Record<string, DraftValue>;

/** Display a draft value in a text control without deciding semantics. */
export const draftValueToDisplayString = (value: DraftValue): string => {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  if (typeof value === "boolean" || typeof value === "number") {
    return String(value);
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return "";
    if (value.every((item): item is string => typeof item === "string")) {
      return value.map((item) => `- ${item}`).join("\n");
    }
    try {
      return JSON.stringify(value);
    } catch {
      return "";
    }
  }
  try {
    return JSON.stringify(value);
  } catch {
    return "";
  }
};

const isBlankString = (value: unknown): value is string =>
  typeof value === "string" && value.trim() === "";

const coerceAssetValue = (value: DraftValue): unknown => {
  if (typeof value === "string") {
    // Back-compat: older drafts and persisted sections carry the canonical
    // object as a JSON string. Parse at the transport boundary (the
    // compatibility bridge), never in field components.
    if (!value.trim()) return undefined;
    return parseAssetReference(value) ?? value;
  }
  if (value === null || value === undefined) return undefined;
  return value;
};

const coerceAssetListValue = (value: DraftValue): unknown => {
  if (typeof value === "string") {
    if (!value.trim()) return undefined;
    return parseAssetReferenceList(value) ?? value;
  }
  if (value === null || value === undefined) return undefined;
  return value;
};

/**
 * Build the transport field map from a typed draft. Values are passed
 * through with kinds preserved; the shared Rust boundary owns coercion and
 * validation. `__*` control keys and blank values are dropped, matching the
 * long-standing builder contract.
 */
export const toTransportFields = (
  formDef: Form,
  values: DraftFields,
): Record<string, unknown> => {
  const fields: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(values)) {
    if (name.startsWith("__")) continue;
    if (value === null || value === undefined) continue;
    if (isBlankString(value)) continue;
    const field = formDef.fields?.[name];
    // Asset kinds keep canonical objects at the transport boundary. Legacy
    // JSON strings parse here (the compatibility bridge), never in field
    // components. Unparseable strings fall through so Rust reports the
    // canonical field diagnostic.
    if (field?.type === "asset_reference") {
      const coerced = coerceAssetValue(value);
      if (coerced === undefined) continue;
      fields[name] = coerced;
      continue;
    }
    if (field && isAssetReferenceListField(field)) {
      const coerced = coerceAssetListValue(value);
      if (coerced === undefined) continue;
      if (Array.isArray(coerced) && coerced.length === 0) continue;
      fields[name] = coerced;
      continue;
    }
    if (typeof value === "string") {
      const trimmed = value.trim();
      if (!trimmed) continue;
      fields[name] = field ? normalizeEntryFieldValue(field, trimmed) : trimmed;
      continue;
    }
    fields[name] = value;
  }
  return fields;
};

/** True when the value already carries the canonical asset object shape. */
export const isCanonicalAssetValue = (value: DraftValue): boolean =>
  isAssetReference(value) ||
  (Array.isArray(value) && value.every(isAssetReference));
