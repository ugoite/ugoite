import { beforeEach, describe, expect, it, vi } from "vitest";
import { createEntryQueryController, previewProjection } from "./entry-query";

const queryMock = vi.fn();

const row = (id: string) => ({
  id,
  form_id: "form-1",
  revision_id: `revision-${id}`,
  created_at_micros: 1_772_960_000_000_000,
  updated_at_micros: 1_772_960_000_000_000,
  preview: id,
});

describe("EntryQueryController", () => {
  beforeEach(() => queryMock.mockReset());

  it("traverses opaque keyset cursors without offset state", async () => {
    queryMock
      .mockResolvedValueOnce({ rows: [row("one")], has_more: true, next: "c1" })
      .mockResolvedValueOnce({ rows: [row("two")], has_more: true, next: "c2" })
      .mockResolvedValueOnce({ rows: [row("one")], has_more: false });

    const controller = createEntryQueryController(
      () => "space-1",
      undefined,
      undefined,
      50,
      queryMock,
    );
    await controller.load();
    await controller.next();
    await controller.previous();

    expect(queryMock).toHaveBeenNthCalledWith(1, "space-1", {
      query: { scope: { kind: "all" }, filters: [], sort: [] },
      projection: previewProjection(),
      limit: 50,
    });
    expect(queryMock).toHaveBeenNthCalledWith(2, "space-1", {
      query: { scope: { kind: "all" }, filters: [], sort: [] },
      projection: previewProjection(),
      limit: 50,
      after: "c1",
    });
    expect(queryMock).toHaveBeenNthCalledWith(3, "space-1", {
      query: { scope: { kind: "all" }, filters: [], sort: [] },
      projection: previewProjection(),
      limit: 50,
    });
    expect(controller.rows().map((current) => current.id)).toEqual(["one"]);
    expect(controller.canGoPrevious()).toBe(false);
  });

  it("starts a fresh chain for query semantics and keeps the page coordinate for projections", async () => {
    queryMock
      .mockResolvedValueOnce({ rows: [row("one")], has_more: true, next: "c1" })
      .mockResolvedValueOnce({ rows: [row("two")], has_more: true, next: "c2" })
      .mockResolvedValueOnce({ rows: [row("two")], has_more: true, next: "c2" })
      .mockResolvedValueOnce({
        rows: [row("one")],
        has_more: true,
        next: "c1",
      });

    const controller = createEntryQueryController(
      () => "space-1",
      undefined,
      undefined,
      50,
      queryMock,
    );
    await controller.load();
    await controller.next();
    controller.setText("alice");
    await vi.waitFor(() => expect(queryMock).toHaveBeenCalledTimes(3));
    controller.setProjection({
      kind: "fields",
      fields: [{ kind: "updated_at" }],
    });
    await vi.waitFor(() => expect(queryMock).toHaveBeenCalledTimes(4));

    expect(queryMock.mock.calls[2][1]).toMatchObject({
      query: {
        scope: { kind: "all" },
        text: "alice",
        filters: [],
        sort: [],
      },
      projection: previewProjection(),
      limit: 50,
    });
    expect(queryMock.mock.calls[3][1]).toMatchObject({
      query: {
        scope: { kind: "all" },
        text: "alice",
      },
      projection: { kind: "fields", fields: [{ kind: "updated_at" }] },
    });
  });
});
