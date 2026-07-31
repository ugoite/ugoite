import { normalizeTimestamp } from "./date-format";
import type {
  EntryRecord,
  SqlSession,
  SqlSessionRow,
  SqlSessionRows,
} from "./types";
import { protocolFetch } from "./ugoite-client/protocol";

const isSqlSessionRow = (value: unknown): value is SqlSessionRow =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const timestampValue = (value: unknown): string | undefined => {
  if (typeof value !== "string" && typeof value !== "number") return undefined;
  return normalizeTimestamp(value);
};

/** Convert the backend's Form SQL projection into the Entry card contract. */
export const sqlSessionRowToEntryRecord = (
  row: SqlSessionRow,
): EntryRecord => {
  const properties = Object.fromEntries(
    Object.entries(row).filter(([key]) => key.startsWith("field_")),
  );
  const form = typeof row.form === "string" ? row.form : undefined;
  const createdAt = timestampValue(row._ugoite_created_at);

  return {
    id: String(row._ugoite_id ?? ""),
    title: String(row._ugoite_title ?? ""),
    ...(form ? { form } : {}),
    ...(createdAt ? { created_at: createdAt } : {}),
    updated_at: timestampValue(row._ugoite_updated_at) ?? "",
    properties,
    tags: [],
    links: [],
  };
};

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
    const rows = Array.isArray(payload.rows)
      ? payload.rows.filter(isSqlSessionRow)
      : [];
    return {
      rows,
      offset: Number(payload.offset ?? 0),
      limit: Number(payload.limit ?? 0),
      totalCount: Number(payload.total_count ?? payload.totalCount ?? 0),
    };
  },
};
