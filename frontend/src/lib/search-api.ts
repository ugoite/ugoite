import type { EntryRecord, KeywordSearchResult } from "./types";
import { normalizeEntryRecord, normalizeTimestamp } from "./date-format";
import { protocolFetch } from "./ugoite-client/protocol";

export type EntrySummary = {
  id: string;
  title: string;
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
};

/** Search & query API client backed by the shared Rust/WASM protocol. */
export const searchApi = {
  async query(
    spaceId: string,
    filter: Record<string, unknown>,
  ): Promise<EntryRecord[]> {
    const entries = await protocolFetch<EntryRecord[]>(
      "search.query",
      { space_id: spaceId },
      { filter },
    );
    return entries.map(normalizeEntryRecord);
  },

  /**
   * Typed structured Search. Logical form/field identity only; SQL
   * relation/column resolution and escaping stay in the trusted Rust layer.
   */
  async queryStructured(
    spaceId: string,
    criteria: StructuredSearchCriteria,
  ): Promise<EntryRecord[]> {
    const entries = await protocolFetch<EntryRecord[]>(
      "search.query",
      { space_id: spaceId },
      { criteria },
    );
    return entries.map(normalizeEntryRecord);
  },

  async keyword(
    spaceId: string,
    query: string,
  ): Promise<KeywordSearchResult[]> {
    const results = await protocolFetch<KeywordSearchResult[]>(
      "search.keyword",
      {
        space_id: spaceId,
        q: query,
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
