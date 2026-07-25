import { describe, expect, it } from "vitest";
import type { Resource } from "solid-js";
import { recoverResource } from "./recoverable-resource";

function fakeResource<T>(value: T | undefined, error?: unknown): Resource<T> {
  const accessor = (() => {
    if (error) throw error;
    return value;
  }) as Resource<T>;
  Object.defineProperties(accessor, {
    state: { get: () => error ? "errored" : "ready" },
    loading: { get: () => false },
    error: { get: () => error },
    latest: { get: () => value },
  });
  return accessor;
}

describe("recoverResource", () => {
  it("preserves successful Resource values and properties", () => {
    const resource = recoverResource(fakeResource("ready"));

    expect(resource()).toBe("ready");
    expect(resource.state).toBe("ready");
    expect(resource.error).toBeUndefined();
  });

  it("returns undefined instead of rethrowing a Resource error", () => {
    const failure = new Error("request failed");
    const resource = recoverResource(fakeResource<string>(undefined, failure));

    expect(resource()).toBeUndefined();
    expect(resource.state).toBe("errored");
    expect(resource.error).toBe(failure);
  });
});
