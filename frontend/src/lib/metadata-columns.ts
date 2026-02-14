export const RESERVED_METADATA_COLUMNS = [
	"id",
	"entry_id",
	"title",
	"form",
	"tags",
	"links",
	"assets",
	"created_at",
	"updated_at",
	"revision_id",
	"parent_revision_id",
	"deleted",
	"deleted_at",
	"author",
	"integrity",
	"space_id",
	"word_count",
] as const;

const RESERVED_METADATA_SET = new Set(RESERVED_METADATA_COLUMNS.map((name) => name.toLowerCase()));

export function isReservedMetadataColumn(name: string): boolean {
	return RESERVED_METADATA_SET.has(name.trim().toLowerCase());
}
