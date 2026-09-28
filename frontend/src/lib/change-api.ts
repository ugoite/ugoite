import { protocolFetch } from "./ugoite-client/protocol";

/** One Space-level append-only Change: a durable Knowledge mutation. */
export type SpaceChange = {
  change_id: string;
  generation: number;
  actor_principal_id: string;
  message: string | null;
  reverts_change_id: string | null;
  run_id: string | null;
  created_at_micros: number;
};

export type SpaceChangeDescriptor = Pick<
  SpaceChange,
  | "actor_principal_id"
  | "message"
  | "reverts_change_id"
  | "run_id"
  | "created_at_micros"
>;

export type SpaceChangeComparedValue = {
  state: "missing" | "value" | "unavailable" | "redacted";
  value?: unknown;
};

export type SpaceChangeFieldGroup = {
  form_id: string;
  field_id: number;
  before: SpaceChangeComparedValue;
  after: SpaceChangeComparedValue;
  affected_entry_count: number;
};

export type SpaceChangeSummary = {
  affected_entry_count: number;
  target_form_ids: string[];
  field_groups: SpaceChangeFieldGroup[];
};

export type SpaceChangeQueryRow = {
  change_id: string;
  generation: number;
  change: SpaceChangeDescriptor;
  publication: {
    generation: number;
    publication_uri: { space_uid: string; key: string };
    publication_checksum: string;
  };
  target_visibility: "complete" | "partial";
  summary: SpaceChangeSummary | null;
};

export type SpaceChangeQueryFilters = {
  limit?: number;
  cursor?: string;
  actor_principal_id?: string;
  run_id?: string;
  text?: string;
  created_after_micros?: number;
  created_before_micros?: number;
  sort?: SpaceChangeSort[];
};

export type SpaceChangeSort = {
  field: "created_at_micros" | "actor_principal_id" | "run_id";
  direction: "asc" | "desc";
};

export type SpaceChangeQueryPage = {
  changes: SpaceChangeQueryRow[];
  next_cursor: string | null;
};

export type SpaceChangeAffectedEntry = {
  form_id: string;
  entry_id: string;
  before_revision_id: string | null;
  after_revision_id: string;
  operation: string;
  fields: Array<{
    field_id: number;
    before: SpaceChangeComparedValue;
    after: SpaceChangeComparedValue;
  }>;
};

export type SpaceChangeInspection = {
  change_id: string;
  change: SpaceChangeDescriptor;
  target_visibility: "complete" | "partial";
  summary: SpaceChangeSummary | null;
  targets: SpaceChangeAffectedEntry[];
  next_cursor: string | null;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : {};

const asString = (value: unknown): string | null =>
  typeof value === "string" ? value : null;

const requiredString = (value: unknown, field: string): string => {
  const result = asString(value);
  if (result === null) throw new Error(`Invalid Change query field: ${field}`);
  return result;
};

const requiredNumber = (value: unknown, field: string): number => {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`Invalid Change query field: ${field}`);
  }
  return value;
};

const requiredFieldId = (value: unknown, field: string): number => {
  const result = requiredNumber(value, field);
  if (!Number.isSafeInteger(result) || result <= 0) {
    throw new Error(`Invalid Change query field: ${field}`);
  }
  return result;
};

const decodeComparedValue = (value: unknown): SpaceChangeComparedValue => {
  const row = asRecord(value);
  const state = asString(row.state);
  if (
    state !== "missing" && state !== "value" && state !== "unavailable" &&
    state !== "redacted"
  ) {
    throw new Error("Invalid Change query compared value");
  }
  return state === "value" ? { state, value: row.value } : { state };
};

