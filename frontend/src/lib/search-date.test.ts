import { afterEach, describe, expect, it, vi } from "vitest";
import { localInputToRfc3339Instant } from "./search-date";

describe("search date adapter", () => {
  afterEach(() => vi.unstubAllEnvs());

  it("converts datetime-local to an explicit instant in the browser timezone", () => {
    vi.stubEnv("TZ", "America/Los_Angeles");

    expect(localInputToRfc3339Instant("2026-03-08T01:30")).toBe(
      "2026-03-08T09:30:00.000Z",
    );
  });

  it("uses local calendar-day boundaries across the spring DST transition", () => {
    vi.stubEnv("TZ", "America/Los_Angeles");

    const from = localInputToRfc3339Instant("2026-03-08", "start");
    const to = localInputToRfc3339Instant("2026-03-08", "end");

    expect(from).toBe("2026-03-08T08:00:00.000Z");
    expect(to).toBe("2026-03-09T07:00:00.000Z");
    expect(new Date(to).getTime() - new Date(from).getTime()).toBe(
      23 * 60 * 60 * 1000,
    );
  });

  it("uses local calendar-day boundaries across the autumn DST transition", () => {
    vi.stubEnv("TZ", "America/New_York");

    const from = localInputToRfc3339Instant("2026-11-01", "start");
    const to = localInputToRfc3339Instant("2026-11-01", "end");

    expect(from).toBe("2026-11-01T04:00:00.000Z");
    expect(to).toBe("2026-11-02T05:00:00.000Z");
    expect(new Date(to).getTime() - new Date(from).getTime()).toBe(
      25 * 60 * 60 * 1000,
    );
  });
});
