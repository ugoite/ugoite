import metadataRegistry from "../../../crates/ugoite-domain/src/metadata.json";

export const RESERVED_METADATA_COLUMNS: readonly string[] =
  metadataRegistry.columns;

const RESERVED_METADATA_SET = new Set(
  RESERVED_METADATA_COLUMNS.map((name) => name.toLowerCase()),
);

export function isReservedMetadataColumn(name: string): boolean {
  return RESERVED_METADATA_SET.has(name.trim().toLowerCase());
}
