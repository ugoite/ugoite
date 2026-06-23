import { normalizeTimestamp } from "./date-format";
import type { SqlCreatePayload, SqlEntry, SqlUpdatePayload } from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

type SqlMutationResponse = {
  id: string;
  revisionId: string;
};

const normalizeSqlEntry = (entry: SqlEntry): SqlEntry => {
  const normalizedEntry = { ...entry };
  normalizedEntry.created_at = normalizeTimestamp(entry.created_at);
  normalizedEntry.updated_at = normalizeTimestamp(entry.updated_at);
  return normalizedEntry;
};

export const sqlApi = {
  async list(spaceId: string): Promise<SqlEntry[]> {
    const entries = await protocolFetch<SqlEntry[]>("sql.list", {
      space_id: spaceId,
    });
    return entries.map(normalizeSqlEntry);
  },

  async get(spaceId: string, sqlId: string): Promise<SqlEntry> {
    const entry = await protocolFetch<SqlEntry>("sql.get", {
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
};
