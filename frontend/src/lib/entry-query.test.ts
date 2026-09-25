import { createRoot, createSignal } from "solid-js";
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
    }, expect.any(AbortSignal));
    expect(queryMock).toHaveBeenNthCalledWith(2, "space-1", {
      query: { scope: { kind: "all" }, filters: [], sort: [] },
      projection: previewProjection(),
      limit: 50,
      after: "c1",
    }, expect.any(AbortSignal));
    expect(queryMock).toHaveBeenNthCalledWith(3, "space-1", {
      query: { scope: { kind: "all" }, filters: [], sort: [] },
      projection: previewProjection(),
      limit: 50,
    }, expect.any(AbortSignal));
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

  it("clears stale rows while a fresh query chain is loading", async () => {
    let resolveNext:
      | ((page: { rows: never[]; has_more: boolean }) => void)
      | undefined;
    queryMock
      .mockResolvedValueOnce({ rows: [row("one")], has_more: false })
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          resolveNext = resolve;
        })
      );

    const controller = createEntryQueryController(
      () => "space-1",
      undefined,
      undefined,
      50,
      queryMock,
    );
    await controller.load();
    controller.setText("fresh");
    await vi.waitFor(() => expect(controller.loading()).toBe(true));

    expect(controller.rows()).toEqual([]);
    resolveNext?.({ rows: [], has_more: false });
    await vi.waitFor(() => expect(controller.loading()).toBe(false));
  });

  it("retries the failed page coordinate without treating the previous page as success", async () => {
    queryMock
      .mockResolvedValueOnce({ rows: [row("one")], has_more: true, next: "c1" })
      .mockRejectedValueOnce(new Error("temporary failure"))
      .mockResolvedValueOnce({ rows: [row("two")], has_more: false });

    const controller = createEntryQueryController(
      () => "space-1",
      undefined,
      undefined,
      50,
      queryMock,
    );
    await controller.load();
    await controller.next();
    expect(controller.rows().map((entry) => entry.id)).toEqual(["one"]);
    expect(controller.error()).toBeInstanceOf(Error);
    expect(controller.currentStart()).toBeUndefined();

    await controller.retry();

    expect(queryMock).toHaveBeenLastCalledWith(
      "space-1",
      expect.objectContaining({
        after: "c1",
      }),
      expect.any(AbortSignal),
    );
    expect(controller.rows().map((entry) => entry.id)).toEqual(["two"]);
    expect(controller.currentStart()).toBe("c1");
    expect(controller.error()).toBeNull();
  });

  it("aborts superseded reads and prevents their late results from replacing the current query", async () => {
    let resolveOld:
      | ((page: { rows: ReturnType<typeof row>[]; has_more: boolean }) => void)
      | undefined;
    queryMock
      .mockImplementationOnce((
        _space: string,
        _request: unknown,
        _signal: AbortSignal,
      ) =>
        new Promise((resolve) => {
          resolveOld = resolve;
        })
      )
      .mockResolvedValueOnce({ rows: [row("current")], has_more: false });

    const controller = createEntryQueryController(
      () => "space-1",
      undefined,
      undefined,
      50,
      queryMock,
    );
    void controller.load();
    const oldSignal = queryMock.mock.calls[0][2] as AbortSignal;
    controller.setText("current");

    await vi.waitFor(() => expect(queryMock).toHaveBeenCalledTimes(2));
    expect(oldSignal.aborted).toBe(true);
    resolveOld?.({ rows: [row("stale")], has_more: false });
    await vi.waitFor(() =>
      expect(controller.rows().map((entry) => entry.id))
        .toEqual(["current"])
    );
  });

  it("returns one first page from the measurement dataset without extra reads", async () => {
    const dataset = Array.from(
      { length: 120 },
      (_, index) => row(`entry-${index}`),
    );
    const measureQuery = vi.fn(async (
      _space: string,
      request: { limit: number },
      _signal: AbortSignal,
    ) => ({
      rows: dataset.slice(0, request.limit),
      has_more: dataset.length > request.limit,
      next: "measurement-next",
    }));
    const controller = createEntryQueryController(
      () => "measurement-space",
      undefined,
      undefined,
      50,
      measureQuery,
    );

    await controller.load();

    expect(controller.rows()).toHaveLength(50);
    expect(controller.hasMore()).toBe(true);
    expect(measureQuery).toHaveBeenCalledTimes(1);
  });

  it("aborts pending reads and discards their results when Space changes", async () => {
    let setSpace!: (spaceId: string) => void;
    let controller!: ReturnType<typeof createEntryQueryController>;
    let resolveOld:
      | ((page: { rows: ReturnType<typeof row>[]; has_more: boolean }) => void)
      | undefined;
    let dispose = () => {};
    queryMock
      .mockImplementationOnce(() =>
        new Promise((resolve) => {
          resolveOld = resolve;
        })
      )
      .mockResolvedValueOnce({ rows: [row("new-space")], has_more: false });

    createRoot((stop) => {
      dispose = stop;
      const [spaceId, updateSpace] = createSignal("space-old");
      setSpace = updateSpace;
      controller = createEntryQueryController(
        spaceId,
        undefined,
        undefined,
        50,
        queryMock,
      );
      void controller.load();
    });

    const oldSignal = queryMock.mock.calls[0][2] as AbortSignal;
    setSpace("space-new");
    await vi.waitFor(() => expect(queryMock).toHaveBeenCalledTimes(2));
    expect(oldSignal.aborted).toBe(true);
    resolveOld?.({ rows: [row("old-space")], has_more: false });
    await vi.waitFor(() =>
      expect(controller.rows().map((entry) => entry.id))
        .toEqual(["new-space"])
    );
    dispose();
  });
});
