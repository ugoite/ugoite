import { createRoot } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { EntryRecord } from "./types";
import { entryApi, UgoiteApiError } from "./ugoite-client";
import { createEntryStore } from "./entry-store";

const record: EntryRecord = {
  id: "entry-1",
  updated_at: "2026-01-01T00:00:00Z",
  properties: { title: "Draft" },
  tags: [],
};

describe("EntryStore mutation outcomes", () => {
  afterEach(() => vi.restoreAllMocks());

  it("keeps the optimistic Entry state when an update outcome is unknown", async () => {
    vi.spyOn(entryApi, "list").mockResolvedValue([record]);
    const unknown = new UgoiteApiError({
      kind: "transport",
      operation: "entry.update",
      message: "connection reset",
    });
    vi.spyOn(entryApi, "update").mockRejectedValue(unknown);

    let dispose!: () => void;
    let store!: ReturnType<typeof createEntryStore>;
    createRoot((rootDispose) => {
      dispose = rootDispose;
      store = createEntryStore(() => "space-1");
    });
    try {
      await store.loadEntries();
      await expect(store.updateEntry("entry-1", {
        parent_revision_id: "revision-1",
        fields: { title: "Updated draft" },
      })).rejects.toBe(unknown);

      expect(store.entries()).toHaveLength(1);
      expect(store.entries()[0].updated_at).not.toBe(record.updated_at);
      expect(store.error()).toContain("before retrying");
    } finally {
      dispose();
    }
  });

  it("reconciles an unknown delete without repeating the write", async () => {
    const list = vi.spyOn(entryApi, "list")
      .mockResolvedValueOnce([record])
      .mockResolvedValueOnce([]);
    const unknown = new UgoiteApiError({
      kind: "transport",
      operation: "entry.delete",
      message: "connection reset",
    });
    const remove = vi.spyOn(entryApi, "delete").mockRejectedValue(unknown);

    let dispose!: () => void;
    let store!: ReturnType<typeof createEntryStore>;
    createRoot((rootDispose) => {
      dispose = rootDispose;
      store = createEntryStore(() => "space-1");
    });
    try {
      await store.loadEntries();
      await expect(store.deleteEntry("entry-1")).rejects.toBe(unknown);

      expect(remove).toHaveBeenCalledTimes(1);
      expect(list).toHaveBeenCalledTimes(2);
      expect(store.entries()).toEqual([]);
      expect(store.error()).toContain("before retrying");
    } finally {
      dispose();
    }
  });
});
