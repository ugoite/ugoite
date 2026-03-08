export const RESERVED_METADATA_CLASSES = ["SQL", "Assets"] as const;

type NamedForm = { name: string };

const RESERVED_METADATA_CLASS_SET = new Set(
	RESERVED_METADATA_CLASSES.map((name) => name.trim().toLowerCase()),
);

export function isReservedMetadataForm(name: string): boolean {
	return RESERVED_METADATA_CLASS_SET.has(name.trim().toLowerCase());
}

export function filterCreatableEntryForms<T extends NamedForm>(forms: readonly T[]): T[] {
	return forms.filter((form) => !isReservedMetadataForm(form.name));
}
