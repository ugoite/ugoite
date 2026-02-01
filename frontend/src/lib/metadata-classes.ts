export const RESERVED_METADATA_CLASSES = ["SQL"] as const;

const RESERVED_METADATA_CLASS_SET = new Set(
	RESERVED_METADATA_CLASSES.map((name) => name.trim().toLowerCase()),
);

export function isReservedMetadataClass(name: string): boolean {
	return RESERVED_METADATA_CLASS_SET.has(name.trim().toLowerCase());
}
