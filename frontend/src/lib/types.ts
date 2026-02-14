/**
 * Type definitions for the Ugoite API
 * Based on docs/spec/data-model/overview.md and docs/spec/api/rest.md
 */

/** Space metadata */
export interface Space {
	id: string;
	name: string;
	created_at: string;
	storage_config?: Record<string, unknown>;
	settings?: Record<string, unknown>;
}

/** Space patch payload */
export interface SpacePatchPayload {
	name?: string;
	storage_config?: Record<string, unknown>;
	settings?: Record<string, unknown>;
}

/** Test connection payload */
export interface TestConnectionPayload {
	storage_config: Record<string, unknown>;
}

/** Asset metadata */
export interface Asset {
	id: string;
	name: string;
	path: string;
	link?: string;
	uploaded_at?: string;
}

/** Entry record (from index) */
export interface EntryRecord {
	id: string;
	title: string;
	form?: string;
	updated_at: string;
	properties: Record<string, unknown>;
	tags: string[];
	links: EntryLink[];
	canvas_position?: CanvasPosition;
	checksum?: string;
	assets?: Asset[];
}

/** Entry link */
export interface EntryLink {
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

/** Full entry content */
export interface Entry {
	id: string;
	title?: string;
	frontmatter?: Record<string, unknown>;
	sections?: Record<string, string>;
	assets?: Asset[];
	links?: EntryLink[];
	form?: string;
	tags?: string[];
	canvas_position?: CanvasPosition;
	content: string;
	revision_id: string;
	created_at: string;
	updated_at: string;
}

/** Entry history entry */
export interface EntryRevision {
	revision_id: string;
	created_at: string;
	checksum: string;
}

/** Create entry payload */
export interface EntryCreatePayload {
	id?: string;
	content: string;
}

/** Update entry payload */
export interface EntryUpdatePayload {
	markdown: string;
	parent_revision_id: string;
	frontmatter?: Record<string, unknown>;
	canvas_position?: CanvasPosition;
	assets?: Asset[];
}

export interface FormField {
	type: string;
	required: boolean;
	target_form?: string;
}

export interface Form {
	name: string;
	version: number;
	template: string;
	fields: Record<string, FormField>;
	defaults?: Record<string, unknown>;
}

export interface FormCreatePayload {
	name: string;
	version?: number;
	template: string;
	fields: Record<string, FormField>;
	defaults?: Record<string, unknown>;
	strategies?: Record<string, unknown>;
}

/** Query request */
export interface QueryRequest {
	filter: Record<string, unknown>;
}

/** SQL variable definition */
export interface SqlVariable {
	type: string;
	name: string;
	description: string;
}

/** Saved SQL entry */
export interface SqlEntry {
	id: string;
	name: string;
	sql: string;
	variables: SqlVariable[];
	created_at: string;
	updated_at: string;
	revision_id: string;
}

export interface SqlCreatePayload {
	id?: string;
	name: string;
	sql: string;
	variables: SqlVariable[];
}

export interface SqlUpdatePayload {
	name: string;
	sql: string;
	variables: SqlVariable[];
	parent_revision_id?: string | null;
}

export interface SqlSession {
	id: string;
	space_id: string;
	sql_id: string;
	sql: string;
	status: "ready" | "running" | "failed" | "expired";
	created_at: string;
	expires_at: string;
	error?: string | null;
	view: {
		sql_id: string;
		snapshot_id: number;
		snapshot_at?: string;
		schema_version?: number;
	};
	pagination: {
		strategy: "offset";
		order_by: string[];
		default_limit: number;
		max_limit: number;
	};
	count?: {
		mode: "on_demand" | "cached";
		cached_at?: string | null;
		value?: number | null;
	};
}

export interface SqlSessionRows {
	rows: EntryRecord[];
	offset: number;
	limit: number;
	totalCount: number;
}

/** API error response */
export interface ApiError {
	detail: string;
}

/** Search result entry */
export type SearchResult = EntryRecord;
