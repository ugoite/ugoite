export const RESERVED_METADATA_CLASSES = ["SQL", "Assets"] as const;

const RESERVED_METADATA_CLASS_SET = new Set(
	RESERVED_METADATA_CLASSES.map((name) => name.trim().toLowerCase()),
);

export function isReservedMetadataForm(name: string): boolean {
	return RESERVED_METADATA_CLASS_SET.has(name.trim().toLowerCase());
}
