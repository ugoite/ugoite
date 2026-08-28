import type {
  Entry,
  EntryCreatePayload,
  EntryRecord,
  EntryRevision,
  EntryRevisionContent,
  EntryUpdatePayload,
  Form,
} from "./types";
import { normalizeEntryRecord, normalizeTimestamp } from "./date-format";
import { buildEntryMarkdownByMode } from "./entry-input";
import { protocolFetch, UgoiteApiError } from "./ugoite-client/protocol";

type EntryResponse = Omit<Entry, "content"> & {
  content?: string;
  markdown?: string;
};

const normalizeEntry = (entry: EntryResponse): Entry => ({
  ...entry,
  content: entry.content ?? entry.markdown ?? "",
  created_at: normalizeTimestamp(entry.created_at),
  updated_at: normalizeTimestamp(entry.updated_at),
});

const currentRevisionIdFromError = (
  error: UgoiteApiError,
): string | undefined => {
  if (!error.payload || typeof error.payload !== "object") return undefined;
  const value = (error.payload as Record<string, unknown>)[
    "current_revision_id"
  ];
  return typeof value === "string" ? value : undefined;
};

/** Entry API client backed by the shared Rust/WASM protocol. */
export const entryApi = {
  async list(
    spaceId: string,
    limit?: number,
    offset?: number,
  ): Promise<EntryRecord[]> {
    const entries = await protocolFetch<EntryRecord[]>("entry.list", {
      space_id: spaceId,
      ...(limit === undefined ? {} : { limit }),
      ...(offset === undefined ? {} : { offset }),
    });
    return entries.map(normalizeEntryRecord);
  },

  async get(
    spaceId: string,
    entryId: string,
    pin?: string,
  ): Promise<Entry> {
    const entry = await protocolFetch<EntryResponse>("entry.get", {
      space_id: spaceId,
      entry_id: entryId,
      ...(pin ? { pin } : {}),
    });
    return normalizeEntry(entry);
  },

  async create(
    spaceId: string,
    payload: EntryCreatePayload,
  ): Promise<{ id: string; revision_id: string }> {
    return await protocolFetch<{ id: string; revision_id: string }>(
      "entry.create",
      { space_id: spaceId },
      payload,
    );
  },

  async createFromMarkdown(
    spaceId: string,
    markdown: string,
    id?: string,
  ): Promise<{ id: string; revision_id: string }> {
    return await this.create(spaceId, { id, markdown });
  },

  async createFromWebform(
    spaceId: string,
    formDef: Form,
    title: string,
    fieldValues: Record<string, string>,
    id?: string,
  ): Promise<{ id: string; revision_id: string }> {
    const markdown = buildEntryMarkdownByMode(
      formDef,
      title,
      fieldValues,
      "webform",
    );
    return await this.create(spaceId, { id, markdown });
  },

  async createFromChat(
    spaceId: string,
    formDef: Form,
    title: string,
    answers: Record<string, string>,
    id?: string,
  ): Promise<{ id: string; revision_id: string }> {
    const markdown = buildEntryMarkdownByMode(formDef, title, answers, "chat");
    return await this.create(spaceId, { id, markdown });
  },

  async update(
    spaceId: string,
    entryId: string,
    payload: EntryUpdatePayload,
  ): Promise<{ id: string; revision_id: string }> {
    try {
      return await protocolFetch<{ id: string; revision_id: string }>(
        "entry.update",
        { space_id: spaceId, entry_id: entryId },
        payload,
      );
    } catch (error) {
      if (error instanceof UgoiteApiError && error.status === 409) {
        throw new RevisionConflictError(
          error.message,
          currentRevisionIdFromError(error),
          error,
        );
      }
      throw error;
    }
  },

  async delete(spaceId: string, entryId: string): Promise<void> {
    await protocolFetch<unknown>("entry.delete", {
      space_id: spaceId,
      entry_id: entryId,
    });
  },

  async history(
    spaceId: string,
    entryId: string,
    pin?: string,
  ): Promise<{ revisions: EntryRevision[] }> {
    return await protocolFetch<{ revisions: EntryRevision[] }>(
      "entry.history",
      {
        space_id: spaceId,
        entry_id: entryId,
        ...(pin ? { pin } : {}),
      },
    );
  },

  async getRevision(
    spaceId: string,
    entryId: string,
    revisionId: string,
    pin?: string,
  ): Promise<EntryRevisionContent> {
    return await protocolFetch<EntryRevisionContent>("entry.revision", {
      space_id: spaceId,
      entry_id: entryId,
      revision_id: revisionId,
      ...(pin ? { pin } : {}),
    });
  },

  async restore(
    spaceId: string,
    entryId: string,
    revisionId: string,
    pin?: string,
  ): Promise<Entry> {
    const entry = await protocolFetch<EntryResponse>(
      "entry.restore",
      { space_id: spaceId, entry_id: entryId },
      { revision_id: revisionId, ...(pin ? { pin } : {}) },
    );
    return normalizeEntry(entry);
  },
};

export class RevisionConflictError extends Error {
  constructor(
    message: string,
    public currentRevisionId?: string,
    public apiError?: UgoiteApiError,
  ) {
    super(message);
    this.name = "RevisionConflictError";
  }
}
