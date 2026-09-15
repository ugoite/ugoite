import { formatAssetSize, isAssetReference } from "./asset-reference";
import type { AssetReference } from "./types";

export type DisplayLocale = "en-US" | "ja-JP";

/** Convert only scalar values to text; objects must use a field renderer. */
export const safeText = (value: unknown): string => {
  if (value === null || value === undefined) return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return "";
};

const assetTypeLabel = (reference: AssetReference): string => {
  const extension = reference.name.split(/[\\/]/).pop()?.split(".").pop();
  if (extension) return extension.toUpperCase();
  const subtype = reference.media_type.split("/", 2)[1];
  return subtype ? subtype.toUpperCase() : reference.media_type;
};

/** Keep Form-owned assets useful in compact lists without exposing internals. */
export const formatAssetSummary = (
  reference: AssetReference,
  locale: DisplayLocale = "en-US",
): string =>
  `${assetTypeLabel(reference)} · ${
    formatAssetSize(
      reference.size_bytes,
      locale,
    )
  }`;

const formatScalarList = (value: unknown[]): string | null => {
  const values = value.map(safeText);
  return values.every((item) => item !== "") ? values.join(", ") : null;
};

/** Render a value for read-only UI without ever falling back to object coercion. */
export const formatValueForDisplay = (
  value: unknown,
  locale: DisplayLocale = "en-US",
  emptyValue = "-",
): string => {
  if (isAssetReference(value)) return formatAssetSummary(value, locale);
  if (Array.isArray(value)) {
    if (value.length === 0) return emptyValue;
    const assets = value.filter(isAssetReference);
    if (assets.length === value.length) {
      return assets.map((asset) => formatAssetSummary(asset, locale)).join(
        ", ",
      );
    }
    return formatScalarList(value) ?? emptyValue;
  }
  return safeText(value) || emptyValue;
};

/** Convert only values supported by the table's plain-text editor. */
export const formatValueForInput = (value: unknown): string => {
  if (Array.isArray(value)) return formatScalarList(value) ?? "";
  return safeText(value);
};

export const isPlainEditableValue = (value: unknown): boolean =>
  value === null || value === undefined ||
  typeof value === "string" || typeof value === "number" ||
  typeof value === "boolean";