const decodeSummary = (value: unknown): SpaceChangeSummary | null => {
  if (value === null) return null;
  const summary = asRecord(value);
  if (!Array.isArray(summary.target_form_ids) || !Array.isArray(summary.field_groups)) {
    throw new Error("Invalid Change query summary");
  }
  return {
    affected_entry_count: requiredNumber(
      summary.affected_entry_count,
      "summary.affected_entry_count",
    ),
    target_form_ids: summary.target_form_ids.map((value) =>
      requiredString(value, "summary.target_form_ids")
    ),
    field_groups: summary.field_groups.map((value) => {
      const group = asRecord(value);
      return {
        form_id: requiredString(group.form_id, "summary.field_groups.form_id"),
        field_id: requiredFieldId(
          group.field_id,
          "summary.field_groups.field_id",
        ),
        before: decodeComparedValue(group.before),
        after: decodeComparedValue(group.after),
        affected_entry_count: requiredNumber(
          group.affected_entry_count,
          "summary.field_groups.affected_entry_count",
        ),
      };
    }),
  };
};

const decodeQueryRow = (value: unknown): SpaceChangeQueryRow => {
  const row = asRecord(value);
  const change = asRecord(row.change);
  const publication = asRecord(row.publication);
  const targetVisibility = asString(row.target_visibility);
  if (targetVisibility !== "complete" && targetVisibility !== "partial") {
    throw new Error("Invalid Change query target visibility");
  }
  return {
    change_id: requiredString(row.change_id, "change_id"),
    generation: requiredNumber(row.generation, "generation"),
    change: {
      actor_principal_id: requiredString(
        change.actor_principal_id,
        "change.actor_principal_id",
      ),
      message: asString(change.message),
      reverts_change_id: asString(change.reverts_change_id),
      run_id: asString(change.run_id),
      created_at_micros: requiredNumber(
        change.created_at_micros,
        "change.created_at_micros",
      ),
    },
    publication: {
      generation: requiredNumber(
        publication.generation,
        "publication.generation",
      ),
      publication_uri: (() => {
        const uri = asRecord(publication.publication_uri);
        return {
          space_uid: requiredString(
            uri.space_uid,
            "publication.publication_uri.space_uid",
          ),
          key: requiredString(uri.key, "publication.publication_uri.key"),
        };
      })(),
      publication_checksum: requiredString(
        publication.publication_checksum,
        "publication.publication_checksum",
      ),
    },
    target_visibility: targetVisibility,
    summary: decodeSummary(row.summary),
  };
};

const decodeAffectedEntry = (value: unknown): SpaceChangeAffectedEntry => {
  const row = asRecord(value);
  if (!Array.isArray(row.fields)) {
    throw new Error("Invalid Change target evidence");
  }
  return {
    form_id: requiredString(row.form_id, "target.form_id"),
    entry_id: requiredString(row.entry_id, "target.entry_id"),
    before_revision_id: asString(row.before_revision_id),
    after_revision_id: requiredString(
      row.after_revision_id,
      "target.after_revision_id",
    ),
    operation: requiredString(row.operation, "target.operation"),
    fields: row.fields.map((item) => {
      const field = asRecord(item);
      return {
        field_id: requiredFieldId(field.field_id, "target.fields.field_id"),
        before: decodeComparedValue(field.before),
        after: decodeComparedValue(field.after),
      };
    }),
  };
};

const decodeInspection = (value: unknown): SpaceChangeInspection => {
  const row = asRecord(value);
  const change = asRecord(row.change);
  const targetVisibility = asString(row.target_visibility);
  if (targetVisibility !== "complete" && targetVisibility !== "partial") {
    throw new Error("Invalid Change inspection visibility");
  }
  if (!Array.isArray(row.targets)) {
    throw new Error("Invalid Change inspection targets");
  }
  return {
    change_id: requiredString(row.change_id, "change_id"),
    change: {
      actor_principal_id: requiredString(
        change.actor_principal_id,
        "change.actor_principal_id",
      ),
      message: asString(change.message),
      reverts_change_id: asString(change.reverts_change_id),
      run_id: asString(change.run_id),
      created_at_micros: requiredNumber(
        change.created_at_micros,
        "change.created_at_micros",
      ),
    },
    target_visibility: targetVisibility,
    summary: decodeSummary(row.summary ?? null),
    targets: row.targets.map(decodeAffectedEntry),
    next_cursor: asString(row.next_cursor),
  };
};

