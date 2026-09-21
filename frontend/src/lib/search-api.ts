import type { EntryRecord, KeywordSearchResult } from "./types";
import { normalizeEntryRecord, normalizeTimestamp } from "./date-format";
import { sqlSessionRowToEntryRecord } from "./sql-session-api";
import { protocolFetch } from "./ugoite-client/protocol";

export type EntrySummary = {
  id: string;
  form: string;
};

export type StructuredSearchCondition = {
  field: string;
  operator: string;
  value: string;
};

export type StructuredSearchCriteria = {
  form: string;
  updated_from?: string;
  updated_to?: string;
  conditions: StructuredSearchCondition[];
  limit?: number;
  offset?: number;
};

const normalizeSearchEntry = (entry: Record<string, unknown>): EntryRecord => {
  if (typeof entry.id === "string") {
    return normalizeEntryRecord(entry as EntryRecord);
  }
  return sqlSessionRowToEntryRecord(entry);
};

/** Search & query API client backed by the shared Rust/WASM protocol. */
export const searchApi = {
  async query(
    spaceId: string,
    filter: Record<string, unknown>,
  ): Promise<EntryRecord[]> {
    // v0.2 contract: /query is criteria-only. A form-scoped legacy filter is
    // translated to the equivalent criteria payload so FormTable and the
    // Entries list keep working without caller churn.
    const form = filter.form;
    const body = typeof form === "string" && form.trim() !== ""
      ? { criteria: { form, conditions: [] } }
      : { filter };
    const entries = await protocolFetch<Record<string, unknown>[]>(
      "search.query",
      { space_id: spaceId },
      body,
    );
    return entries.map(normalizeSearchEntry);
  },

  /**
   * Typed structured Search. Logical form/field identity only; SQL
   * relation/column resolution and escaping stay in the trusted Rust layer.
   */
  async queryStructured(
    spaceId: string,
    criteria: StructuredSearchCriteria,
  ): Promise<EntryRecord[]> {
    const entries = await protocolFetch<Record<string, unknown>[]>(
      "search.query",
      { space_id: spaceId },
      { criteria },
    );
    return entries.map(normalizeSearchEntry);
  },

  async keyword(
    spaceId: string,
    query: string,
    limit?: number,
    offset?: number,
  ): Promise<KeywordSearchResult[]> {
    const results = await protocolFetch<KeywordSearchResult[]>(
      "search.keyword",
      {
        space_id: spaceId,
        q: query,
        ...(limit === undefined ? {} : { limit }),
        ...(offset === undefined ? {} : { offset }),
      },
    );
    return results.map((result) => ({
      ...result,
      created_at: normalizeTimestamp(result.created_at),
      updated_at: normalizeTimestamp(result.updated_at),
    }));
  },

  async rowReferenceOptions(
    spaceId: string,
    targetForm: string,
    query: string,
    limit: number,
  ): Promise<EntrySummary[]> {
    return await protocolFetch<EntrySummary[]>("entry.options", {
      space_id: spaceId,
      form: targetForm,
      q: query,
      limit,
    });
  },
};
