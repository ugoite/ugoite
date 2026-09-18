import {
  hasDuplicateAssetReferences,
  isAssetReference,
  isAssetReferenceListField,
} from "~/lib/asset-reference";
import type { AssetReference, Form } from "~/lib/types";

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
export const normalizeEntryFieldValue = (
  field: Form["fields"][string],
  value: string,
): string => {
  if (!ZONED_TIMESTAMP_TYPES.has(field.type)) return value;
  return addBrowserTimezoneOffset(value);
};

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

const isBlankString = (value: unknown): boolean =>
  typeof value === "string" && value.trim() === "";

/**
 * Normalize any stored draft shape into the object array a repeated
 * object-list editor binds: object arrays stay as-is, legacy JSON text
 * parses when it holds an array of objects, everything else starts empty.
 * The shared Rust boundary remains the authority for item validity.
 */
export const normalizeObjectListValue = (
  value: DraftValue,
): Record<string, unknown>[] => {
  if (Array.isArray(value)) {
    return (value as unknown[]).filter(
      (item): item is Record<string, unknown> =>
        typeof item === "object" && item !== null && !Array.isArray(item),
    );
  }
  if (typeof value === "string") {
    try {
      const parsed: unknown = JSON.parse(value.trim());
      if (Array.isArray(parsed)) return normalizeObjectListValue(parsed);
    } catch {
      // Fall through to empty: unparseable text stays readable through the
      // legacy textarea surfaces until edited, never invented here.
    }
  }
  return [];
};

const LIST_ITEM_PREFIX_PATTERN = /^(?:[-*+](?:\s+\[[ xX]\])?\s*)/;

/**
 * Split Markdown-list presentation back into items. Approximates the shared
 * Rust list coercion (strip `-`/`*`/`+` bullets with optional checkboxes,
 * skip empties) so a repeated list editor and the legacy textarea read the
 * same stored shape. Edge cases differ deliberately: bare markers without
 * trailing content count as empty here, and remainders are trimmed. The
 * editor only emits typed arrays, so this path serves legacy text until it
 * is edited, never new input.
 */
export const parseMarkdownStringList = (text: string): string[] => {
  const items: string[] = [];
  for (const line of text.split(/\r?\n/)) {
    const item = line.trim().replace(LIST_ITEM_PREFIX_PATTERN, "").trim();
    if (item) items.push(item);
  }
  return items;
};

/**
 * Normalize any stored draft shape into the string array a repeated list
 * editor binds: typed arrays stay as-is, legacy Markdown-list text parses,
 * everything else starts empty.
 */
export const normalizeStringListValue = (value: DraftValue): string[] => {
  if (Array.isArray(value)) {
    return (value as unknown[]).filter((item): item is string =>
      typeof item === "string"
    );
  }
  if (typeof value === "string") return parseMarkdownStringList(value);
  return [];
};

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
 * Normalize any stored shape into the boolean array a repeated list
 * editor binds. Typed arrays stay; legacy text lines map through the
 * shared boolean aliases with unparseable lines kept raw so Rust reports
 * the canonical diagnostic.
 */
const BOOLEAN_TRUE_ALIASES = new Set(["true", "yes", "on", "1"]);
const BOOLEAN_FALSE_ALIASES = new Set(["false", "no", "off", "0"]);

export const parseBooleanAlias = (text: string): boolean | undefined => {
  const normalized = text.trim().toLowerCase();
  if (BOOLEAN_TRUE_ALIASES.has(normalized)) return true;
  if (BOOLEAN_FALSE_ALIASES.has(normalized)) return false;
  return undefined;
};

export const normalizeBooleanListValue = (
  value: DraftValue,
): Array<boolean | string | number> => {
  if (Array.isArray(value)) {
    // Numbers pass through for Rust to judge (it accepts 1/0 as booleans).
    return (value as unknown[]).filter(
      (item): item is boolean | string | number =>
        typeof item === "boolean" || typeof item === "string" ||
        typeof item === "number",
    );
  }
  if (typeof value === "string") {
    return value
      .split(/\r?\n/)
      .map((line) => line.trim().replace(LIST_ITEM_PREFIX_PATTERN, "").trim())
      .filter((line) => line !== "")
      .map((line) => parseBooleanAlias(line) ?? line);
  }
  return [];
};

/**
 * Normalize any stored shape into the number array a repeated list editor
 * binds. Finite numerics stay numeric; other text stays raw for Rust to
 * judge, matching the scalar number inputs.
 */
export const normalizeNumberListValue = (
  value: DraftValue,
): Array<number | string> => {
  if (Array.isArray(value)) {
    return (value as unknown[]).filter(
      (item): item is number | string =>
        typeof item === "number" || typeof item === "string",
    );
  }
  if (typeof value === "string") {
    return value
      .split(/\r?\n/)
      .map((line) => line.trim().replace(LIST_ITEM_PREFIX_PATTERN, "").trim())
      .filter((line) => line !== "")
      .map((line) => {
        const parsed = Number(line);
        return line !== "" && Number.isFinite(parsed) ? parsed : line;
      });
  }
  return [];
};
/**
 * Commit number-item text: finite numerics store numeric, everything else
 * (including blank) stays raw text for Rust to judge, matching the scalar
 * number inputs.
 */
export const parseNumberItemText = (raw: string): number | string => {
  if (raw.trim() === "") return "";
  const parsed = Number(raw);
  return Number.isFinite(parsed) ? parsed : raw;
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

const coerceAssetValue = (value: DraftValue): unknown => {
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

const coerceAssetListValue = (value: DraftValue): unknown => {
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
    if (
      field &&
      (isPlainNumberListField(field) || isBooleanListField(field)) &&
      Array.isArray(value)
    ) {
      // Whitespace-only text never persists as an item; kinds stay mixed
      // for Rust to judge, and emptiness stays meaningful for requiredness.
      fields[name] = value.filter(
        (item) => !(typeof item === "string" && item.trim() === ""),
      );
      continue;
    }
    if (field && isPlainStringListField(field) && Array.isArray(value)) {
      // Blank items never persist: matches the shared Rust coercion that
      // skips empty lines, so an untouched extra row cannot create "" items.
      // An empty array stays meaningful (required-emptiness is decided by
      // the shared boundary, matching the previous pass-through).
      // Non-string members are dropped here; the editor cannot produce
      // them, so this only affects already-corrupted drafts.
      fields[name] = value.filter(
        (item): item is string =>
          typeof item === "string" && item.trim() !== "",
      );
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
