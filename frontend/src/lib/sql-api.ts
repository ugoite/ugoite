import { normalizeTimestamp } from "./date-format";
import type {
  SqlCreatePayload,
  SqlEntry,
  SqlQueryCountRequest,
  SqlQueryPlan,
  SqlQueryPlanRequest,
  SqlQueryPage,
  SqlQueryRequest,
  SqlUpdatePayload,
} from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

type SqlMutationResponse = {
  id: string;
  revisionId: string;
};

type SqlApiResponse = Omit<SqlEntry, "created_at" | "updated_at"> & {
  created_at: string | number;
  updated_at: string | number;
};

const normalizeSqlEntry = (entry: SqlApiResponse): SqlEntry => {
  const normalizedEntry = { ...entry };
  normalizedEntry.created_at = normalizeTimestamp(entry.created_at);
  normalizedEntry.updated_at = normalizeTimestamp(entry.updated_at);
  return normalizedEntry;
};

export const sqlApi = {
  async list(spaceId: string): Promise<SqlEntry[]> {
    const entries = await protocolFetch<SqlApiResponse[]>("sql.list", {
      space_id: spaceId,
    });
    return entries.map(normalizeSqlEntry);
  },

  async get(spaceId: string, sqlId: string): Promise<SqlEntry> {
    const entry = await protocolFetch<SqlApiResponse>("sql.get", {
      space_id: spaceId,
      sql_id: sqlId,
    });
    return normalizeSqlEntry(entry);
  },

  async create(
    spaceId: string,
    payload: SqlCreatePayload,
  ): Promise<SqlMutationResponse> {
    const data = await protocolFetch<Record<string, string>>(
      "sql.create",
      { space_id: spaceId },
      payload,
    );
    return { id: data.id, revisionId: data.revision_id };
  },

  async update(
    spaceId: string,
    sqlId: string,
    payload: SqlUpdatePayload,
  ): Promise<SqlMutationResponse> {
    const data = await protocolFetch<Record<string, string>>(
      "sql.update",
      { space_id: spaceId, sql_id: sqlId },
      payload,
    );
    return { id: data.id, revisionId: data.revision_id };
  },

  async delete(spaceId: string, sqlId: string): Promise<void> {
    await protocolFetch<unknown>("sql.delete", {
      space_id: spaceId,
      sql_id: sqlId,
    });
  },

  async query(
    spaceId: string,
    request: SqlQueryRequest,
    signal?: AbortSignal,
  ): Promise<SqlQueryPage> {
    return await protocolFetch<SqlQueryPage>(
      "sql.query",
      { space_id: spaceId },
      request,
      { signal },
    );
  },

  async count(
    spaceId: string,
    request: SqlQueryCountRequest,
    signal?: AbortSignal,
  ): Promise<number> {
    const payload = await protocolFetch<{ count: number }>(
      "sql.query.count",
      { space_id: spaceId },
      request,
      { signal },
    );
    return payload.count;
  },

  async plan(
    spaceId: string,
    request: SqlQueryPlanRequest,
    signal?: AbortSignal,
  ): Promise<SqlQueryPlan> {
    return await protocolFetch<SqlQueryPlan>(
      "sql.query.plan",
      { space_id: spaceId },
      request,
      { signal },
    );
  },
};
