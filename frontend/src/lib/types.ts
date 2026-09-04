/**
 * Type definitions for the Ugoite API
 * Based on docs/architecture/data-model/overview.md and docs/architecture/api/rest.md
 */

/** Space metadata */
export interface SpaceStorage {
  type?: string;
  root?: string;
  uri?: string;
  endpoint?: string;
  [key: string]: unknown;
}

export interface StorageConnectionConfig {
  uri: string;
  endpoint?: string;
  [key: string]: unknown;
}

export interface SpaceStorageConfig extends SpaceStorage {
  uri?: string;
  endpoint?: string;
  [key: string]: unknown;
}

/** Space metadata */
export interface Space {
  id: string;
  space_uid?: string;
  slug?: string;
  name: string;
  created_at: string;
  storage?: SpaceStorage;
  storage_config?: SpaceStorageConfig;
  settings?: Record<string, unknown>;
}

/** Space patch payload */
export interface SpacePatchPayload {
  slug?: string;
  name?: string;
  storage_config?: StorageConnectionConfig;
  settings?: Record<string, unknown>;
}

export interface UserPreferences {
  selected_space_id: string | null;
  locale: "en" | "ja" | null;
}

export type UserPreferencesPatchPayload = Partial<UserPreferences>;

export type AuditOutcome = "success" | "deny" | "error";

export type AuditFilters = {
  action: string;
  actorId: string;
  outcome: AuditOutcome | "";
};

export type AuditListQuery = {
  offset: number;
  limit: number;
  action?: string;
  actorId?: string;
  outcome?: AuditOutcome;
};

export interface NodeAuditEvent {
  event_id: string;
  timestamp: string;
  node_id: string;
  subject_account_id: string | null;
  actor_account_id: string | null;
  credential_id: string | null;
  action: string;
  target_type: string;
  target_id: string | null;
  outcome: string;
  request_id: string | null;
  safe_metadata: Record<string, unknown>;
}

export interface SpaceAuditEvent {
  event_id: string;
  timestamp: string;
  space_id: string;
  action: string;
  subject_principal_id: string;
  actor_principal_id: string | null;
  credential_id: string | null;
  outcome: string;
  target_type: string | null;
  target_id: string | null;
  request_method: string | null;
  request_path: string | null;
  request_id: string | null;
  metadata: Record<string, unknown>;
  prev_hash: string;
  event_hash: string;
}

export interface SpaceAuditPage {
  items: SpaceAuditEvent[];
  total: number;
  offset: number;
  limit: number;
}

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

/** Stable logical asset metadata stored in an AssetReference Form value. */
export interface AssetReference {
  asset_id: string;
  name: string;
  media_type: string;
  size_bytes: number;
  sha256: string;
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
  canvas_position?: CanvasPosition;
  checksum?: string;
  author?: string;
  updated_by?: string;
  deleted_by?: string | null;
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
  form?: string;
  tags?: string[];
  canvas_position?: CanvasPosition;
  content: string;
  markdown?: string;
  revision_id: string;
  created_at: string;
  updated_at: string;
  author?: string;
  updated_by?: string;
  deleted_by?: string | null;
}

/** Rendered content captured for an entry revision. */
export interface EntryRevisionContent {
  revision_id: string;
  parent_revision_id?: string | null;
  author?: string;
  updated_by?: string;
  deleted_by?: string | null;
  markdown: string;
  frontmatter?: Record<string, unknown>;
  sections?: Record<string, string>;
}

/** Entry history entry */
export interface EntryRevision {
  revision_id: string;
  timestamp: string | number;
  checksum: string;
  author?: string;
  updated_by?: string;
  deleted_by?: string | null;
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
}

export interface FormField {
  id?: number;
  type: string;
  required: boolean;
  /** Deprecated fields remain readable but are not required for new entries. */
  deprecated?: boolean;
  target_form?: string;
  items?: {
    type: string;
    target_form?: string;
  };
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
  author?: string;
  updated_by?: string;
  deleted_by?: string | null;
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
