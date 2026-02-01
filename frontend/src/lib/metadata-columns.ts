export const RESERVED_METADATA_COLUMNS = [
	"id",
	"note_id",
	"title",
	"class",
	"tags",
	"links",
	"attachments",
	"created_at",
	"updated_at",
	"revision_id",
	"parent_revision_id",
	"deleted",
	"deleted_at",
	"author",
	"canvas_position",
	"integrity",
	"workspace_id",
	"word_count",
] as const;

const RESERVED_METADATA_SET = new Set(RESERVED_METADATA_COLUMNS.map((name) => name.toLowerCase()));

export function isReservedMetadataColumn(name: string): boolean {
	return RESERVED_METADATA_SET.has(name.trim().toLowerCase());
}
