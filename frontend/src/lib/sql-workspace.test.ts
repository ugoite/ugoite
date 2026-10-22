import { createRoot, createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";

const { planMock, queryMock, countMock } = vi.hoisted(() => ({
  planMock: vi.fn(),
  queryMock: vi.fn(),
  countMock: vi.fn(),
}));

vi.mock("~/lib/ugoite-client", () => ({
  sqlApi: {
    plan: planMock,
    query: queryMock,
    count: countMock,
  },
}));

import { createSqlWorkspaceController } from "./sql-workspace";

const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
};

describe("SQL workspace planning", () => {
  afterEach(() => {
    vi.useRealTimers();
    planMock.mockReset();
    queryMock.mockReset();
    countMock.mockReset();
  });

  it("debounces planning and sends typed nulls for declared arguments", async () => {
    vi.useFakeTimers();
    planMock.mockResolvedValue({ result_schema: [] });
    let controller!: ReturnType<typeof createSqlWorkspaceController>;
    let disposeRoot!: () => void;
    createRoot((dispose) => {
      disposeRoot = dispose;
      controller = createSqlWorkspaceController({
        spaceId: () => "space-one",
        sql: () => "SELECT $day",
        variables: () => [{ name: "day", type: "date" }],
      });
      return dispose;
    });

    await vi.advanceTimersByTimeAsync(349);
    expect(planMock).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await vi.waitFor(() => expect(planMock).toHaveBeenCalledOnce());
    expect(planMock).toHaveBeenCalledWith(
      "space-one",
      {
        sql: "SELECT $day",
        parameters: { day: null },
        parameter_types: { day: "date" },
      },
      expect.any(AbortSignal),
    );
    await vi.waitFor(() => expect(controller.plan()).toMatchObject({
      status: "success",
      isCurrent: true,
    }));
    disposeRoot();
  });

  it("discards a late plan result after SQL changes", async () => {
    vi.useFakeTimers();
    const first = deferred<{ result_schema: [] }>();
    const second = deferred<{ result_schema: [] }>();
    planMock.mockReturnValueOnce(first.promise).mockReturnValueOnce(
      second.promise,
    );
    let controller!: ReturnType<typeof createSqlWorkspaceController>;
    let setSql!: (sql: string) => void;
    let disposeRoot!: () => void;
    createRoot((dispose) => {
      disposeRoot = dispose;
      const [sql, updateSql] = createSignal("SELECT 1");
      setSql = updateSql;
      controller = createSqlWorkspaceController({
        spaceId: () => "space-one",
        sql,
        variables: () => [],
      });
      return dispose;
    });

    await vi.advanceTimersByTimeAsync(350);
    await vi.waitFor(() => expect(planMock).toHaveBeenCalledTimes(1));
    setSql("SELECT 2");
    expect(controller.plan()).toMatchObject({
      status: "pending",
      isCurrent: false,
    });
    await vi.advanceTimersByTimeAsync(350);
    await vi.waitFor(() => expect(planMock).toHaveBeenCalledTimes(2));

    first.resolve({ result_schema: [] });
    await Promise.resolve();
    expect(controller.plan()).toMatchObject({
      status: "pending",
      isCurrent: false,
    });

    second.resolve({ result_schema: [] });
    await vi.waitFor(() => expect(controller.plan()).toMatchObject({
      status: "success",
      isCurrent: true,
    }));
    expect(planMock.mock.calls[0][2].aborted).toBe(true);
    disposeRoot();
  });
});
