import type { EntryRecord, SearchResult } from "./types";
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
    return await protocolFetch<EntryRecord[]>(
      "search.query",
      { space_id: spaceId },
      { filter },
    );
  },

  async keyword(spaceId: string, query: string): Promise<SearchResult[]> {
    return await protocolFetch<SearchResult[]>("search.keyword", {
      space_id: spaceId,
      q: query,
    });
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
