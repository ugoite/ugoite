import { normalizeTimestamp } from "./date-format";
import type { EntryRecord, SqlSession, SqlSessionRows } from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

export const sqlSessionApi = {
  async create(
    spaceId: string,
    sql: string,
    parameters: Record<string, string | number | boolean | null> = {},
    parameterTypes: Record<string, string> = {},
  ): Promise<SqlSession> {
    return await protocolFetch<SqlSession>(
      "sql_session.create",
      { space_id: spaceId },
      { sql, parameters, parameter_types: parameterTypes },
    );
  },

  async get(spaceId: string, sessionId: string): Promise<SqlSession> {
    return await protocolFetch<SqlSession>(
      "sql_session.get",
      { space_id: spaceId, session_id: sessionId },
      undefined,
      { trackLoading: false },
    );
  },

  async count(spaceId: string, sessionId: string): Promise<number> {
    const payload = await protocolFetch<{ count: number }>(
      "sql_session.count",
      { space_id: spaceId, session_id: sessionId },
      undefined,
      { trackLoading: false },
    );
    return payload.count;
  },

  async rows(
    spaceId: string,
    sessionId: string,
    offset: number,
    limit: number,
  ): Promise<SqlSessionRows> {
    const payload = await protocolFetch<Record<string, unknown>>(
      "sql_session.rows",
      {
        space_id: spaceId,
        session_id: sessionId,
        offset,
        limit,
      },
      undefined,
      { trackLoading: false },
    );
    const rows = ((payload.rows ?? []) as EntryRecord[]).map((row) => ({
      ...row,
      updated_at: normalizeTimestamp(row.updated_at),
    }));
    return {
      rows,
      offset: Number(payload.offset ?? 0),
      limit: Number(payload.limit ?? 0),
      totalCount: Number(payload.total_count ?? payload.totalCount ?? 0),
    };
  },
};
