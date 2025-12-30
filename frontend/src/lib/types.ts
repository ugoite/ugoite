/**
 * Type definitions for the IEapp API
 * Based on docs/spec/03_data_model.md and docs/spec/04_api_and_mcp.md
 */

/** Workspace metadata */
export interface Workspace {
	id: string;
	name: string;
	created_at: string;
	storage_config?: Record<string, unknown>;
}

/** Note record (from index) */
export interface NoteRecord {
	id: string;
	title: string;
	class?: string;
	updated_at: string;
	properties: Record<string, unknown>;
	tags: string[];
	links: NoteLink[];
	canvas_position?: CanvasPosition;
	checksum?: string;
}

/** Note link */
export interface NoteLink {
	id: string;
	target: string;
	kind: string;
}

/** Canvas position for spatial view */
export interface CanvasPosition {
	x: number;
	y: number;
}

/** Full note content */
export interface Note {
	id: string;
	content: string;
	revision_id: string;
	created_at: string;
	updated_at: string;
}

/** Note history entry */
export interface NoteRevision {
	revision_id: string;
	created_at: string;
	checksum: string;
}

/** Create note payload */
export interface NoteCreatePayload {
	id?: string;
	content: string;
}

/** Update note payload */
export interface NoteUpdatePayload {
	markdown: string;
	parent_revision_id: string;
	frontmatter?: Record<string, unknown>;
	canvas_position?: CanvasPosition;
}

/** Query request */
export interface QueryRequest {
	filter: Record<string, unknown>;
}

/** API error response */
export interface ApiError {
	detail: string;
}
