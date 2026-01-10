/**
 * Type definitions for the IEapp API
 * Based on docs/spec/data-model/overview.md and docs/spec/api/rest.md
 */

/** Workspace metadata */
export interface Workspace {
	id: string;
	name: string;
	created_at: string;
	storage_config?: Record<string, unknown>;
	settings?: Record<string, unknown>;
}

/** Workspace patch payload */
export interface WorkspacePatchPayload {
	name?: string;
	storage_config?: Record<string, unknown>;
	settings?: Record<string, unknown>;
}

/** Test connection payload */
export interface TestConnectionPayload {
	storage_config: Record<string, unknown>;
}

/** Attachment metadata */
export interface Attachment {
	id: string;
	name: string;
	path: string;
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
	attachments?: Attachment[];
}

/** Note link */
export interface NoteLink {
	id: string;
	source?: string;
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
	title?: string;
	frontmatter?: Record<string, unknown>;
	sections?: Record<string, string>;
	attachments?: Attachment[];
	links?: NoteLink[];
	class?: string;
	tags?: string[];
	canvas_position?: CanvasPosition;
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
	attachments?: Attachment[];
}

export interface ClassField {
	type: string;
	required: boolean;
}

export interface Class {
	name: string;
	version: number;
	template: string;
	fields: Record<string, ClassField>;
	defaults?: Record<string, unknown>;
}

export interface ClassCreatePayload {
	name: string;
	version?: number;
	template: string;
	fields: Record<string, ClassField>;
	defaults?: Record<string, unknown>;
	strategies?: Record<string, unknown>;
}

/** Query request */
export interface QueryRequest {
	filter: Record<string, unknown>;
}

/** API error response */
export interface ApiError {
	detail: string;
}

/** Link resource */
export interface WorkspaceLink {
	id: string;
	source: string;
	target: string;
	kind: string;
}

/** Search result entry */
export type SearchResult = NoteRecord;
