import type { AssetReference, FormField } from "./types";

const SHA256_PATTERN = /^[a-f0-9]{64}$/i;

/** Validate the one canonical AssetReference shape at the browser boundary. */
export const isAssetReference = (value: unknown): value is AssetReference => {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const reference = value as Partial<AssetReference>;
  return typeof reference.asset_id === "string" &&
    reference.asset_id.trim().length > 0 &&
    typeof reference.name === "string" && reference.name.trim().length > 0 &&
    typeof reference.media_type === "string" &&
    reference.media_type.trim().length > 0 &&
    typeof reference.size_bytes === "number" &&
    Number.isSafeInteger(reference.size_bytes) && reference.size_bytes >= 0 &&
    typeof reference.sha256 === "string" &&
    SHA256_PATTERN.test(reference.sha256);
};

export const isAssetReferenceListField = (field: FormField): boolean =>
  field.type === "list" && field.items?.type === "asset_reference";

export const parseAssetReference = (
  value: string,
): AssetReference | null => {
  if (!value.trim()) return null;
  try {
    const parsed: unknown = JSON.parse(value);
    return isAssetReference(parsed) ? parsed : null;
  } catch {
    return null;
  }
};

export const parseAssetReferenceList = (
  value: string,
): AssetReference[] | null => {
  if (!value.trim()) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    if (!Array.isArray(parsed) || !parsed.every(isAssetReference)) return null;
    return parsed;
  } catch {
    return null;
  }
};

export const serializeAssetReference = (value: AssetReference): string =>
  JSON.stringify(value);

export const serializeAssetReferenceList = (
  value: AssetReference[],
): string => JSON.stringify(value);

export const hasDuplicateAssetReferences = (
  references: AssetReference[],
): boolean => {
  const ids = new Set<string>();
  for (const reference of references) {
    if (ids.has(reference.asset_id)) return true;
    ids.add(reference.asset_id);
  }
  return false;
};

export const formatAssetSize = (sizeBytes: number, locale = "en-US"): string =>
  new Intl.NumberFormat(locale, {
    style: "unit",
    unit: "byte",
    unitDisplay: "long",
    maximumFractionDigits: 1,
  }).format(sizeBytes);
