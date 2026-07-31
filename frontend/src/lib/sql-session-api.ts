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

export class SqlSessionEntryProjectionError extends Error {
  constructor() {
    super(
      "SQL session result is not an Entry projection: expected _ugoite_id, _ugoite_title, and valid _ugoite_updated_at.",
    );
    this.name = "SqlSessionEntryProjectionError";
  }
}

const timestampValue = (value: unknown): string | undefined => {
  if (typeof value === "number") {
    return Number.isFinite(value) ? normalizeTimestamp(value) : undefined;
  }
  if (typeof value !== "string") return undefined;
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  const numeric = Number(trimmed);
  if (!Number.isFinite(numeric) && Number.isNaN(Date.parse(trimmed))) {
    return undefined;
  }
  return normalizeTimestamp(trimmed);
};

/** Convert the backend's Form SQL projection into the Entry card contract. */
export const sqlSessionRowToEntryRecord = (
  row: SqlSessionRow,
): EntryRecord => {
  if (
    typeof row._ugoite_id !== "string" ||
    !row._ugoite_id.trim() ||
    typeof row._ugoite_title !== "string"
  ) {
    throw new SqlSessionEntryProjectionError();
  }
  const updatedAt = timestampValue(row._ugoite_updated_at);
  if (!updatedAt) throw new SqlSessionEntryProjectionError();

  const properties = Object.fromEntries(
    Object.entries(row).filter(([key]) => key.startsWith("field_")),
  );
  const form = typeof row.form === "string" ? row.form : undefined;
  const createdAt = timestampValue(row._ugoite_created_at);

  return {
    id: row._ugoite_id,
    title: row._ugoite_title,
    ...(form ? { form } : {}),
    ...(createdAt ? { created_at: createdAt } : {}),
    updated_at: updatedAt,
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