/** Result of appending a revert Change. The reverted Change is kept. */
export type RevertResult = {
  change_id: string;
  reverts_change_id: string;
  run_id: string | null;
};

/** Result of appending Run undo inverses. The Run itself is untouched. */
export type UndoResult = {
  run_id: string;
  reverted_change_count: number;
};

/**
 * Typed client for Space-level Change history and append-only recovery.
 *
 * Change history is durable Knowledge mutation evidence. It is distinct
 * from the Audit log, which is security/operational evidence.
 *
 * Revert and undo never rewrite the past: each returns a server-confirmed
 * result describing the newly appended Change(s).
 */
export const changeApi = {
  async list(spaceId: string): Promise<SpaceChange[]> {
    const changes = await protocolFetch<unknown[]>("change.list", {
      space_id: spaceId,
    });
    return changes.map((item) => {
      const row = asRecord(item);
      const change = asRecord(row.change);
      return {
        change_id: asString(row.change_id) ?? "",
        generation: typeof row.generation === "number" ? row.generation : 0,
        actor_principal_id: asString(change.actor_principal_id) ?? "",
        message: asString(change.message),
        reverts_change_id: asString(change.reverts_change_id),
        run_id: asString(change.run_id),
        created_at_micros: typeof change.created_at_micros === "number"
          ? change.created_at_micros
          : 0,
      };
    });
  },

  async query(
    spaceId: string,
    filters: SpaceChangeQueryFilters = {},
  ): Promise<SpaceChangeQueryPage> {
    const value = asRecord(
      await protocolFetch<unknown>("change.query", {
        space_id: spaceId,
        ...filters,
      }),
    );
    if (!Array.isArray(value.changes)) {
      throw new Error("Invalid Change query page");
    }
    return {
      changes: value.changes.map(decodeQueryRow),
      next_cursor: asString(value.next_cursor),
    };
  },

  async inspect(
    spaceId: string,
    changeId: string,
    options: { limit?: number; cursor?: string } = {},
  ): Promise<SpaceChangeInspection> {
    return decodeInspection(
      await protocolFetch<unknown>("change.inspect", {
        space_id: spaceId,
        change_id: changeId,
        limit: options.limit ?? 10,
        cursor: options.cursor,
      }),
    );
  },

  async affectedEntry(
    spaceId: string,
    changeId: string,
    entryId: string,
  ): Promise<SpaceChangeAffectedEntry> {
    const response = asRecord(
      await protocolFetch<unknown>("change.affected.get", {
        space_id: spaceId,
        change_id: changeId,
        entry_id: entryId,
      }),
    );
    return decodeAffectedEntry(response.target);
  },

  async revert(
    spaceId: string,
    changeId: string,
    options: { runId?: string; message?: string } = {},
  ): Promise<RevertResult> {
    const body: Record<string, string> = {};
    if (options.runId) body.run_id = options.runId;
    if (options.message) body.message = options.message;
    const result = await protocolFetch<Record<string, unknown>>(
      "change.revert",
      { space_id: spaceId, change_id: changeId },
      body,
    );
    return {
      change_id: asString(result.change_id) ?? "",
      reverts_change_id: asString(result.reverts_change_id) ?? changeId,
      run_id: asString(result.run_id),
    };
  },

  async undoRun(spaceId: string, runId: string): Promise<UndoResult> {
    const result = await protocolFetch<Record<string, unknown>>("run.undo", {
      space_id: spaceId,
      run_id: runId,
    });
    const count = result.reverted_change_count;
    return {
      run_id: asString(result.run_id) ?? runId,
      reverted_change_count: typeof count === "number" ? count : 0,
    };
  },
};
