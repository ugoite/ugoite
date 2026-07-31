import { normalizeTimestamp } from "./date-format";
import type { EntryRecord, SearchResult } from "./types";
import { normalizeEntryRecord } from "./date-format";
import { protocolFetch } from "./ugoite-client/protocol";

export type EntrySummary = {
  id: string;
  title: string;
  form: string;
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

  async keyword(spaceId: string, query: string): Promise<SearchResult[]> {
    const results = await protocolFetch<SearchResult[]>("search.keyword", {
     space_id: spaceId,
     q: query,
   });
    return results.map((result) => ({
      ...result,
      created_at: result.created_at === undefined
        ? undefined
        : normalizeTimestamp(result.created_at),
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
