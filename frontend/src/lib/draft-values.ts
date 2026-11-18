import {
  hasDuplicateAssetReferences,
  isAssetReference,
  isAssetReferenceListField,
} from "~/lib/asset-reference";
import type { AssetReference, Form, StructuredEntryFields } from "~/lib/types";

const ZONED_TIMESTAMP_TYPES = new Set(["timestamp_tz", "timestamp_tz_ns"]);
const LOCAL_DATETIME_PATTERN =
  /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2})(?::(\d{2})(\.\d+)?)?$/;

const pad = (value: number) => String(value).padStart(2, "0");

const addBrowserTimezoneOffset = (value: string): string => {
  const match = LOCAL_DATETIME_PATTERN.exec(value);
  if (!match) return value;

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  const absoluteOffset = Math.abs(offsetMinutes);
  const offset = `${sign}${pad(Math.floor(absoluteOffset / 60))}:${
    pad(
      absoluteOffset % 60,
    )
  }`;
  const seconds = match[2] ?? "00";
  const fraction = match[3] ?? "";
  return `${match[1]}:${seconds}${fraction}${offset}`;
};

/**
 * Preserve a browser datetime-local control's local offset for timezone-aware
 * fields. Rust remains the authority for timestamp validation/normalization.
 */
const withBrowserTimezoneOffset = (
  field: Form["fields"][string],
  value: string,
): string => {
  if (!ZONED_TIMESTAMP_TYPES.has(field.type)) return value;
  return addBrowserTimezoneOffset(value);
};

/** Display a draft value in a text control without deciding semantics. */
export const draftValueToDisplayString = (value: unknown): string => {
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

const isBlankString = (value: unknown): boolean =>
  typeof value === "string" && value.trim() === "";

/**
 * Plain number lists: explicit integer/long/float/double item types.
 */
export const isPlainNumberListField = (field: {
  type?: string;
  items?: { type: string } | undefined;
}): boolean => {
  if (field.type !== "list") return false;
  const itemType = field.items?.type;
  return itemType === "integer" || itemType === "long" ||
    itemType === "float" || itemType === "double";
};

/**
 * Plain boolean lists: explicit boolean item type.
 */
export const isBooleanListField = (field: {
  type?: string;
  items?: { type: string } | undefined;
}): boolean => {
  return field.type === "list" && field.items?.type === "boolean";
};

/**
 * Plain string lists: untyped lists (string items per the domain) or an
 * explicit string item type. Other item kinds keep their current editors
 * until their typed passes land.
 */
export const isPlainStringListField = (field: {
  type?: string;
  items?: { type: string } | undefined;
}): boolean => {
  if (field.type !== "list") return false;
  const itemType = field.items?.type;
  return itemType === undefined || itemType === "string";
};

export type AssetReferenceReadIssue = "invalid" | "duplicate";

export type AssetReferenceReadResult = {
  references: AssetReference[];
  issue?: AssetReferenceReadIssue;
};

const invalidAssetReferences = (): AssetReferenceReadResult => ({
  references: [],
  issue: "invalid",
});

const finishAssetReferenceRead = (
  references: AssetReference[],
): AssetReferenceReadResult => ({
  references,
  ...(hasDuplicateAssetReferences(references) ? { issue: "duplicate" } : {}),
});

/**
 * Read canonical typed references and the JSON-string form accepted by the
 * compatibility bridge. Form-owned callers pass `multiple`; inventory
 * callers omit it to accept either scalar or list properties. Malformed
 * values are reported instead of being silently filtered out.
 */
export const readAssetReferences = (
  value: unknown,
  multiple?: boolean,
): AssetReferenceReadResult => {
  if (value === null || value === undefined || isBlankString(value)) {
    return { references: [] };
  }

  let parsed: unknown = value;
  if (typeof value === "string") {
    try {
      parsed = JSON.parse(value.trim());
    } catch {
      return invalidAssetReferences();
    }
  }

  if (Array.isArray(parsed)) {
    if (multiple === false || !parsed.every(isAssetReference)) {
      return invalidAssetReferences();
    }
    return finishAssetReferenceRead(parsed);
  }

  if (multiple === true || !isAssetReference(parsed)) {
    return invalidAssetReferences();
  }
  return finishAssetReferenceRead([parsed]);
};

const coerceAssetValue = (value: unknown): unknown => {
  // Back-compat: older drafts and persisted sections carry the canonical
  // object as a JSON string. Parse at the transport boundary (the
  // compatibility bridge), never in field components. Invalid values remain
  // intact so the Rust boundary can return its canonical diagnostic.
  const result = readAssetReferences(value, false);
  return result.references[0] ??
    (value === null || value === undefined || isBlankString(value)
      ? undefined
      : value);
};

const coerceAssetListValue = (value: unknown): unknown => {
  const result = readAssetReferences(value, true);
  return result.issue === "invalid"
    ? value
    : result.references.length === 0
    ? undefined
    : result.references;
};

/**
 * Build the transport field map from a typed draft. Values are passed
 * through with kinds preserved; the shared Rust boundary owns coercion and
 * validation. `__*` control keys and blank values are dropped, matching the
 * long-standing builder contract.
 */
export const toTransportFields = (
  formDef: Form,
  values: StructuredEntryFields,
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
    if (
      field && Array.isArray(value) &&
      (isPlainStringListField(field) || isPlainNumberListField(field) ||
        isBooleanListField(field))
    ) {
      // Empty rows in repeated controls are unfilled Work.
      fields[name] = value.filter((item) => !isBlankString(item));
      continue;
    }
    if (typeof value === "string") {
      const trimmed = value.trim();
      if (!trimmed) continue;
      fields[name] = field ? withBrowserTimezoneOffset(field, trimmed) : trimmed;
      continue;
    }
    fields[name] = value;
  }
  return fields;
};

/** True when the value already carries the canonical asset object shape. */
export const isCanonicalAssetValue = (value: unknown): boolean =>
  isAssetReference(value) ||
  (Array.isArray(value) && value.every(isAssetReference));
