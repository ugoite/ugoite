/**
 * Type definitions for the Ugoite API
 * Based on docs/spec/data-model/overview.md and docs/spec/api/rest.md
 */

/** Space metadata */
export interface SpaceStorage {
  type?: string;
  root?: string;
}

/** Space metadata */
export interface SpaceStorage {
  type?: string;
  root?: string;
}

export interface StorageConnectionConfig {
  uri: string;
  endpoint?: string;
  [key: string]: unknown;
}

/** Space metadata */
export interface Space {
  id: string;
  name: string;
  created_at: string;
  storage?: SpaceStorage;
  storage_config?: StorageConnectionConfig;
  settings?: Record<string, unknown>;
}

/** Space patch payload */
export interface SpacePatchPayload {
  name?: string;
  storage_config?: StorageConnectionConfig;
  settings?: Record<string, unknown>;
}

export interface UserPreferences {
  selected_space_id: string | null;
  locale: "en" | "ja" | null;
}

export type UserPreferencesPatchPayload = Partial<UserPreferences>;

/** Test connection payload */
export interface TestConnectionPayload {
  storage_config: StorageConnectionConfig;
}

export interface SpaceMember {
  principal: {
    principal_id: string;
    kind: "human" | "agent";
    display_name: string;
    state: "invited" | "active" | "suspended" | "revoked";
    created_at: string;
  };
  role: "owner" | "editor" | "viewer";
  created_at: string;
}

export interface SpaceMemberInvitePayload {
  label: string;
  role: "owner" | "editor" | "viewer";
}

export interface SpaceMemberInviteResponse {
  invitation_id: string;
  expires_at: string;
  invitation_url: string;
}

export interface SpaceMemberRoleUpdatePayload {
  role: "owner" | "editor" | "viewer";
}

export interface AgentPrincipal {
  agent_id: string;
  display_name: string;
  description: string;
  sponsor_principal_id: string;
  owner_principal_ids: string[];
  mode: "autonomous" | "delegated" | "both";
  status: "active" | "suspended" | "revoked";
  created_at: string;
  expires_at: string;
  last_used_at?: string;
}

export interface AgentCreatePayload {
  display_name: string;
  description: string;
  mode: "autonomous" | "delegated" | "both";
  public_key_jwk: Record<string, unknown>;
  granted_actions: Array<"read" | "create" | "update">;
  expires_at: string;
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
  created_at?: string;
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
  markdown?: string;
  revision_id: string;
  created_at: string;
  updated_at: string;
}

/** Rendered content captured for an entry revision. */
export interface EntryRevisionContent {
  revision_id: string;
  parent_revision_id?: string | null;
  author?: string;
  markdown: string;
  frontmatter?: Record<string, unknown>;
  sections?: Record<string, string>;
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
  markdown: string;
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
  id?: number;
  type: string;
  required: boolean;
  target_form?: string;
  /** Backend-owned stable SQL column; never derive this from the field label. */
  sql_column?: string;
}

export interface Form {
  id?: string;
  name: string;
  /** Backend-owned DataFusion relation; never derive this from the Form name. */
  sql_relation?: string;
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
  name: string | null;
  kind: "user-query" | "search-history";
  metadata?: SqlMetadata;
  sql: string;
  variables: SqlVariable[];
  created_at: string;
  updated_at: string;
  revision_id: string;
}

export interface SqlCreatePayload {
  name: string | null;
  kind: "user-query" | "search-history";
  metadata?: SqlMetadata;
  sql: string;
  variables: SqlVariable[];
}

export interface SqlUpdatePayload {
  name: string | null;
  kind: "user-query" | "search-history";
  metadata?: SqlMetadata;
  sql: string;
  variables: SqlVariable[];
  parent_revision_id: string;
}

export type SqlMetadata =
  | {
    searchCriteria: {
      formName: string;
      tags: string[];
      updatedFrom: string;
      updatedTo: string;
      fieldConditions: Array<{
        field: string;
        operator: "equals" | "contains" | "lt" | "lte" | "gt" | "gte";
        value: string;
      }>;
    };
    generatedName?: never;
  }
  | {
    searchCriteria?: never;
    generatedName: "untitled";
  };

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

/** A row from an arbitrary SQL session projection. */
export type SqlSessionRow = Record<string, unknown>;

export interface SqlSessionRows {
  rows: SqlSessionRow[];
  offset: number;
  limit: number;
  totalCount: number;
}

/** API error response */
export interface ApiError {
  detail: string;
}

/** Minimal keyword-search result returned by the backend Entry scan. */
export interface KeywordSearchResult {
  id: string;
  title: string;
  form: string;
  created_at: string | number;
  updated_at: string | number;
}
